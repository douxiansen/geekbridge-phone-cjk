// SPDX-License-Identifier: GPL-3.0-or-later
//! The background service. It owns the worker threads, keeps the shared
//! state the UI reads, and reports changes as events.
//!
//! Threads: phones (who is connected), calls (poll + track), messages (the
//! phone's single message session: poll, send, reconnect), contacts (phonebook
//! and call history), and a command dispatcher.

use crate::calls::{CallEvent, CallSnapshot, CallTracker, CallState};
use crate::contacts::{Contact, ContactBook};
use crate::conversations::{parse_map_time, Conversations, InboxWatcher, Message};
use crate::platform::{PhoneInfo, Platform};
use crate::recents::{Kind, Recent, Recents};
use crate::settings::Settings;
use crate::store::{self, Paths};
use crate::vcard;
use phone_obex::{Book, MapClient, PbapClient};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How a link to one of the phone's services is doing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LinkStatus {
    #[default]
    Idle,
    Connecting,
    Ready,
    /// The phone has not allowed access (answer the prompt on the phone).
    AccessDenied,
    Failed(String),
}

/// A call as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveCall {
    pub id: String,
    pub state: CallState,
    pub number: String,
    pub name: Option<String>,
    /// When it became active, for the timer.
    pub since: Option<Instant>,
}

/// Everything the UI reads. Lock it briefly and copy what you need.
#[derive(Debug, Default)]
pub struct State {
    pub phones: Vec<PhoneInfo>,
    /// The phone in use.
    pub phone: Option<PhoneInfo>,
    pub calls: Vec<ActiveCall>,
    pub contacts: ContactBook,
    pub contacts_status: LinkStatus,
    pub conversations: Conversations,
    pub messages_status: LinkStatus,
    pub recents: Recents,
    pub settings: Settings,
}

impl State {
    /// Contact name for a number, or the number.
    pub fn display_name(&self, number: &str) -> String {
        self.contacts.display(number)
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    /// The phone in use, or its state, changed.
    Phone(Option<PhoneInfo>),
    Call(CallEvent),
    CallsChanged,
    ContactsChanged,
    ConversationsChanged,
    RecentsChanged,
    /// A text arrived on the phone.
    TextReceived(Message),
    /// The outcome of [`Handle::send_text`].
    TextResult { token: u64, number: String, result: Result<(), String> },
    /// Something the user should know, in a sentence.
    Notice(String),
}

#[derive(Debug, Clone)]
pub enum Command {
    Dial(String),
    Answer(String),
    HangUp(String),
    HangUpAll,
    Tones(String),
    SendText { token: u64, number: String, text: String },
    ReloadContacts,
    /// Use this paired phone from now on.
    SelectPhone(String),
    SetSettings(Settings),
}

#[derive(Clone)]
pub struct Options {
    pub paths: Paths,
    pub phones_every: Duration,
    pub calls_every: Duration,
    pub reconnect_min: Duration,
    pub reconnect_max: Duration,
    /// How long to wait before retrying after the phone refused access.
    pub denied_retry: Duration,
    /// Called after every event, from a worker thread (to wake a UI loop).
    pub wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Options {
    pub fn new(paths: Paths) -> Self {
        Self {
            paths,
            phones_every: Duration::from_secs(3),
            calls_every: Duration::from_millis(500),
            reconnect_min: Duration::from_secs(2),
            reconnect_max: Duration::from_secs(60),
            denied_retry: Duration::from_secs(15),
            wake: None,
        }
    }
}

/// Wakes sleeping threads when the service stops.
struct Gate {
    stopped: Mutex<bool>,
    cv: Condvar,
}

impl Gate {
    /// Sleeps up to `d`; returns true if the service stopped.
    fn sleep(&self, d: Duration) -> bool {
        let guard = lock(&self.stopped);
        let (guard, _) = self.cv.wait_timeout_while(guard, d, |s| !*s).unwrap_or_else(|e| e.into_inner());
        *guard
    }
    fn is_stopped(&self) -> bool {
        *lock(&self.stopped)
    }
    fn stop(&self) {
        *lock(&self.stopped) = true;
        self.cv.notify_all();
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct Inner {
    platform: Arc<dyn Platform>,
    opts: Options,
    state: Mutex<State>,
    events: Sender<Event>,
    gate: Gate,
    cmd: Sender<Command>,
    next_token: Mutex<u64>,
}

impl Inner {
    fn emit(&self, e: Event) {
        let _ = self.events.send(e);
        if let Some(w) = &self.opts.wake {
            w();
        }
    }
    fn notice(&self, msg: impl Into<String>) {
        let msg = msg.into();
        log::warn!("{msg}");
        self.emit(Event::Notice(msg));
    }
    fn phone_dir(&self, address: &str) -> PathBuf {
        self.opts.paths.data.join("phones").join(address.replace(':', "_"))
    }
    fn settings_path(&self) -> PathBuf {
        self.opts.paths.config.join("settings.json")
    }
    fn settings(&self) -> Settings {
        lock(&self.state).settings.clone()
    }
    fn selected(&self) -> Option<PhoneInfo> {
        lock(&self.state).phone.clone()
    }
    fn selected_connected(&self) -> Option<PhoneInfo> {
        self.selected().filter(|p| p.connected)
    }
}

/// A cloneable handle for the UI.
#[derive(Clone)]
pub struct Handle {
    inner: Arc<Inner>,
}

impl Handle {
    pub fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.inner.state)
    }
    pub fn send(&self, c: Command) {
        let _ = self.inner.cmd.send(c);
    }
    pub fn dial(&self, number: &str) {
        self.send(Command::Dial(number.to_string()));
    }
    pub fn answer(&self, id: &str) {
        self.send(Command::Answer(id.to_string()));
    }
    pub fn hang_up(&self, id: &str) {
        self.send(Command::HangUp(id.to_string()));
    }
    /// Queues a text and returns a token that the matching `TextResult` carries.
    pub fn send_text(&self, number: &str, text: &str) -> u64 {
        let token = {
            let mut t = lock(&self.inner.next_token);
            *t += 1;
            *t
        };
        self.send(Command::SendText { token, number: number.to_string(), text: text.to_string() });
        token
    }
    pub fn set_settings(&self, s: Settings) {
        self.send(Command::SetSettings(s));
    }
}

/// The running service. Dropping it stops the workers.
pub struct Service {
    handle: Handle,
    threads: Vec<JoinHandle<()>>,
}

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Service {
    pub fn start(platform: Arc<dyn Platform>, opts: Options) -> (Self, Receiver<Event>) {
        let (events_tx, events_rx) = mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let settings: Settings = store::load::<Settings>(&opts.paths.config.join("settings.json")).sanitised();
        let inner = Arc::new(Inner {
            platform,
            opts,
            state: Mutex::new(State { settings, ..State::default() }),
            events: events_tx,
            gate: Gate { stopped: Mutex::new(false), cv: Condvar::new() },
            cmd: cmd_tx,
            next_token: Mutex::new(0),
        });
        let (msg_tx, msg_rx) = mpsc::channel::<(u64, String, String)>();
        let (contacts_tx, contacts_rx) = mpsc::channel::<()>();

        let mut threads = Vec::new();
        let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| thread::Builder::new().name(name.into()).spawn(f).expect("spawn thread");
        {
            let i = inner.clone();
            threads.push(spawn("gbp-phones", Box::new(move || phones_loop(i))));
        }
        {
            let i = inner.clone();
            threads.push(spawn("gbp-calls", Box::new(move || calls_loop(i))));
        }
        {
            let i = inner.clone();
            threads.push(spawn("gbp-messages", Box::new(move || messages_loop(i, msg_rx))));
        }
        {
            let i = inner.clone();
            threads.push(spawn("gbp-contacts", Box::new(move || contacts_loop(i, contacts_rx))));
        }
        {
            let i = inner.clone();
            threads.push(spawn("gbp-commands", Box::new(move || command_loop(i, cmd_rx, msg_tx, contacts_tx))));
        }
        (Self { handle: Handle { inner }, threads }, events_rx)
    }

    pub fn handle(&self) -> Handle {
        self.handle.clone()
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.handle.inner.gate.stop();
        // Wake the dispatcher, which waits on its channel.
        let _ = self.handle.inner.cmd.send(Command::ReloadContacts);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// --- phones ---------------------------------------------------------------

fn load_phone_data(inner: &Inner, address: &str) {
    let dir = inner.phone_dir(address);
    let contacts: Vec<Contact> = store::load(&dir.join("contacts.json"));
    let conversations: Conversations = store::load(&dir.join("conversations.json"));
    let recents: Recents = store::load(&dir.join("recents.json"));
    let mut st = lock(&inner.state);
    st.contacts = ContactBook::from_contacts(contacts);
    st.conversations = conversations;
    st.recents = recents;
}

fn phones_loop(inner: Arc<Inner>) {
    let mut warned = false;
    loop {
        match inner.platform.phones() {
            Ok(list) => {
                warned = false;
                let preferred = inner.settings().preferred_phone;
                let chosen = preferred
                    .as_ref()
                    .and_then(|a| list.iter().find(|p| &p.address == a).cloned())
                    .or_else(|| list.iter().find(|p| p.connected).cloned())
                    .or_else(|| list.first().cloned());
                let (changed, address_changed) = {
                    let st = lock(&inner.state);
                    (st.phone != chosen || st.phones != list, st.phone.as_ref().map(|p| &p.address) != chosen.as_ref().map(|p| &p.address))
                };
                if address_changed {
                    if let Some(p) = &chosen {
                        load_phone_data(&inner, &p.address);
                    }
                }
                if changed {
                    {
                        let mut st = lock(&inner.state);
                        st.phones = list;
                        st.phone = chosen.clone();
                    }
                    inner.emit(Event::Phone(chosen));
                    if address_changed {
                        inner.emit(Event::ContactsChanged);
                        inner.emit(Event::ConversationsChanged);
                        inner.emit(Event::RecentsChanged);
                    }
                }
            }
            Err(e) => {
                if !warned {
                    inner.notice(format!("Cannot list Bluetooth phones: {e}"));
                    warned = true;
                }
            }
        }
        if inner.gate.sleep(inner.opts.phones_every) {
            return;
        }
    }
}

// --- calls ----------------------------------------------------------------

fn calls_loop(inner: Arc<Inner>) {
    let started = Instant::now();
    let mut tracker = CallTracker::new();
    let mut shown: Vec<ActiveCall> = Vec::new();
    let mut warned = false;
    loop {
        if let Some(phone) = inner.selected_connected() {
            match inner.platform.calls(&phone.address) {
                Ok(snaps) => {
                    warned = false;
                    let now_ms = started.elapsed().as_millis() as u64;
                    let events = tracker.ingest(&snaps, now_ms);
                    let mut since: HashMap<String, u64> = HashMap::new();
                    for e in &events {
                        if let CallEvent::Changed { id, since_ms: Some(ms), .. } = e {
                            since.insert(id.clone(), *ms);
                        }
                    }
                    let next = build_calls(&inner, &snaps, &shown, &since, started);
                    for e in events {
                        if let CallEvent::Ended { number, direction, seconds, .. } = &e {
                            let mut st = lock(&inner.state);
                            st.recents.add(Recent { number: number.clone(), kind: Kind::from(direction), at: unix_now(), seconds: *seconds });
                            let _ = store::save(&inner.phone_dir(&phone.address).join("recents.json"), &st.recents);
                            drop(st);
                            inner.emit(Event::RecentsChanged);
                        }
                        inner.emit(Event::Call(e));
                    }
                    if next != shown {
                        lock(&inner.state).calls = next.clone();
                        shown = next;
                        inner.emit(Event::CallsChanged);
                    }
                }
                Err(e) => {
                    // No hands-free link yet is normal; say it once.
                    if !warned {
                        log::info!("calls unavailable: {e}");
                        warned = true;
                    }
                }
            }
        } else if !shown.is_empty() {
            shown.clear();
            lock(&inner.state).calls.clear();
            tracker = CallTracker::new();
            inner.emit(Event::CallsChanged);
        }
        if inner.gate.sleep(inner.opts.calls_every) {
            return;
        }
    }
}

fn build_calls(inner: &Inner, snaps: &[CallSnapshot], previous: &[ActiveCall], since: &HashMap<String, u64>, started: Instant) -> Vec<ActiveCall> {
    let st = lock(&inner.state);
    snaps
        .iter()
        .map(|s| {
            let kept = previous.iter().find(|p| p.id == s.id).and_then(|p| p.since);
            let from_event = since.get(&s.id).map(|ms| started + Duration::from_millis(*ms));
            ActiveCall {
                id: s.id.clone(),
                state: s.state,
                number: s.number.clone(),
                name: s.name.clone().or_else(|| st.contacts.name_for(&s.number).map(str::to_string)),
                since: kept.or(from_event),
            }
        })
        .collect()
}

// --- messages -------------------------------------------------------------

type MapSession = MapClient<Box<dyn crate::platform::Stream>>;

struct Link {
    address: String,
    map: MapSession,
}

fn messages_loop(inner: Arc<Inner>, rx: Receiver<(u64, String, String)>) {
    let mut link: Option<Link> = None;
    let mut watcher: Option<(String, InboxWatcher)> = None;
    let mut backoff = inner.opts.reconnect_min;
    let mut next_attempt = Instant::now();
    let mut next_poll = Instant::now();
    let mut told_denied = false;

    let set_status = |s: LinkStatus| {
        let mut st = lock(&inner.state);
        if st.messages_status != s {
            st.messages_status = s;
        }
    };

    loop {
        if inner.gate.is_stopped() {
            if let Some(l) = link.take() {
                let _ = l.map.close();
            }
            return;
        }
        let phone = inner.selected_connected();
        // A different (or absent) phone ends the session.
        if link.as_ref().map(|l| &l.address) != phone.as_ref().map(|p| &p.address) {
            if let Some(l) = link.take() {
                let _ = l.map.close();
            }
        }
        let Some(phone) = phone else {
            set_status(LinkStatus::Idle);
            // Answer sends with a clear failure while there is no phone.
            while let Ok((token, number, _)) = rx.try_recv() {
                inner.emit(Event::TextResult { token, number, result: Err("No phone is connected.".into()) });
            }
            if inner.gate.sleep(Duration::from_millis(500)) {
                return;
            }
            continue;
        };

        if link.is_none() {
            if Instant::now() < next_attempt {
                if inner.gate.sleep(Duration::from_millis(250)) {
                    return;
                }
                continue;
            }
            set_status(LinkStatus::Connecting);
            let opened = inner.platform.open_map(&phone.address).map_err(|e| e.to_string()).and_then(|s| {
                MapClient::connect(s).map_err(|e| if e.is_access_denied() { "denied".to_string() } else { e.to_string() })
            });
            match opened {
                Ok(map) => {
                    link = Some(Link { address: phone.address.clone(), map });
                    set_status(LinkStatus::Ready);
                    backoff = inner.opts.reconnect_min;
                    told_denied = false;
                    next_poll = Instant::now();
                    if watcher.as_ref().map(|(a, _)| a != &phone.address).unwrap_or(true) {
                        watcher = Some((phone.address.clone(), InboxWatcher::new(&lock(&inner.state).conversations)));
                    }
                }
                Err(e) => {
                    if e == "denied" {
                        set_status(LinkStatus::AccessDenied);
                        if !told_denied {
                            inner.notice("Allow message access for this computer on your phone, then it will connect.");
                            told_denied = true;
                        }
                        backoff = backoff.max(inner.opts.denied_retry);
                    } else {
                        set_status(LinkStatus::Failed(e.clone()));
                        log::info!("message link failed: {e}");
                    }
                    next_attempt = Instant::now() + backoff;
                    backoff = (backoff * 2).min(inner.opts.reconnect_max);
                    continue;
                }
            }
        }

        // Wait for a send request or the next poll, whichever comes first.
        let wait = next_poll.saturating_duration_since(Instant::now()).min(Duration::from_millis(500));
        match rx.recv_timeout(wait) {
            Ok((token, number, text)) => {
                let result = match link.as_mut() {
                    Some(l) => l.map.send_sms(&number, &text).map_err(|e| e.to_string()),
                    None => Err("Not connected to the phone's messages.".into()),
                };
                if result.is_err() {
                    // The session may be dead; reconnect on the next turn.
                    if let Some(l) = link.take() {
                        let _ = l.map.close();
                    }
                } else {
                    let mut st = lock(&inner.state);
                    st.conversations.add_pending(&number, &text, unix_now());
                    drop(st);
                    inner.emit(Event::ConversationsChanged);
                    // Look soon, to swap the pending copy for the phone's.
                    next_poll = Instant::now() + Duration::from_millis(1500);
                }
                inner.emit(Event::TextResult { token, number, result });
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }

        if Instant::now() >= next_poll {
            let every = Duration::from_secs(u64::from(inner.settings().inbox_poll_secs));
            next_poll = Instant::now() + every;
            let Some(l) = link.as_mut() else { continue };
            match poll_once(&mut l.map) {
                Ok((inbox, sent)) => {
                    let (changed, fresh) = {
                        let mut st = lock(&inner.state);
                        let fresh = watcher.as_mut().map(|(_, w)| w.take_new(&inbox)).unwrap_or_default();
                        let mut changed = st.conversations.ingest(&inbox, &sent);
                        changed |= st.conversations.expire_pending(unix_now());
                        if changed {
                            let _ = store::save(&inner.phone_dir(&phone.address).join("conversations.json"), &st.conversations);
                        }
                        (changed, fresh)
                    };
                    if changed {
                        inner.emit(Event::ConversationsChanged);
                    }
                    for m in fresh {
                        inner.emit(Event::TextReceived(m));
                    }
                }
                Err(e) => {
                    log::info!("message poll failed: {e}");
                    if let Some(l) = link.take() {
                        let _ = l.map.close();
                    }
                    set_status(LinkStatus::Failed(e.to_string()));
                    next_attempt = Instant::now() + backoff;
                }
            }
        }
    }
}

fn poll_once(map: &mut MapSession) -> Result<(Vec<phone_obex::MessageEntry>, Vec<phone_obex::MessageEntry>), phone_obex::ObexError> {
    map.set_folder("/telecom/msg/inbox")?;
    let inbox = map.list_messages(100, 255)?;
    map.set_folder("/telecom/msg/sent")?;
    let sent = map.list_messages(100, 255)?;
    Ok((inbox, sent))
}

// --- contacts -------------------------------------------------------------

fn pull_contacts(inner: &Inner, address: &str) -> Result<(), String> {
    let stream = inner.platform.open_pbap(address).map_err(|e| e.to_string())?;
    let mut pbap = PbapClient::connect(stream).map_err(|e| if e.is_access_denied() { "Allow contacts access for this computer on your phone.".to_string() } else { e.to_string() })?;
    let contacts_text = pbap.pull(Book::Contacts, 5000).map_err(|e| e.to_string())?;
    let book = ContactBook::from_cards(vcard::parse(&contacts_text));
    let mut history = Vec::new();
    for (b, kind) in [(Book::Received, Kind::Incoming), (Book::Dialed, Kind::Outgoing), (Book::Missed, Kind::Missed)] {
        // History is a nicety: a phone that refuses it still gives contacts.
        if let Ok(text) = pbap.pull(b, 200) {
            for card in vcard::parse(&text) {
                if let (Some(n), Some(t)) = (card.numbers.first(), card.call_time.as_deref().and_then(parse_map_time)) {
                    history.push(Recent { number: n.clone(), kind, at: t, seconds: 0 });
                }
            }
        }
    }
    let _ = pbap.close();

    let dir = inner.phone_dir(address);
    let mut st = lock(&inner.state);
    let _ = store::save(&dir.join("contacts.json"), &book.contacts().to_vec());
    st.contacts = book;
    for h in history {
        st.recents.add(h);
    }
    let _ = store::save(&dir.join("recents.json"), &st.recents);
    Ok(())
}

fn contacts_loop(inner: Arc<Inner>, rx: Receiver<()>) {
    let mut loaded_for: Option<String> = None;
    let mut last_try = Instant::now() - Duration::from_secs(3600);
    let mut failures = 0u32;
    loop {
        let requested = rx.recv_timeout(Duration::from_millis(500)).is_ok();
        if inner.gate.is_stopped() {
            return;
        }
        let Some(phone) = inner.selected_connected() else {
            loaded_for = None;
            continue;
        };
        let settings = inner.settings();
        let due = loaded_for.as_deref() != Some(phone.address.as_str()) && settings.load_contacts_on_connect;
        let retry_wait = Duration::from_secs(10 * 2u64.pow(failures.min(5)));
        if !(requested || (due && last_try.elapsed() >= retry_wait)) {
            continue;
        }
        last_try = Instant::now();
        lock(&inner.state).contacts_status = LinkStatus::Connecting;
        match pull_contacts(&inner, &phone.address) {
            Ok(()) => {
                failures = 0;
                loaded_for = Some(phone.address.clone());
                lock(&inner.state).contacts_status = LinkStatus::Ready;
                inner.emit(Event::ContactsChanged);
                inner.emit(Event::RecentsChanged);
            }
            Err(e) => {
                failures += 1;
                lock(&inner.state).contacts_status = LinkStatus::Failed(e.clone());
                log::info!("contacts pull failed: {e}");
                if failures == 1 {
                    inner.notice(format!("Could not read contacts: {e}"));
                }
            }
        }
    }
}

// --- commands -------------------------------------------------------------

fn command_loop(inner: Arc<Inner>, rx: Receiver<Command>, msg_tx: Sender<(u64, String, String)>, contacts_tx: Sender<()>) {
    while let Ok(cmd) = rx.recv() {
        if inner.gate.is_stopped() {
            return;
        }
        let phone = inner.selected_connected();
        let need_phone = || -> Result<PhoneInfo, String> { phone.clone().ok_or_else(|| "No phone is connected.".to_string()) };
        let result: Result<(), String> = match cmd {
            Command::Dial(n) => need_phone().and_then(|p| inner.platform.dial(&p.address, &n).map_err(|e| e.to_string())),
            Command::Answer(id) => need_phone().and_then(|p| inner.platform.answer(&p.address, &id).map_err(|e| e.to_string())),
            Command::HangUp(id) => need_phone().and_then(|p| inner.platform.hang_up(&p.address, &id).map_err(|e| e.to_string())),
            Command::HangUpAll => need_phone().and_then(|p| inner.platform.hang_up_all(&p.address).map_err(|e| e.to_string())),
            Command::Tones(t) => need_phone().and_then(|p| inner.platform.send_tones(&p.address, &t).map_err(|e| e.to_string())),
            Command::SendText { token, number, text } => {
                let _ = msg_tx.send((token, number, text));
                Ok(())
            }
            Command::ReloadContacts => {
                let _ = contacts_tx.send(());
                Ok(())
            }
            Command::SelectPhone(address) => {
                let mut s = inner.settings();
                s.preferred_phone = Some(address);
                apply_settings(&inner, s);
                Ok(())
            }
            Command::SetSettings(s) => {
                apply_settings(&inner, s.sanitised());
                Ok(())
            }
        };
        if let Err(e) = result {
            inner.notice(e);
        }
    }
}

fn apply_settings(inner: &Inner, s: Settings) {
    if let Err(e) = store::save(&inner.settings_path(), &s) {
        log::warn!("cannot save settings: {e}");
    }
    lock(&inner.state).settings = s;
}

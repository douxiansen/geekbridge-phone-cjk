// SPDX-License-Identifier: GPL-3.0-or-later
//! Shows calls and texts as macOS-style banners in the top-right corner, using the
//! `phone-overlay` crate, and routes what the user taps on them (Answer, Decline, Hang up,
//! quick reply, open) back to the phone and the window.

use crate::notify::{
    default_notifier, CallEndedNotice, CallNotice, CallUpdate, Notifier, NotifierFactory,
    NotifierHost, NotifyId, TextNotice,
};
use phone_core::calls::CallState as CoreCallState;
use phone_core::settings::ThemePref as CoreTheme;
use phone_overlay::{
    Action, Banner, CallBanner, CallState, OverlayConfig, OverlayHandle, ReplyStatus, TextBanner,
    ThemePref,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// What a banner stands for, so an action on it can be routed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Call { call_id: String },
    Text { number: String },
}

#[derive(Default)]
struct Ids {
    next: u64,
    by_notify: HashMap<NotifyId, u64>,
    targets: HashMap<u64, Target>,
    /// Quick replies in flight: the service's token to the banner it came from.
    replies: HashMap<u64, u64>,
}

impl Ids {
    fn id_for(&mut self, key: &NotifyId, target: Target) -> u64 {
        if let Some(id) = self.by_notify.get(key) {
            self.targets.insert(*id, target);
            return *id;
        }
        self.next += 1;
        let id = self.next;
        self.by_notify.insert(key.clone(), id);
        self.targets.insert(id, target);
        id
    }

    fn forget(&mut self, key: &NotifyId) -> Option<u64> {
        let id = self.by_notify.remove(key)?;
        self.targets.remove(&id);
        self.replies.retain(|_, banner| *banner != id);
        Some(id)
    }

    fn forget_id(&mut self, id: u64) {
        self.by_notify.retain(|_, v| *v != id);
        self.targets.remove(&id);
        self.replies.retain(|_, banner| *banner != id);
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A [`Notifier`] that draws banners with `phone-overlay`.
pub struct OverlayNotifier {
    overlay: OverlayHandle,
    ids: Arc<Mutex<Ids>>,
}

impl OverlayNotifier {
    /// Builds the notifier over an overlay and starts routing the overlay's actions.
    pub fn new(
        overlay: OverlayHandle,
        actions: std::sync::mpsc::Receiver<Action>,
        host: NotifierHost,
    ) -> Self {
        let ids = Arc::new(Mutex::new(Ids::default()));
        let (overlay2, ids2) = (overlay.clone(), ids.clone());
        let spawned = std::thread::Builder::new()
            .name("gbp-overlay-actions".into())
            .spawn(move || {
                while let Ok(action) = actions.recv() {
                    route(&action, &ids2, &overlay2, &host);
                }
            });
        if let Err(e) = spawned {
            log::warn!("cannot start the overlay action thread: {e}");
        }
        Self { overlay, ids }
    }

    fn key_for_call(call_id: &str) -> NotifyId {
        NotifyId::Call(call_id.to_string())
    }
}

/// Sends what the user did on a banner to the phone or the window.
fn route(action: &Action, ids: &Mutex<Ids>, overlay: &OverlayHandle, host: &NotifierHost) {
    let target = |id: u64| lock(ids).targets.get(&id).cloned();
    match action {
        Action::Answer(id) => {
            if let Some(Target::Call { call_id }) = target(*id) {
                host.answer(&call_id);
            }
        }
        Action::Decline(id) | Action::HangUp(id) => {
            if let Some(Target::Call { call_id }) = target(*id) {
                host.hang_up(&call_id);
            }
        }
        Action::Reply { id, number, text } => {
            let token = host.reply(number, text);
            lock(ids).replies.insert(token, *id);
            overlay.reply_status(*id, ReplyStatus::Sending);
        }
        Action::Open(id) => match target(*id) {
            Some(Target::Text { number }) => host.open_conversation(&number),
            _ => host.show_window(),
        },
        Action::Dismiss(id) => lock(ids).forget_id(*id),
        // The overlay may grow actions; ignore ones we do not know.
        #[allow(unreachable_patterns)]
        _ => {}
    }
}

impl Notifier for OverlayNotifier {
    fn incoming_call(&self, c: &CallNotice) {
        let id = lock(&self.ids).id_for(
            &c.id(),
            Target::Call {
                call_id: c.call_id.clone(),
            },
        );
        self.overlay.show(Banner::Call(CallBanner {
            id,
            name: c.name.clone().unwrap_or_default(),
            number: c.number.clone(),
            state: CallState::Incoming,
        }));
    }

    fn call_updated(&self, u: &CallUpdate) {
        let state = match u.state {
            CoreCallState::Active => CallState::Active {
                since: u.since.unwrap_or_else(Instant::now),
            },
            CoreCallState::Held => CallState::Held,
            CoreCallState::Dialing | CoreCallState::Alerting => CallState::Dialing,
            // Ringing and ended calls are other methods' business.
            _ => return,
        };
        let id = lock(&self.ids).id_for(
            &u.id(),
            Target::Call {
                call_id: u.call_id.clone(),
            },
        );
        self.overlay.show(Banner::Call(CallBanner {
            id,
            name: u.name.clone().unwrap_or_default(),
            number: u.number.clone(),
            state,
        }));
    }

    fn call_ended(&self, c: &CallEndedNotice) {
        if let Some(id) = lock(&self.ids).forget(&Self::key_for_call(&c.call_id)) {
            self.overlay.remove(id);
        }
        if c.missed {
            // A missed call deserves a note that outlives the ringing banner.
            let key = NotifyId::Text(format!("missed:{}", c.call_id));
            let id = lock(&self.ids).id_for(
                &key,
                Target::Text {
                    number: c.number.clone(),
                },
            );
            self.overlay.show(Banner::Text(TextBanner {
                id,
                name: c.name.clone().unwrap_or_default(),
                number: c.number.clone(),
                body: "Missed call".into(),
                can_reply: false,
            }));
        }
    }

    fn text(&self, t: &TextNotice) {
        let id = lock(&self.ids).id_for(
            &t.id(),
            Target::Text {
                number: t.number.clone(),
            },
        );
        self.overlay.show(Banner::Text(TextBanner {
            id,
            name: t.name.clone().unwrap_or_default(),
            number: t.number.clone(),
            body: t.body.clone(),
            can_reply: t.quick_reply,
        }));
    }

    fn clear(&self, id: &NotifyId) {
        if let Some(banner) = lock(&self.ids).forget(id) {
            self.overlay.remove(banner);
        }
    }

    fn reply_result(&self, token: u64, _number: &str, result: &Result<(), String>) {
        let Some(banner) = lock(&self.ids).replies.remove(&token) else {
            return; // a text sent from the window, not from a banner
        };
        self.overlay.reply_status(
            banner,
            match result {
                Ok(()) => ReplyStatus::Sent,
                Err(e) => ReplyStatus::Failed(e.clone()),
            },
        );
    }
}

fn theme_pref(t: CoreTheme) -> ThemePref {
    match t {
        CoreTheme::System => ThemePref::System,
        CoreTheme::Dark => ThemePref::Dark,
        CoreTheme::Light => ThemePref::Light,
    }
}

/// A factory for [`crate::RunOptions::notifier`]: banners where the compositor supports them
/// (`wlr-layer-shell`), freedesktop notifications otherwise.
pub fn factory() -> NotifierFactory {
    Box::new(|host: NotifierHost| -> Box<dyn Notifier> {
        let (theme, secs) = {
            let st = host.handle.state();
            (theme_pref(st.settings.theme), st.settings.text_banner_secs)
        };
        let config = OverlayConfig {
            theme,
            text_timeout: Duration::from_secs(u64::from(secs)),
            app_name: "Phone".to_string(),
        };
        match phone_overlay::spawn(config) {
            Ok((overlay, actions)) => Box::new(OverlayNotifier::new(overlay, actions, host)),
            Err(e) => {
                log::info!("no overlay ({e}); using desktop notifications");
                default_notifier(host)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use phone_overlay::Command;

    fn notifier() -> (OverlayNotifier, Arc<Mutex<Vec<Command>>>) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let s2 = sent.clone();
        let handle = OverlayHandle::from_sink(move |c| lock(&s2).push(c));
        // No actions are delivered in these tests.
        let ids = Arc::new(Mutex::new(Ids::default()));
        (
            OverlayNotifier {
                overlay: handle,
                ids,
            },
            sent,
        )
    }

    fn ring(id: &str) -> CallNotice {
        CallNotice {
            call_id: id.into(),
            number: "+49151".into(),
            name: Some("Mara".into()),
        }
    }

    #[test]
    fn ringing_shows_one_banner_and_answering_updates_it_in_place() {
        let (n, sent) = notifier();
        n.incoming_call(&ring("/c1"));
        n.call_updated(&CallUpdate {
            call_id: "/c1".into(),
            number: "+49151".into(),
            name: Some("Mara".into()),
            state: CoreCallState::Active,
            since: Some(Instant::now()),
        });
        let cmds = lock(&sent);
        let ids: Vec<u64> = cmds
            .iter()
            .filter_map(|c| match c {
                Command::Show(Banner::Call(b)) => Some(b.id),
                _ => None,
            })
            .collect();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], ids[1], "the same banner is replaced, not stacked");
        assert!(
            matches!(&cmds[0], Command::Show(Banner::Call(b)) if b.state == CallState::Incoming && b.name == "Mara")
        );
        assert!(
            matches!(&cmds[1], Command::Show(Banner::Call(b)) if matches!(b.state, CallState::Active { .. }))
        );
    }

    #[test]
    fn the_end_of_a_call_removes_its_banner_and_a_missed_one_leaves_a_note() {
        let (n, sent) = notifier();
        n.incoming_call(&ring("/c1"));
        n.call_ended(&CallEndedNotice {
            call_id: "/c1".into(),
            number: "+49151".into(),
            name: None,
            missed: true,
            seconds: 0,
        });
        let cmds = lock(&sent);
        assert!(cmds.iter().any(|c| matches!(c, Command::Remove(_))));
        assert!(cmds.iter().any(|c| matches!(c, Command::Show(Banner::Text(t)) if t.body == "Missed call" && !t.can_reply)));
    }

    #[test]
    fn texts_from_one_person_share_a_banner_and_offer_reply_when_enabled() {
        let (n, sent) = notifier();
        let t = |body: &str| TextNotice {
            number: "+4915112345678".into(),
            name: None,
            body: body.into(),
            at: 0,
            quick_reply: true,
            timeout_secs: 8,
        };
        n.text(&t("one"));
        n.text(&t("two"));
        let cmds = lock(&sent);
        let banners: Vec<&TextBanner> = cmds
            .iter()
            .filter_map(|c| match c {
                Command::Show(Banner::Text(b)) => Some(b),
                _ => None,
            })
            .collect();
        assert_eq!(banners.len(), 2);
        assert_eq!(banners[0].id, banners[1].id);
        assert_eq!(banners[1].body, "two");
        assert!(banners[1].can_reply);
    }

    #[test]
    fn reply_results_reach_only_the_banner_that_sent_them() {
        let (n, sent) = notifier();
        n.text(&TextNotice {
            number: "+1".into(),
            name: None,
            body: "x".into(),
            at: 0,
            quick_reply: true,
            timeout_secs: 8,
        });
        let banner = *lock(&n.ids).targets.keys().next().unwrap();
        lock(&n.ids).replies.insert(77, banner);
        n.reply_result(5, "+1", &Ok(())); // not ours
        n.reply_result(77, "+1", &Err("no link".into()));
        let cmds = lock(&sent);
        let statuses: Vec<&ReplyStatus> = cmds
            .iter()
            .filter_map(|c| match c {
                Command::ReplyStatus(_, s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(statuses.len(), 1);
        assert!(matches!(statuses[0], ReplyStatus::Failed(e) if e == "no link"));
    }

    #[test]
    fn clearing_removes_the_banner_once() {
        let (n, sent) = notifier();
        n.incoming_call(&ring("/c1"));
        n.clear(&NotifyId::Call("/c1".into()));
        n.clear(&NotifyId::Call("/c1".into()));
        assert_eq!(
            lock(&sent)
                .iter()
                .filter(|c| matches!(c, Command::Remove(_)))
                .count(),
            1
        );
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later

//! The platform-neutral overlay state machine: which banners exist, how far
//! their animations are, who is hovered or focused, and what Actions the
//! user's input produced. Time is always passed in, so everything here is
//! deterministic and testable without a display.

use std::time::{Duration, Instant};

use crate::anim::{self, Ease, Tween};
use crate::edit::{EditOutcome, TextField};
use crate::input::{ButtonState, Key};
use crate::layout::{
    self, geometry, hit_test, stack_tops, GeoInput, Geometry, Hit, Kind, BANNER_W, BANNER_X, EDGE,
    MAX_VISIBLE, PREVIEW_LINES, SEND_D, SHADOW_PAD, SURFACE_W,
};
use crate::model::{Action, Banner, CallState, Command, OverlayConfig, ReplyStatus, TextBanner};
use crate::text::{self, Style, Wt};

/// How long a "Sent" banner lingers before it goes away.
pub const SENT_LINGER: Duration = Duration::from_millis(1500);
/// Caret blink half-period.
pub const CARET_BLINK: Duration = Duration::from_millis(530);
/// Redraw interval for ambient animation (ripple dots).
pub const AMBIENT_FRAME: Duration = Duration::from_millis(50);
/// Upper bound on queued banners; the oldest are dropped beyond it.
const MAX_ENTRIES: usize = 32;
/// Time constant of the stack reflow smoothing.
const REFLOW_TAU: f32 = 0.055;
/// Font size of the reply field text.
pub const FIELD_FONT: f32 = 14.0;
/// Reply text left padding inside the field.
pub const FIELD_TEXT_X: f32 = 14.0;

struct Entry {
    banner: Banner,
    /// Wrapped body preview (text banners).
    lines: Vec<String>,
    created: Instant,
    appear: Tween,
    leaving: Option<Instant>,
    shown_y: f32,
    placed: bool,
    expand: Tween,
    status_t: Tween,
    hovered: bool,
    suppress_expand: bool,
    field: TextField,
    focused: bool,
    reply: Option<(ReplyStatus, Instant)>,
    dismiss_at: Option<Instant>,
    remaining: Duration,
    btn_hit: Option<Hit>,
    btn_hover: Tween,
    btn_press: Tween,
}

impl Entry {
    fn new(banner: Banner, now: Instant, timeout: Duration) -> Self {
        let mut e = Entry {
            banner,
            lines: Vec::new(),
            created: now,
            appear: Tween::at(0.0, now),
            leaving: None,
            shown_y: EDGE,
            placed: false,
            expand: Tween::at(0.0, now),
            status_t: Tween::at(0.0, now),
            hovered: false,
            suppress_expand: false,
            field: TextField::new(),
            focused: false,
            reply: None,
            dismiss_at: None,
            remaining: timeout,
            btn_hit: None,
            btn_hover: Tween::at(0.0, now),
            btn_press: Tween::at(0.0, now),
        };
        e.relayout_text();
        e
    }

    fn relayout_text(&mut self) {
        self.lines = match &self.banner {
            Banner::Text(t) => {
                let g = geometry(&GeoInput {
                    kind: Kind::Text {
                        can_reply: t.can_reply,
                    },
                    body_lines: PREVIEW_LINES,
                    expand: 0.0,
                    status: 0.0,
                });
                text::wrap_lines(&t.body, body_style(), g.text_col.w, PREVIEW_LINES)
            }
            Banner::Call(_) => Vec::new(),
        };
    }

    fn can_reply(&self) -> bool {
        matches!(
            &self.banner,
            Banner::Text(TextBanner {
                can_reply: true,
                ..
            })
        )
    }

    fn is_text(&self) -> bool {
        matches!(self.banner, Banner::Text(_))
    }

    fn kind(&self) -> Kind {
        match &self.banner {
            Banner::Call(c) => {
                if c.state == CallState::Incoming {
                    Kind::Ringing
                } else {
                    Kind::InCall
                }
            }
            Banner::Text(t) => Kind::Text {
                can_reply: t.can_reply,
            },
        }
    }

    fn geo_input(&self, now: Instant) -> GeoInput {
        GeoInput {
            kind: self.kind(),
            body_lines: self.lines.len(),
            expand: self.expand.value(now),
            status: self.status_t.value(now),
        }
    }

    fn target_input(&self) -> GeoInput {
        GeoInput {
            kind: self.kind(),
            body_lines: self.lines.len(),
            expand: self.expand.target(),
            status: self.status_t.target(),
        }
    }

    fn sending(&self) -> bool {
        matches!(self.reply, Some((ReplyStatus::Sending, _)))
    }

    /// Whether the auto-dismiss countdown is held.
    fn paused(&self) -> bool {
        self.hovered || !self.field.is_empty() || self.sending()
    }

    fn leave(&mut self, now: Instant) {
        if self.leaving.is_none() {
            self.leaving = Some(now);
            self.hovered = false;
            self.focused = false;
        }
    }
}

/// Style of a banner's body text. Shared with the renderer so wrapping and
/// drawing agree.
pub fn body_style() -> Style {
    Style::new(13.0, Wt::Regular)
}

/// Everything the renderer needs to draw one banner.
pub struct BannerView<'a> {
    /// The banner.
    pub banner: &'a Banner,
    /// Wrapped preview lines.
    pub lines: &'a [String],
    /// Geometry at the current animation values.
    pub geo: Geometry,
    /// Geometry inputs behind `geo`.
    pub input: GeoInput,
    /// Top-left in surface coordinates, slide offset included.
    pub x: f32,
    /// Top edge in surface coordinates.
    pub y: f32,
    /// Group opacity (fade in/out).
    pub alpha: f32,
    /// The button under the pointer.
    pub hover: Option<Hit>,
    /// The button being pressed.
    pub pressed: Option<Hit>,
    /// Animated hover level of `btn_hit`.
    pub btn_hover: f32,
    /// Animated press level of `btn_hit`.
    pub btn_press: f32,
    /// Which button the two levels belong to.
    pub btn_hit: Option<Hit>,
    /// Whether the pointer is over the banner.
    pub hovered: bool,
    /// The reply field.
    pub field: &'a TextField,
    /// Whether the field has the text cursor.
    pub focused: bool,
    /// Whether the caret is in the visible half of its blink.
    pub caret_on: bool,
    /// Reply status, if any.
    pub reply: Option<&'a ReplyStatus>,
    /// Time since the banner was first shown.
    pub age: Duration,
    /// Time since the call became active, for active calls.
    pub call_elapsed: Option<Duration>,
    /// Ripple loop phase `0.0..1.0`.
    pub ripple_phase: f32,
    /// App name for the chip row.
    pub app_name: &'a str,
}

/// What the presenter should do next to keep the display right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    /// Nothing is showing or changing: sleep until an event arrives.
    Idle,
    /// Animating: redraw on every display frame.
    Frame,
    /// Redraw (and tick) at this time.
    At(Instant),
}

/// The overlay's complete state.
pub struct OverlayState {
    text_timeout: Duration,
    app_name: String,
    entries: Vec<Entry>,
    now: Instant,
    last_tick: Instant,
    pointer: Option<(f32, f32)>,
    pressed: Option<(u64, Hit)>,
    caret_epoch: Instant,
}

impl OverlayState {
    /// A new, empty state at `now`.
    pub fn new(config: &OverlayConfig, now: Instant) -> Self {
        Self {
            text_timeout: config.text_timeout,
            app_name: config.app_name.clone(),
            entries: Vec::new(),
            now,
            last_tick: now,
            pointer: None,
            pressed: None,
            caret_epoch: now,
        }
    }

    /// The app name for the chip row.
    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    /// The time of the last update; the renderer reads animations at this time.
    pub fn now(&self) -> Instant {
        self.now
    }

    /// Whether no banner exists at all (the surface can be destroyed).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Ids of all banners, oldest first.
    pub fn ids(&self) -> Vec<u64> {
        self.entries.iter().map(|e| e.banner.id()).collect()
    }

    /// Whether any reply field wants the keyboard.
    pub fn wants_keyboard(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.leaving.is_none() && e.can_reply() && (e.focused || !e.field.is_empty()))
    }

    // ---- commands -------------------------------------------------------

    /// Apply a command from the app.
    pub fn apply(&mut self, cmd: Command, now: Instant) {
        self.advance_clock(now);
        match cmd {
            Command::Show(b) => self.show(b, now),
            Command::Remove(id) => {
                if let Some(e) = self.entry_mut(id) {
                    e.leave(now);
                }
            }
            Command::ReplyStatus(id, st) => self.set_reply_status(id, st, now),
        }
        self.sync(now);
    }

    fn show(&mut self, banner: Banner, now: Instant) {
        let id = banner.id();
        let timeout = self.text_timeout;
        if let Some(e) = self.entry_mut(id) {
            let same_text = e.banner == banner;
            e.banner = banner;
            e.relayout_text();
            if !same_text {
                e.remaining = timeout;
            }
            if e.leaving.take().is_some() {
                e.appear.retarget(1.0, now, anim::ENTER, Ease::Out);
            }
            return;
        }
        self.entries.push(Entry::new(banner, now, timeout));
        while self.entries.len() > MAX_ENTRIES {
            self.entries.remove(0);
        }
    }

    fn set_reply_status(&mut self, id: u64, st: ReplyStatus, now: Instant) {
        let Some(e) = self.entry_mut(id) else { return };
        if !e.can_reply() {
            return;
        }
        match st {
            ReplyStatus::Sent => {
                e.field.clear();
                e.dismiss_at = Some(now + SENT_LINGER);
            }
            ReplyStatus::Failed(_) | ReplyStatus::Sending => e.dismiss_at = None,
        }
        e.reply = Some((st, now));
    }

    fn entry_mut(&mut self, id: u64) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.banner.id() == id)
    }

    // ---- time -----------------------------------------------------------

    fn advance_clock(&mut self, now: Instant) {
        if now > self.now {
            self.now = now;
        }
    }

    /// Advance timers and animations to `now`. Returns the Actions that
    /// timeouts produced.
    pub fn tick(&mut self, now: Instant) -> Vec<Action> {
        self.advance_clock(now);
        let now = self.now;
        let dt = now
            .saturating_duration_since(self.last_tick)
            .min(Duration::from_secs(1));
        self.last_tick = now;
        let mut actions = Vec::new();

        let visible = self.visible_indices();
        for (i, e) in self.entries.iter_mut().enumerate() {
            if e.leaving.is_some() {
                continue;
            }
            let shown = visible.contains(&i);
            if shown && e.appear.target() < 1.0 {
                e.appear.retarget(1.0, now, anim::ENTER, Ease::Out);
            }
            if !shown {
                continue;
            }
            if let Some(at) = e.dismiss_at {
                if now >= at {
                    e.dismiss_at = None;
                    actions.push(Action::Dismiss(e.banner.id()));
                    e.leave(now);
                    continue;
                }
            }
            if e.is_text() && !e.paused() {
                e.remaining = e.remaining.saturating_sub(dt);
                if e.remaining.is_zero() {
                    actions.push(Action::Dismiss(e.banner.id()));
                    e.leave(now);
                }
            }
        }

        // Drop banners whose exit finished, and queued ones that were removed.
        let visible = self.visible_indices();
        let mut idx = 0;
        self.entries.retain(|e| {
            let i = idx;
            idx += 1;
            match e.leaving {
                Some(t) => visible.contains(&i) && now.saturating_duration_since(t) < anim::LEAVE,
                None => true,
            }
        });
        if let Some((id, _)) = self.pressed {
            if !self.entries.iter().any(|e| e.banner.id() == id) {
                self.pressed = None;
            }
        }

        self.sync(now);
        self.smooth_stack(dt);
        actions
    }

    /// Re-evaluate everything derived from the pointer and animation targets.
    fn sync(&mut self, now: Instant) {
        self.update_hover(now);
        let pointer_pressed = self.pressed;
        let hover = self.hover_hit(now);
        for e in &mut self.entries {
            let want = e.can_reply()
                && e.leaving.is_none()
                && !e.suppress_expand
                && (e.hovered || e.focused || !e.field.is_empty() || e.reply.is_some());
            e.expand
                .retarget(f32::from(u8::from(want)), now, anim::EXPAND, Ease::Out);
            let want_status = e.can_reply() && e.reply.is_some();
            e.status_t.retarget(
                f32::from(u8::from(want_status)),
                now,
                anim::EXPAND,
                Ease::Out,
            );

            let id = e.banner.id();
            let hovered_btn = match hover {
                Some((hid, h)) if hid == id && is_button(h) => Some(h),
                _ => None,
            };
            if let Some(h) = hovered_btn {
                if e.btn_hit != Some(h) {
                    e.btn_hit = Some(h);
                    e.btn_hover = Tween::at(0.0, now);
                    e.btn_press = Tween::at(0.0, now);
                }
            }
            let hov_t = f32::from(u8::from(hovered_btn.is_some() && hovered_btn == e.btn_hit));
            e.btn_hover.retarget(hov_t, now, anim::PRESS, Ease::Out);
            let pr_t = f32::from(u8::from(
                pointer_pressed == Some((id, e.btn_hit.unwrap_or(Hit::Body)))
                    && hovered_btn == e.btn_hit,
            ));
            e.btn_press.retarget(pr_t, now, anim::PRESS, Ease::Out);
        }
    }

    fn smooth_stack(&mut self, dt: Duration) {
        let now = self.now;
        let slots = self.slots(now, false);
        let k = 1.0 - (-dt.as_secs_f32() / REFLOW_TAU).exp();
        for (idx, top, _) in slots {
            let e = &mut self.entries[idx];
            if !e.placed {
                e.shown_y = top;
                e.placed = true;
            } else if (e.shown_y - top).abs() < 0.2 {
                e.shown_y = top;
            } else {
                e.shown_y += (top - e.shown_y) * k;
            }
        }
    }

    // ---- stack ----------------------------------------------------------

    /// Entry indices of the banners that are on screen, top banner first.
    fn visible_indices(&self) -> Vec<usize> {
        (0..self.entries.len()).rev().take(MAX_VISIBLE).collect()
    }

    /// (entry index, target top, height) for each visible banner.
    fn slots(&self, now: Instant, at_target: bool) -> Vec<(usize, f32, f32)> {
        let order = self.visible_indices();
        let heights: Vec<f32> = order
            .iter()
            .map(|&i| {
                let e = &self.entries[i];
                let input = if at_target {
                    e.target_input()
                } else {
                    e.geo_input(now)
                };
                geometry(&input).height
            })
            .collect();
        let tops = stack_tops(&heights);
        order
            .into_iter()
            .zip(tops)
            .zip(heights)
            .map(|((i, t), h)| (i, t, h))
            .collect()
    }

    /// Views of the visible banners, top banner first.
    pub fn views(&self) -> Vec<BannerView<'_>> {
        let now = self.now;
        let hover = self.hover_hit(now);
        self.slots(now, false)
            .into_iter()
            .map(|(idx, _, _)| {
                let e = &self.entries[idx];
                let input = e.geo_input(now);
                let geo = geometry(&input);
                let (x, y, alpha) = self.placement(e, now);
                let id = e.banner.id();
                let call_elapsed = match &e.banner {
                    Banner::Call(c) => match c.state {
                        CallState::Active { since } => Some(now.saturating_duration_since(since)),
                        _ => None,
                    },
                    Banner::Text(_) => None,
                };
                BannerView {
                    banner: &e.banner,
                    lines: &e.lines,
                    geo,
                    input,
                    x,
                    y,
                    alpha,
                    hover: match hover {
                        Some((hid, h)) if hid == id => Some(h),
                        _ => None,
                    },
                    pressed: match self.pressed {
                        Some((pid, h)) if pid == id => Some(h),
                        _ => None,
                    },
                    btn_hover: e.btn_hover.value(now),
                    btn_press: e.btn_press.value(now),
                    btn_hit: e.btn_hit,
                    hovered: e.hovered,
                    field: &e.field,
                    focused: e.focused,
                    caret_on: self.caret_visible(now),
                    reply: e.reply.as_ref().map(|(s, _)| s),
                    age: now.saturating_duration_since(e.created),
                    call_elapsed,
                    ripple_phase: anim::loop_phase(
                        now.saturating_duration_since(e.created),
                        anim::RIPPLE_PERIOD,
                    ),
                    app_name: &self.app_name,
                }
            })
            .collect()
    }

    /// (x, y, alpha) of a banner in surface coordinates.
    fn placement(&self, e: &Entry, now: Instant) -> (f32, f32, f32) {
        let travel = BANNER_W + EDGE + 8.0;
        let a = e.appear.value(now);
        let mut x = BANNER_X + (1.0 - a) * travel;
        let mut alpha = a;
        if let Some(t) = e.leaving {
            let p = now.saturating_duration_since(t).as_secs_f32() / anim::LEAVE.as_secs_f32();
            let l = anim::ease_in_cubic(p);
            x += l * travel;
            alpha *= 1.0 - l;
        }
        (x, e.shown_y, alpha.clamp(0.0, 1.0))
    }

    /// Size of the layer surface in logical pixels: big enough for the banners
    /// now and at the end of every running animation, so it only ever has to
    /// grow while animating and shrinks once things settle. `(0, 0)` when
    /// nothing is shown.
    pub fn surface_size(&self) -> (f32, f32) {
        let now = self.now;
        let cur = self.slots(now, false);
        let tgt = self.slots(now, true);
        if cur.is_empty() {
            return (0.0, 0.0);
        }
        let mut bottom: f32 = 0.0;
        for (idx, top, h) in cur.iter().chain(tgt.iter()) {
            let y = self.entries[*idx].shown_y.max(*top);
            bottom = bottom.max(y + h);
        }
        (SURFACE_W, (bottom + SHADOW_PAD).ceil())
    }

    /// Rectangles (surface coordinates) that should take pointer input;
    /// everything else is click-through.
    pub fn input_rects(&self) -> Vec<layout::Rect> {
        self.views()
            .into_iter()
            .filter(|v| v.alpha > 0.0)
            .map(|v| layout::Rect::new(v.x, v.y, BANNER_W, v.geo.height))
            .collect()
    }

    // ---- hover / hit-testing -------------------------------------------

    fn hit_at(&self, x: f32, y: f32, now: Instant) -> Option<(u64, Hit)> {
        for (idx, _, _) in self.slots(now, false) {
            let e = &self.entries[idx];
            if e.leaving.is_some() {
                continue;
            }
            let (bx, by, _) = self.placement(e, now);
            let input = e.geo_input(now);
            let geo = geometry(&input);
            // The close cross only exists while the pointer is over the banner,
            // which is already true when we are testing inside its rect.
            if let Some(h) = hit_test(&geo, &input, x - bx, y - by, e.is_text()) {
                return Some((e.banner.id(), h));
            }
        }
        None
    }

    /// The banner and part currently under the pointer, if any.
    pub fn hover(&self) -> Option<(u64, Hit)> {
        self.hover_hit(self.now)
    }

    fn hover_hit(&self, now: Instant) -> Option<(u64, Hit)> {
        let (x, y) = self.pointer?;
        self.hit_at(x, y, now)
    }

    fn update_hover(&mut self, now: Instant) {
        let over = self.hover_hit(now).map(|(id, _)| id);
        for e in &mut self.entries {
            let h = Some(e.banner.id()) == over && e.leaving.is_none();
            if h && !e.hovered && e.can_reply() && !e.suppress_expand {
                // Hovering a text banner puts the text cursor in its reply field.
                e.focused = true;
            }
            if !h && e.hovered {
                e.suppress_expand = false;
                if e.field.is_empty() {
                    e.focused = false;
                }
            }
            e.hovered = h;
        }
        // One focused field at a time: the hovered one wins.
        if let Some(over) = over {
            for e in &mut self.entries {
                if e.banner.id() != over && e.field.is_empty() {
                    e.focused = false;
                }
            }
        }
    }

    // ---- input ----------------------------------------------------------

    /// The pointer moved to (`x`, `y`) in surface coordinates.
    pub fn pointer_motion(&mut self, x: f32, y: f32, now: Instant) {
        self.advance_clock(now);
        self.pointer = Some((x, y));
        self.sync(now);
    }

    /// The pointer left the surface.
    pub fn pointer_leave(&mut self, now: Instant) {
        self.advance_clock(now);
        self.pointer = None;
        self.pressed = None;
        self.sync(now);
    }

    /// The primary button went down or up at the current pointer position.
    pub fn pointer_button(&mut self, state: ButtonState, now: Instant) -> Vec<Action> {
        self.advance_clock(now);
        let mut actions = Vec::new();
        let Some((x, y)) = self.pointer else {
            return actions;
        };
        let hit = self.hit_at(x, y, now);
        match state {
            ButtonState::Down => {
                self.pressed = hit;
                if let Some((id, Hit::Field)) = hit {
                    self.focus_field(id);
                }
            }
            ButtonState::Up => {
                let pressed = self.pressed.take();
                if pressed.is_some() && pressed == hit {
                    if let Some((id, h)) = hit {
                        self.activate(id, h, now, &mut actions);
                    }
                }
            }
        }
        self.sync(now);
        actions
    }

    fn focus_field(&mut self, id: u64) {
        for e in &mut self.entries {
            e.focused = e.banner.id() == id;
        }
        self.caret_epoch = self.now;
    }

    fn activate(&mut self, id: u64, hit: Hit, now: Instant, out: &mut Vec<Action>) {
        match hit {
            Hit::Answer => out.push(Action::Answer(id)),
            Hit::Decline => out.push(Action::Decline(id)),
            Hit::HangUp => out.push(Action::HangUp(id)),
            Hit::Close => {
                out.push(Action::Dismiss(id));
                if let Some(e) = self.entry_mut(id) {
                    e.leave(now);
                }
            }
            Hit::Send => self.submit(id, out),
            Hit::Field => {}
            Hit::Body => {
                out.push(Action::Open(id));
                if let Some(e) = self.entry_mut(id) {
                    if e.is_text() {
                        e.leave(now);
                    }
                }
            }
        }
    }

    fn submit(&mut self, id: u64, out: &mut Vec<Action>) {
        let now = self.now;
        let Some(e) = self.entry_mut(id) else { return };
        let Banner::Text(t) = &e.banner else { return };
        if e.field.is_empty() || e.sending() {
            return;
        }
        out.push(Action::Reply {
            id,
            number: t.number.clone(),
            text: e.field.text().to_owned(),
        });
        // Optimistic: the app confirms with `reply_status`.
        e.reply = Some((ReplyStatus::Sending, now));
        e.dismiss_at = None;
    }

    /// A key for the focused reply field.
    pub fn key(&mut self, key: Key, now: Instant) -> Vec<Action> {
        self.advance_clock(now);
        let mut actions = Vec::new();
        let target = self
            .entries
            .iter()
            .position(|e| e.leaving.is_none() && e.can_reply() && e.focused)
            .or_else(|| {
                self.entries
                    .iter()
                    .position(|e| e.leaving.is_none() && e.can_reply() && !e.field.is_empty())
            });
        let Some(i) = target else { return actions };
        let id = self.entries[i].banner.id();
        if self.entries[i].sending() && !matches!(key, Key::Escape) {
            return actions;
        }
        let outcome = self.entries[i].field.apply(&key);
        match outcome {
            EditOutcome::Changed => {
                self.entries[i].focused = true;
                // Typing again clears an earlier failure message.
                if matches!(self.entries[i].reply, Some((ReplyStatus::Failed(_), _))) {
                    self.entries[i].reply = None;
                }
                self.caret_epoch = now;
                self.scroll_to_caret(i);
            }
            EditOutcome::Submit => self.submit(id, &mut actions),
            EditOutcome::Cancel => {
                let e = &mut self.entries[i];
                e.field.clear();
                e.focused = false;
                e.suppress_expand = true;
                if !matches!(e.reply, Some((ReplyStatus::Sent, _))) {
                    e.reply = None;
                }
            }
            EditOutcome::None => {}
        }
        self.sync(now);
        actions
    }

    /// The window system took the keyboard away.
    pub fn keyboard_focus_lost(&mut self, now: Instant) {
        self.advance_clock(now);
        for e in &mut self.entries {
            if e.field.is_empty() && !e.hovered {
                e.focused = false;
            }
        }
        self.sync(now);
    }

    fn scroll_to_caret(&mut self, i: usize) {
        let input = self.entries[i].target_input();
        let Some(field) = geometry(&input).field else {
            return;
        };
        let visible = field.w - FIELD_TEXT_X - SEND_D - 14.0;
        let style = Style::new(FIELD_FONT, Wt::Regular);
        let e = &mut self.entries[i];
        let caret = text::measure(e.field.before_cursor(), style);
        let total = text::measure(e.field.text(), style);
        let mut s = e.field.scroll;
        if caret - s > visible {
            s = caret - visible;
        }
        if caret < s {
            s = caret;
        }
        if total <= visible {
            s = 0.0;
        }
        e.field.scroll = s.max(0.0);
    }

    fn caret_visible(&self, now: Instant) -> bool {
        let t = now.saturating_duration_since(self.caret_epoch);
        (t.as_millis() / CARET_BLINK.as_millis()).is_multiple_of(2)
    }

    // ---- scheduling -----------------------------------------------------

    /// Whether something is moving that needs a redraw every display frame.
    pub fn is_animating(&self) -> bool {
        let now = self.now;
        let slots = self.slots(now, true);
        let order = self.visible_indices();
        order.iter().any(|&i| {
            let e = &self.entries[i];
            e.leaving.is_some()
                || e.appear.is_running(now)
                || e.expand.is_running(now)
                || e.status_t.is_running(now)
                || e.btn_hover.is_running(now)
                || e.btn_press.is_running(now)
                || !e.placed
        }) || slots
            .iter()
            .any(|(i, top, _)| (self.entries[*i].shown_y - top).abs() > 0.2)
            || self
                .entries
                .iter()
                .enumerate()
                .any(|(i, e)| !order.contains(&i) && e.leaving.is_some())
    }

    /// When the presenter must wake up next.
    pub fn next_wake(&self) -> Wake {
        if self.entries.is_empty() {
            return Wake::Idle;
        }
        if self.is_animating() {
            return Wake::Frame;
        }
        let now = self.now;
        let mut at: Option<Instant> = None;
        let mut want = |t: Instant| {
            at = Some(at.map_or(t, |a| a.min(t)));
        };
        for i in self.visible_indices() {
            let e = &self.entries[i];
            match &e.banner {
                Banner::Call(c) => match c.state {
                    CallState::Incoming | CallState::Dialing => want(now + AMBIENT_FRAME),
                    CallState::Active { since } => {
                        let secs = now.saturating_duration_since(since).as_secs() + 1;
                        want(since + Duration::from_secs(secs));
                    }
                    CallState::Held => {}
                },
                Banner::Text(_) => {
                    if !e.paused() {
                        want(now + e.remaining);
                    }
                    if e.focused {
                        let t = now.saturating_duration_since(self.caret_epoch);
                        let n = t.as_millis() / CARET_BLINK.as_millis() + 1;
                        want(self.caret_epoch + CARET_BLINK * n as u32);
                    }
                    if let Some(d) = e.dismiss_at {
                        want(d);
                    }
                }
            }
        }
        match at {
            Some(t) => Wake::At(t),
            None => Wake::Idle,
        }
    }
}

fn is_button(h: Hit) -> bool {
    matches!(
        h,
        Hit::Answer | Hit::Decline | Hit::HangUp | Hit::Send | Hit::Close
    )
}

#[cfg(test)]
mod tests;

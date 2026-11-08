// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::model::{CallBanner, ThemePref};

fn cfg() -> OverlayConfig {
    OverlayConfig {
        theme: ThemePref::Dark,
        text_timeout: Duration::from_secs(8),
        app_name: "Phone".into(),
    }
}

fn call(id: u64, state: CallState) -> Banner {
    Banner::Call(CallBanner {
        id,
        name: "Ada Lovelace".into(),
        number: "+1 555 0100".into(),
        state,
    })
}

fn text_banner(id: u64, can_reply: bool) -> Banner {
    Banner::Text(TextBanner {
        id,
        name: "Grace Hopper".into(),
        number: "+1 555 0199".into(),
        body: "Are we still on for lunch? I found a great place near the harbour.".into(),
        can_reply,
    })
}

struct Rig {
    st: OverlayState,
    now: Instant,
}

impl Rig {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            st: OverlayState::new(&cfg(), now),
            now,
        }
    }

    fn show(&mut self, b: Banner) {
        self.st.apply(Command::Show(b), self.now);
    }

    /// Advance in 16 ms frames, collecting actions.
    fn run(&mut self, ms: u64) -> Vec<Action> {
        let mut out = Vec::new();
        let end = self.now + Duration::from_millis(ms);
        while self.now < end {
            self.now += Duration::from_millis(16).min(end - self.now);
            out.extend(self.st.tick(self.now));
        }
        out
    }

    fn settle(&mut self) -> Vec<Action> {
        self.run(400)
    }

    fn move_to(&mut self, x: f32, y: f32) {
        self.st.pointer_motion(x, y, self.now);
    }

    fn click(&mut self) -> Vec<Action> {
        let mut a = self.st.pointer_button(ButtonState::Down, self.now);
        a.extend(self.st.pointer_button(ButtonState::Up, self.now));
        a
    }

    /// Surface coordinates of the centre of `r` (banner-local) on banner `id`.
    fn centre_of(&self, id: u64, pick: impl Fn(&BannerView<'_>) -> Option<Rect>) -> (f32, f32) {
        let views = self.st.views();
        let v = views
            .iter()
            .find(|v| v.banner.id() == id)
            .expect("banner visible");
        let r = pick(v).expect("rect");
        let (cx, cy) = r.center();
        (v.x + cx, v.y + cy)
    }
}

use crate::layout::Rect;

#[test]
fn nothing_shown_is_idle_and_empty() {
    let r = Rig::new();
    assert!(r.st.is_empty());
    assert_eq!(r.st.surface_size(), (0.0, 0.0));
    assert_eq!(r.st.next_wake(), Wake::Idle);
    assert!(r.st.input_rects().is_empty());
    assert!(!r.st.is_animating());
}

#[test]
fn banner_slides_in_then_settles() {
    let mut r = Rig::new();
    r.show(call(1, CallState::Incoming));
    r.run(16);
    let v0 = &r.st.views()[0];
    assert!(v0.x > BANNER_X + 100.0, "starts off to the right: {}", v0.x);
    assert!(v0.alpha < 0.5);
    assert!(r.st.is_animating());
    assert_eq!(r.st.next_wake(), Wake::Frame);
    r.settle();
    let v = &r.st.views()[0];
    assert!((v.x - BANNER_X).abs() < 0.01);
    assert_eq!(v.alpha, 1.0);
    assert!(!r.st.is_animating());
    let (w, h) = r.st.surface_size();
    assert_eq!(w, SURFACE_W);
    assert!(h > v.geo.height);
}

#[test]
fn remove_slides_out_and_frees_the_surface() {
    let mut r = Rig::new();
    r.show(call(1, CallState::Incoming));
    r.settle();
    r.st.apply(Command::Remove(1), r.now);
    r.run(100);
    assert!(!r.st.is_empty(), "still animating out");
    assert!(r.st.views()[0].alpha < 1.0);
    r.run(200);
    assert!(r.st.is_empty());
    assert_eq!(r.st.next_wake(), Wake::Idle);
    // Removing an unknown id is harmless.
    r.st.apply(Command::Remove(99), r.now);
}

#[test]
fn calls_have_no_timeout() {
    let mut r = Rig::new();
    r.show(call(1, CallState::Incoming));
    let actions = r.run(60_000);
    assert!(actions.is_empty());
    assert_eq!(r.st.ids(), vec![1]);
}

#[test]
fn text_times_out_with_dismiss() {
    let mut r = Rig::new();
    r.show(text_banner(5, true));
    assert!(r.run(7_500).is_empty());
    let a = r.run(1_000);
    assert_eq!(a, vec![Action::Dismiss(5)]);
    r.run(400);
    assert!(r.st.is_empty());
}

#[test]
fn hover_pauses_the_timeout() {
    let mut r = Rig::new();
    r.show(text_banner(5, true));
    r.settle();
    let (x, y) = r.centre_of(5, |v| Some(Rect::new(0.0, 0.0, BANNER_W, v.geo.height)));
    r.move_to(x, y);
    assert!(r.run(30_000).is_empty(), "hovered banners never time out");
    r.st.pointer_leave(r.now);
    assert!(r.run(7_000).is_empty());
    assert_eq!(r.run(1_500), vec![Action::Dismiss(5)]);
}

#[test]
fn hover_reveals_and_focuses_the_reply_field() {
    let mut r = Rig::new();
    r.show(text_banner(5, true));
    r.settle();
    let collapsed = r.st.views()[0].geo.height;
    assert!(!r.st.wants_keyboard());
    let (x, y) = r.centre_of(5, |v| Some(Rect::new(0.0, 0.0, 10.0, v.geo.height)));
    r.move_to(x + 20.0, y);
    assert!(r.st.wants_keyboard(), "hover focuses the field");
    r.settle();
    let v = &r.st.views()[0];
    assert!(v.geo.height > collapsed + 30.0, "expanded");
    assert!(v.focused);
    // Surface grows ahead of the animation.
    r.st.pointer_leave(r.now);
    r.settle();
    assert!(!r.st.wants_keyboard());
    assert_eq!(r.st.views()[0].geo.height, collapsed);
}

#[test]
fn banners_without_reply_do_not_expand() {
    let mut r = Rig::new();
    r.show(text_banner(5, false));
    r.settle();
    let h = r.st.views()[0].geo.height;
    let (x, y) = r.centre_of(5, |v| Some(Rect::new(0.0, 0.0, 10.0, v.geo.height)));
    r.move_to(x + 20.0, y);
    r.settle();
    assert_eq!(r.st.views()[0].geo.height, h);
    assert!(!r.st.wants_keyboard());
}

#[test]
fn quick_reply_flow() {
    let mut r = Rig::new();
    r.show(text_banner(5, true));
    r.settle();
    let (x, y) = r.centre_of(5, |v| Some(Rect::new(0.0, 0.0, 10.0, v.geo.height)));
    r.move_to(x + 20.0, y);
    r.settle();

    assert!(r
        .st
        .key(Key::Text("Yes, h\u{e9}las!".into()), r.now)
        .is_empty());
    // Enter sends.
    let a = r.st.key(Key::Enter, r.now);
    assert_eq!(
        a,
        vec![Action::Reply {
            id: 5,
            number: "+1 555 0199".into(),
            text: "Yes, h\u{e9}las!".into()
        }]
    );
    assert!(matches!(r.st.views()[0].reply, Some(ReplyStatus::Sending)));
    // Typing is ignored while sending; a second Enter does not double-send.
    assert!(r.st.key(Key::Enter, r.now).is_empty());

    r.st.apply(Command::ReplyStatus(5, ReplyStatus::Sent), r.now);
    assert!(r.st.views()[0].field.is_empty());
    assert!(r.run(1_300).is_empty(), "lingers after Sent");
    // Sent holds the banner for ~1.5 s even while the pointer is over it.
    let a = r.run(400);
    assert_eq!(a, vec![Action::Dismiss(5)]);
    r.run(400);
    assert!(r.st.is_empty());
}

#[test]
fn failed_reply_keeps_the_text_for_a_retry() {
    let mut r = Rig::new();
    r.show(text_banner(5, true));
    r.settle();
    let (x, y) = r.centre_of(5, |v| Some(Rect::new(0.0, 0.0, 10.0, v.geo.height)));
    r.move_to(x + 20.0, y);
    r.st.key(Key::Text("hi".into()), r.now);
    r.st.key(Key::Enter, r.now);
    r.st.apply(
        Command::ReplyStatus(5, ReplyStatus::Failed("No link".into())),
        r.now,
    );
    assert_eq!(r.st.views()[0].field.text(), "hi");
    assert!(r.run(20_000).is_empty(), "typed text keeps the banner up");
    // Typing clears the error and a retry works.
    r.st.key(Key::Text("!".into()), r.now);
    assert!(r.st.views()[0].reply.is_none());
    let a = r.st.key(Key::Enter, r.now);
    assert!(matches!(a[0], Action::Reply { .. }));
}

#[test]
fn empty_reply_is_not_sent_and_escape_cancels() {
    let mut r = Rig::new();
    r.show(text_banner(5, true));
    r.settle();
    let (x, y) = r.centre_of(5, |v| Some(Rect::new(0.0, 0.0, 10.0, v.geo.height)));
    r.move_to(x + 20.0, y);
    assert!(r.st.key(Key::Enter, r.now).is_empty());
    r.st.key(Key::Text("draft".into()), r.now);
    r.st.key(Key::Escape, r.now);
    assert!(r.st.views()[0].field.is_empty());
    assert!(!r.st.views()[0].focused);
    r.settle();
    // Esc keeps it collapsed even though the pointer is still there.
    let collapsed = geometry(&GeoInput {
        kind: Kind::Text { can_reply: true },
        body_lines: r.st.views()[0].lines.len(),
        expand: 0.0,
        status: 0.0,
    })
    .height;
    assert_eq!(r.st.views()[0].geo.height, collapsed);
}

#[test]
fn call_buttons_emit_actions() {
    let mut r = Rig::new();
    r.show(call(1, CallState::Incoming));
    r.settle();
    for (hit, expected) in [
        (Hit::Answer, Action::Answer(1)),
        (Hit::Decline, Action::Decline(1)),
    ] {
        let (x, y) = r.centre_of(1, |v| {
            v.geo
                .buttons
                .iter()
                .find(|(h, _)| *h == hit)
                .map(|(_, r)| *r)
        });
        r.move_to(x, y);
        assert_eq!(r.st.views()[0].hover, Some(hit));
        assert_eq!(r.click(), vec![expected]);
    }
    // Calls stay up until the app removes them.
    r.show(call(1, CallState::Active { since: r.now }));
    r.settle();
    let (x, y) = r.centre_of(1, |v| v.geo.buttons.first().map(|(_, r)| *r));
    r.move_to(x, y);
    assert_eq!(r.click(), vec![Action::HangUp(1)]);
    assert_eq!(r.st.ids(), vec![1]);
}

#[test]
fn press_must_be_released_on_the_same_target() {
    let mut r = Rig::new();
    r.show(call(1, CallState::Incoming));
    r.settle();
    let (ax, ay) = r.centre_of(1, |v| v.geo.buttons.get(1).map(|(_, r)| *r));
    let (dx, dy) = r.centre_of(1, |v| v.geo.buttons.first().map(|(_, r)| *r));
    r.move_to(ax, ay);
    assert!(r.st.pointer_button(ButtonState::Down, r.now).is_empty());
    assert!(r.st.views()[0].pressed == Some(Hit::Answer));
    r.move_to(dx, dy);
    assert!(r.st.pointer_button(ButtonState::Up, r.now).is_empty());
}

#[test]
fn clicking_outside_banners_does_nothing() {
    let mut r = Rig::new();
    r.show(call(1, CallState::Incoming));
    r.settle();
    r.move_to(2.0, 2.0);
    assert!(r.click().is_empty());
    assert!(r.st.views()[0].hover.is_none());
}

#[test]
fn body_click_opens_and_closes_text_banners() {
    let mut r = Rig::new();
    r.show(text_banner(5, false));
    r.settle();
    let (x, y) = r.centre_of(5, |v| Some(v.geo.text_col));
    r.move_to(x, y);
    assert_eq!(r.click(), vec![Action::Open(5)]);
    r.run(400);
    assert!(r.st.is_empty());
}

#[test]
fn close_button_dismisses() {
    let mut r = Rig::new();
    r.show(text_banner(5, true));
    r.settle();
    let (x, y) = r.centre_of(5, |v| v.geo.close);
    r.move_to(x, y);
    r.settle();
    let (x, y) = r.centre_of(5, |v| v.geo.close);
    r.move_to(x, y);
    assert_eq!(r.click(), vec![Action::Dismiss(5)]);
    r.run(400);
    assert!(r.st.is_empty());
}

#[test]
fn stack_is_newest_first_and_capped() {
    let mut r = Rig::new();
    for id in 1..=4 {
        r.show(call(id, CallState::Incoming));
        r.run(50);
    }
    r.settle();
    let views = r.st.views();
    assert_eq!(views.len(), MAX_VISIBLE);
    let ids: Vec<u64> = views.iter().map(|v| v.banner.id()).collect();
    assert_eq!(ids, vec![4, 3, 2], "newest on top, oldest queued");
    assert!(views[0].y < views[1].y && views[1].y < views[2].y);
    let gap = views[1].y - (views[0].y + views[0].geo.height);
    assert!((gap - layout::GAP).abs() < 0.3, "gap {gap}");
    assert_eq!(r.st.input_rects().len(), 3);

    // Removing the top one lets the queued one in.
    r.st.apply(Command::Remove(4), r.now);
    r.run(500);
    let ids: Vec<u64> = r.st.views().iter().map(|v| v.banner.id()).collect();
    assert_eq!(ids, vec![3, 2, 1]);
}

#[test]
fn queued_text_banners_wait_before_counting_down() {
    let mut r = Rig::new();
    r.show(text_banner(1, false));
    for id in 2..=4 {
        r.show(call(id, CallState::Incoming));
    }
    // Banner 1 is queued behind three calls: its 8 s have not started.
    assert!(r.run(20_000).is_empty());
    r.st.apply(Command::Remove(4), r.now);
    assert!(r.run(7_000).is_empty());
    assert_eq!(r.run(2_000), vec![Action::Dismiss(1)]);
}

#[test]
fn show_with_same_id_replaces() {
    let mut r = Rig::new();
    r.show(call(1, CallState::Incoming));
    r.settle();
    r.show(call(1, CallState::Active { since: r.now }));
    assert_eq!(r.st.ids(), vec![1]);
    let v = &r.st.views()[0];
    assert!(matches!(
        v.banner,
        Banner::Call(CallBanner {
            state: CallState::Active { .. },
            ..
        })
    ));
    assert!(v.call_elapsed.is_some());
    // Showing a leaving banner again revives it.
    r.st.apply(Command::Remove(1), r.now);
    r.run(50);
    r.show(call(1, CallState::Incoming));
    r.run(600);
    assert_eq!(r.st.ids(), vec![1]);
}

#[test]
fn active_call_wakes_once_a_second_and_ringing_animates() {
    let mut r = Rig::new();
    r.show(call(1, CallState::Active { since: r.now }));
    r.settle();
    match r.st.next_wake() {
        Wake::At(t) => {
            let d = t.saturating_duration_since(r.now);
            assert!(d <= Duration::from_secs(1), "{d:?}");
        }
        other => panic!("expected a timed wake, got {other:?}"),
    }
    r.st.apply(Command::Remove(1), r.now);
    r.run(400);
    r.show(call(2, CallState::Dialing));
    r.settle();
    match r.st.next_wake() {
        Wake::At(t) => assert!(t.saturating_duration_since(r.now) <= AMBIENT_FRAME),
        other => panic!("expected ripple wake, got {other:?}"),
    }
}

#[test]
fn surface_never_shrinks_below_the_content_while_animating() {
    let mut r = Rig::new();
    r.show(text_banner(5, true));
    r.settle();
    let (_, h0) = r.st.surface_size();
    let (x, y) = r.centre_of(5, |v| Some(Rect::new(0.0, 0.0, 10.0, v.geo.height)));
    r.move_to(x + 20.0, y);
    let (_, h1) = r.st.surface_size();
    assert!(h1 > h0, "grows immediately to the expanded size");
    for _ in 0..30 {
        r.run(16);
        let v = &r.st.views()[0];
        let (_, h) = r.st.surface_size();
        assert!(h >= v.y + v.geo.height + SHADOW_PAD - 1.0);
    }
}

#[test]
fn keyboard_focus_loss_unfocuses_idle_fields() {
    let mut r = Rig::new();
    r.show(text_banner(5, true));
    r.settle();
    let (x, y) = r.centre_of(5, |v| Some(Rect::new(0.0, 0.0, 10.0, v.geo.height)));
    r.move_to(x + 20.0, y);
    r.st.pointer_leave(r.now);
    r.st.keyboard_focus_lost(r.now);
    assert!(!r.st.wants_keyboard());
}

#[test]
fn long_reply_scrolls_the_field() {
    let mut r = Rig::new();
    r.show(text_banner(5, true));
    r.settle();
    let (x, y) = r.centre_of(5, |v| Some(Rect::new(0.0, 0.0, 10.0, v.geo.height)));
    r.move_to(x + 20.0, y);
    r.settle();
    r.st.key(Key::Text("w".repeat(200)), r.now);
    let scroll = r.st.views()[0].field.scroll;
    if crate::text::family_in_use().is_some() {
        assert!(scroll > 0.0);
    }
    r.st.key(Key::Home, r.now);
    assert_eq!(r.st.views()[0].field.scroll, 0.0);
}

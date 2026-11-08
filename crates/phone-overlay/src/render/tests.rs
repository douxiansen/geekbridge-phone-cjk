// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::{Duration, Instant};

use super::*;
use crate::model::{CallBanner, Command, OverlayConfig, TextBanner};

fn state_with(banners: Vec<Banner>) -> (OverlayState, Instant) {
    let t0 = Instant::now();
    let mut st = OverlayState::new(&OverlayConfig::default(), t0);
    for b in banners {
        st.apply(Command::Show(b), t0);
    }
    let mut now = t0;
    for _ in 0..40 {
        now += Duration::from_millis(16);
        st.tick(now);
    }
    (st, now)
}

fn incoming() -> Banner {
    Banner::Call(CallBanner {
        id: 1,
        name: "Ada Lovelace".into(),
        number: "+1 555 0100".into(),
        state: CallState::Incoming,
    })
}

fn texty() -> Banner {
    Banner::Text(crate::model::TextBanner {
        id: 2,
        name: "Grace Hopper".into(),
        number: "+1 555 0199".into(),
        body: "Lunch at 12:30? \u{1F600} caf\u{e9} \u{201c}Le Petit\u{201d}".into(),
        can_reply: true,
    })
}

fn opaque_pixels(pm: &Pixmap) -> usize {
    pm.data()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] > 8)
        .count()
}

#[test]
fn empty_state_renders_a_transparent_pixel() {
    let st = OverlayState::new(&OverlayConfig::default(), Instant::now());
    let pm = render(&st, 1.0, Theme::Dark);
    assert_eq!((pm.width(), pm.height()), (1, 1));
    assert_eq!(opaque_pixels(&pm), 0);
}

#[test]
fn rendering_is_deterministic_and_non_empty() {
    let (st, _) = state_with(vec![incoming(), texty()]);
    for theme in [Theme::Dark, Theme::Light] {
        for scale in [1.0, 2.0] {
            let a = render(&st, scale, theme);
            let b = render(&st, scale, theme);
            assert_eq!(a.data(), b.data(), "{theme:?} @{scale}");
            assert!(opaque_pixels(&a) > 20_000, "{theme:?} @{scale}");
        }
    }
}

#[test]
fn size_follows_surface_and_scale() {
    let (st, _) = state_with(vec![incoming()]);
    let (w, h) = st.surface_size();
    let one = render(&st, 1.0, Theme::Dark);
    let two = render(&st, 2.0, Theme::Dark);
    assert_eq!(one.width(), w.ceil() as u32);
    assert_eq!(one.height(), h.ceil() as u32);
    assert_eq!(two.width(), (w * 2.0).ceil() as u32);
    // Silly scales are clamped, not fatal.
    assert!(render(&st, f32::NAN, Theme::Dark).width() > 1);
    assert!(render(&st, 1e9, Theme::Dark).width() > 1);
    assert!(render(&st, -3.0, Theme::Dark).width() > 1);
}

#[test]
fn light_and_dark_differ_and_panel_is_translucent() {
    let (st, _) = state_with(vec![incoming()]);
    let dark = render(&st, 1.0, Theme::Dark);
    let light = render(&st, 1.0, Theme::Light);
    assert_ne!(dark.data(), light.data());
    // A panel pixel far from any content is neither transparent nor opaque.
    let v = &st.views()[0];
    let (px, py) = ((v.x + 180.0) as u32, (v.y + 4.0 + 40.0) as u32);
    let i = ((py * dark.width() + px) * 4 + 3) as usize;
    let a = dark.data()[i];
    assert!(a > 150 && a < 255, "panel alpha {a}");
}

#[test]
fn shadow_stays_outside_the_panel_and_fades() {
    let (st, _) = state_with(vec![incoming()]);
    let pm = render(&st, 1.0, Theme::Dark);
    let v = &st.views()[0];
    let alpha_at =
        |x: f32, y: f32| pm.data()[((y as u32 * pm.width() + x as u32) * 4 + 3) as usize];
    let below = alpha_at(v.x + 180.0, v.y + v.geo.height + 3.0);
    let far = alpha_at(v.x + 180.0, v.y + v.geo.height + 20.0);
    assert!(below > 10, "shadow right under the banner: {below}");
    assert!(far < below);
}

#[test]
fn expanded_text_banner_draws_more_than_collapsed() {
    let (mut st, mut now) = state_with(vec![texty()]);
    let collapsed = render(&st, 1.0, Theme::Dark);
    let v = &st.views()[0];
    let (x, y) = (v.x + 30.0, v.y + 30.0);
    st.pointer_motion(x, y, now);
    for _ in 0..20 {
        now += Duration::from_millis(16);
        st.tick(now);
    }
    let open = render(&st, 1.0, Theme::Dark);
    assert!(open.height() > collapsed.height());
    assert!(opaque_pixels(&open) > opaque_pixels(&collapsed));
}

#[test]
fn ripple_changes_between_frames_but_loops() {
    let t0 = Instant::now();
    let mut st = OverlayState::new(&OverlayConfig::default(), t0);
    st.apply(Command::Show(incoming()), t0);
    let mut now = t0;
    for _ in 0..40 {
        now += Duration::from_millis(16);
        st.tick(now);
    }
    let a = render(&st, 1.0, Theme::Dark);
    st.tick(now + Duration::from_millis(300));
    let b = render(&st, 1.0, Theme::Dark);
    assert_ne!(a.data(), b.data());
}

#[test]
fn active_call_timer_advances_each_second() {
    let t0 = Instant::now();
    let mut st = OverlayState::new(&OverlayConfig::default(), t0);
    let active = Banner::Call(CallBanner {
        id: 7,
        name: "Ada".into(),
        number: "+1 555 0100".into(),
        state: CallState::Active { since: t0 },
    });
    st.apply(Command::Show(active), t0);
    let mut now = t0;
    for _ in 0..40 {
        now += Duration::from_millis(16);
        st.tick(now);
    }
    let a = render(&st, 1.0, Theme::Dark);
    let same = render(&st, 1.0, Theme::Dark);
    assert_eq!(a.data(), same.data());
    st.tick(now + Duration::from_secs(2));
    let b = render(&st, 1.0, Theme::Dark);
    assert_ne!(a.data(), b.data(), "timer digits change");
}

#[test]
fn avatar_colours_are_stable_and_initials_unicode_safe() {
    assert_eq!(avatar_color("Ada"), avatar_color("Ada"));
    assert_ne!(avatar_color("Ada"), avatar_color("Bob"));
    assert_eq!(initials("ada lovelace").as_deref(), Some("AL"));
    assert_eq!(initials("\u{c9}mile").as_deref(), Some("\u{c9}"));
    assert_eq!(initials("+49 170 123").as_deref(), None);
    assert_eq!(initials("").as_deref(), None);
    assert_eq!(initials("Zo\u{eb} A. B").as_deref(), Some("ZA"));
}

#[test]
fn timer_formats() {
    assert_eq!(timer_text(0), "00:00");
    assert_eq!(timer_text(75), "01:15");
    assert_eq!(timer_text(3_725), "1:02:05");
}

#[test]
fn hostile_text_does_not_panic() {
    let weird = TextBanner {
        id: 9,
        name: "\u{202e}RTL \u{0}\u{1f}name\u{200d}".into(),
        number: String::new(),
        body: "a\nb\r\n\t".repeat(500) + &"\u{1F468}\u{200D}\u{1F469}".repeat(200),
        can_reply: true,
    };
    let (st, _) = state_with(vec![Banner::Text(weird)]);
    let pm = render(&st, 1.5, Theme::Light);
    assert!(opaque_pixels(&pm) > 1000);
}

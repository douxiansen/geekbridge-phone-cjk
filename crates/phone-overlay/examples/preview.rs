// SPDX-License-Identifier: GPL-3.0-or-later

//! Render every banner variant to PNGs, no display needed.
//!
//! ```text
//! cargo run --example preview [out_dir]
//! ```
//!
//! The default output directory is `/var/home/l1/overlay-shots/`. Each image
//! is the overlay surface composited over a stand-in wallpaper so the panel
//! translucency and the shadow can be judged.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use phone_overlay::{
    render, Banner, ButtonState, CallBanner, CallState, Command, Key, OverlayConfig, OverlayState,
    ReplyStatus, TextBanner, Theme,
};
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, SpreadMode,
    Transform,
};

struct Scene {
    st: OverlayState,
    now: Instant,
}

impl Scene {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            st: OverlayState::new(&OverlayConfig::default(), now),
            now,
        }
    }

    fn show(&mut self, b: Banner) {
        self.st.apply(Command::Show(b), self.now);
    }

    fn run(&mut self, ms: u64) {
        let end = self.now + Duration::from_millis(ms);
        while self.now < end {
            self.now += Duration::from_millis(16);
            self.st.tick(self.now);
        }
    }

    /// Hover over the first banner's body so its reply field opens.
    fn hover_first(&mut self) {
        let (x, y) = {
            let v = &self.st.views()[0];
            (v.x + 150.0, v.y + 30.0)
        };
        self.st.pointer_motion(x, y, self.now);
        self.run(400);
    }

    fn type_text(&mut self, s: &str) {
        self.st.key(Key::Text(s.to_owned()), self.now);
    }
}

fn call(id: u64, name: &str, number: &str, state: CallState) -> Banner {
    Banner::Call(CallBanner {
        id,
        name: name.into(),
        number: number.into(),
        state,
    })
}

fn text(id: u64, name: &str, body: &str, can_reply: bool) -> Banner {
    Banner::Text(TextBanner {
        id,
        name: name.into(),
        number: "+1 555 0199".into(),
        body: body.into(),
        can_reply,
    })
}

const LUNCH: &str = "Are we still on for lunch? I found a great place near the harbour, \
                     the one with the terrace.";

fn scenes() -> Vec<(&'static str, Scene)> {
    let mut out = Vec::new();

    let mut s = Scene::new();
    s.show(call(1, "Ada Lovelace", "+1 555 0100", CallState::Incoming));
    s.run(700);
    out.push(("call-incoming", s));

    let mut s = Scene::new();
    s.show(call(1, "Ada Lovelace", "+1 555 0100", CallState::Incoming));
    s.run(700 + 400);
    out.push(("call-incoming-b", s));

    let mut s = Scene::new();
    let since = s.now;
    s.show(call(
        1,
        "Ada Lovelace",
        "+1 555 0100",
        CallState::Active { since },
    ));
    s.run(1000);
    s.now += Duration::from_secs(83);
    s.st.tick(s.now);
    out.push(("call-active", s));

    let mut s = Scene::new();
    s.show(call(1, "", "+49 170 5550123", CallState::Dialing));
    s.run(900);
    out.push(("call-dialing-unknown", s));

    let mut s = Scene::new();
    s.show(text(2, "Grace Hopper", LUNCH, true));
    s.run(500);
    out.push(("text", s));

    let mut s = Scene::new();
    s.show(text(2, "Grace Hopper", LUNCH, true));
    s.run(500);
    s.hover_first();
    out.push(("text-reply-expanded", s));

    let mut s = Scene::new();
    s.show(text(2, "Grace Hopper", LUNCH, true));
    s.run(500);
    s.hover_first();
    s.type_text("Yes! See you at 12:30 \u{1F600} caf\u{e9}");
    out.push(("text-reply-typing", s));

    let mut s = Scene::new();
    s.show(text(2, "Grace Hopper", LUNCH, true));
    s.run(500);
    s.hover_first();
    s.type_text("On my way");
    s.st.key(Key::Enter, s.now);
    s.st.apply(Command::ReplyStatus(2, ReplyStatus::Sending), s.now);
    s.run(300);
    out.push(("text-sending", s));

    let mut s = Scene::new();
    s.show(text(2, "Grace Hopper", LUNCH, true));
    s.run(500);
    s.hover_first();
    s.type_text("On my way");
    s.st.key(Key::Enter, s.now);
    s.st.apply(Command::ReplyStatus(2, ReplyStatus::Sent), s.now);
    s.run(300);
    out.push(("text-sent", s));

    let mut s = Scene::new();
    s.show(text(2, "Grace Hopper", LUNCH, true));
    s.run(500);
    s.hover_first();
    s.type_text("On my way");
    s.st.key(Key::Enter, s.now);
    s.st.apply(
        Command::ReplyStatus(2, ReplyStatus::Failed("Phone not connected".into())),
        s.now,
    );
    s.run(300);
    out.push(("text-failed", s));

    let mut s = Scene::new();
    s.show(text(3, "+44 20 7946 0958", "Your code is 482913", false));
    s.run(500);
    out.push(("text-short-noreply", s));

    let mut s = Scene::new();
    s.show(text(2, "Grace Hopper", LUNCH, true));
    s.run(300);
    s.show(text(
        3,
        "Zo\u{eb} \u{c1}lvarez",
        "\u{1F389}\u{1F389} Happy birthday!!",
        true,
    ));
    s.run(300);
    s.show(call(1, "Ada Lovelace", "+1 555 0100", CallState::Incoming));
    s.run(900);
    out.push(("stack", s));

    let mut s = Scene::new();
    s.show(call(1, "Ada Lovelace", "+1 555 0100", CallState::Incoming));
    s.run(400);
    let (x, y) = {
        let v = &s.st.views()[0];
        let (_, r) = v.geo.buttons[1];
        (v.x + r.x + r.w / 2.0, v.y + r.y + r.h / 2.0)
    };
    s.st.pointer_motion(x, y, s.now);
    s.run(300);
    out.push(("call-hover-answer", s));
    let _ = ButtonState::Down;

    out
}

fn wallpaper(w: u32, h: u32, theme: Theme) -> Option<Pixmap> {
    let mut pm = Pixmap::new(w, h)?;
    let (a, b) = match theme {
        Theme::Dark => (
            Color::from_rgba8(0x1B, 0x2A, 0x4A, 255),
            Color::from_rgba8(0x6B, 0x2D, 0x5C, 255),
        ),
        Theme::Light => (
            Color::from_rgba8(0xF2, 0xD8, 0xB0, 255),
            Color::from_rgba8(0xB9, 0xD7, 0xF2, 255),
        ),
    };
    let shader = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(w as f32, h as f32),
        vec![GradientStop::new(0.0, a), GradientStop::new(1.0, b)],
        SpreadMode::Pad,
        Transform::identity(),
    )?;
    let p = Paint {
        shader,
        ..Paint::default()
    };
    let mut pb = PathBuilder::new();
    pb.push_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, w as f32, h as f32)?);
    pm.fill_path(
        &pb.finish()?,
        &p,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    Some(pm)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args().nth(1).map_or_else(
        || PathBuf::from("/var/home/l1/overlay-shots"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&dir)?;
    for (name, scene) in scenes() {
        for (theme, tn) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
            for scale in [1.0_f32, 2.0] {
                let surface = render(&scene.st, scale, theme);
                let Some(mut canvas) = wallpaper(surface.width(), surface.height(), theme) else {
                    continue;
                };
                canvas.draw_pixmap(
                    0,
                    0,
                    surface.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    Transform::identity(),
                    None,
                );
                let path = dir.join(format!("{name}-{tn}-{}x.png", scale as u32));
                canvas.save_png(&path)?;
            }
        }
    }
    println!("wrote previews to {}", dir.display());
    Ok(())
}

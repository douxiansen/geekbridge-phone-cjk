// SPDX-License-Identifier: GPL-3.0-or-later

//! Geometry of one banner and of the stack, in logical pixels. Pure
//! functions of the banner kind and animation values; nothing here measures
//! text, so it is trivially testable.

/// Banner width.
pub const BANNER_W: f32 = 360.0;
/// Corner radius.
pub const RADIUS: f32 = 20.0;
/// Inner padding.
pub const PAD: f32 = 14.0;
/// Vertical gap between stacked banners.
pub const GAP: f32 = 8.0;
/// Banners shown at once; older ones wait their turn.
pub const MAX_VISIBLE: usize = 3;
/// Distance from the screen's top and right edge to the stack.
pub const EDGE: f32 = 12.0;
/// Transparent room left of and below the stack for the shadow.
pub const SHADOW_PAD: f32 = 24.0;
/// Height of the app chip row.
pub const CHIP_H: f32 = 20.0;
/// Space between the chip row and the content.
pub const CHIP_GAP: f32 = 10.0;
/// Avatar diameter on call banners.
pub const CALL_AVATAR: f32 = 48.0;
/// Avatar diameter on text banners.
pub const TEXT_AVATAR: f32 = 40.0;
/// Pill button height.
pub const BUTTON_H: f32 = 34.0;
/// Reply field height.
pub const FIELD_H: f32 = 36.0;
/// Send button diameter.
pub const SEND_D: f32 = 28.0;
/// Space above the reply field.
pub const FIELD_GAP: f32 = 10.0;
/// Height of the reply status line (plus its gap above).
pub const STATUS_H: f32 = 20.0;
/// Preview line count.
pub const PREVIEW_LINES: usize = 2;

/// An axis-aligned rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// Construct from origin and size.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Right edge.
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// Bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Centre point.
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// Whether the point is inside (right and bottom edges exclusive).
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// Move by (`dx`, `dy`).
    pub fn translate(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    /// Shrink (or grow, when negative) by `d` on every side.
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(
            self.x + d,
            self.y + d,
            (self.w - 2.0 * d).max(0.0),
            (self.h - 2.0 * d).max(0.0),
        )
    }

    /// Scale about the centre.
    pub fn scale_about_center(&self, k: f32) -> Rect {
        let (cx, cy) = self.center();
        Rect::new(
            cx - self.w * k / 2.0,
            cy - self.h * k / 2.0,
            self.w * k,
            self.h * k,
        )
    }
}

/// What a point on a banner hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// Plain banner body: clicking opens the app.
    Body,
    /// The dismiss cross on a text banner.
    Close,
    /// Answer button.
    Answer,
    /// Decline button.
    Decline,
    /// Hang up button.
    HangUp,
    /// The reply text field.
    Field,
    /// The reply send button.
    Send,
}

/// Which family of banner is being laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Ringing call: Decline and Answer.
    Ringing,
    /// Dialing, active or held call: Hang up.
    InCall,
    /// Text message; `can_reply` enables the expanding reply field.
    Text {
        /// Whether a reply field exists.
        can_reply: bool,
    },
}

/// Inputs that determine a banner's geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoInput {
    /// Banner family.
    pub kind: Kind,
    /// Preview lines (text banners), 0 to [`PREVIEW_LINES`].
    pub body_lines: usize,
    /// Reply field expansion, `0.0..=1.0`.
    pub expand: f32,
    /// Status line reveal, `0.0..=1.0`.
    pub status: f32,
}

/// Banner-local rectangles (origin at the banner's top-left).
#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    /// Total height.
    pub height: f32,
    /// App icon tile.
    pub icon: Rect,
    /// The chip row.
    pub chip_row: Rect,
    /// Dismiss cross (text banners), sitting where the time label is.
    pub close: Option<Rect>,
    /// Avatar.
    pub avatar: Rect,
    /// Column right of the avatar holding name and body/number.
    pub text_col: Rect,
    /// Free area at the right of a call's main row (ripple, timer).
    pub call_status: Rect,
    /// Buttons with their hit ids.
    pub buttons: Vec<(Hit, Rect)>,
    /// Reply field.
    pub field: Option<Rect>,
    /// Send button inside the field.
    pub send: Option<Rect>,
    /// Status line under the field.
    pub status_line: Option<Rect>,
}

/// Compute the geometry for `input`.
pub fn geometry(input: &GeoInput) -> Geometry {
    let chip_row = Rect::new(PAD, PAD, BANNER_W - 2.0 * PAD, CHIP_H);
    let icon = Rect::new(PAD, PAD, CHIP_H, CHIP_H);
    let y0 = PAD + CHIP_H + CHIP_GAP;
    let inner_w = BANNER_W - 2.0 * PAD;
    let mut g = Geometry {
        height: 0.0,
        icon,
        chip_row,
        close: None,
        avatar: Rect::default(),
        text_col: Rect::default(),
        call_status: Rect::default(),
        buttons: Vec::new(),
        field: None,
        send: None,
        status_line: None,
    };
    match input.kind {
        Kind::Ringing | Kind::InCall => {
            g.avatar = Rect::new(PAD, y0, CALL_AVATAR, CALL_AVATAR);
            let tx = PAD + CALL_AVATAR + 12.0;
            // The right 90 px of the row belong to the ripple / timer.
            let status_w = 90.0;
            g.call_status = Rect::new(BANNER_W - PAD - status_w, y0, status_w, CALL_AVATAR);
            g.text_col = Rect::new(tx, y0, g.call_status.x - 8.0 - tx, CALL_AVATAR);
            let by = y0 + CALL_AVATAR + 12.0;
            if input.kind == Kind::Ringing {
                let bw = (inner_w - 8.0) / 2.0;
                g.buttons
                    .push((Hit::Decline, Rect::new(PAD, by, bw, BUTTON_H)));
                g.buttons
                    .push((Hit::Answer, Rect::new(PAD + bw + 8.0, by, bw, BUTTON_H)));
            } else {
                g.buttons
                    .push((Hit::HangUp, Rect::new(PAD, by, inner_w, BUTTON_H)));
            }
            g.height = by + BUTTON_H + PAD;
        }
        Kind::Text { can_reply } => {
            g.close = Some(Rect::new(BANNER_W - PAD - 18.0, PAD + 1.0, 18.0, 18.0));
            g.avatar = Rect::new(PAD, y0, TEXT_AVATAR, TEXT_AVATAR);
            let tx = PAD + TEXT_AVATAR + 12.0;
            let name_h = 20.0;
            let body_h = if input.body_lines > 0 {
                1.0 + 17.0 * input.body_lines.min(PREVIEW_LINES) as f32
            } else {
                0.0
            };
            let content_h = name_h + body_h;
            let row_h = content_h.max(TEXT_AVATAR);
            g.text_col = Rect::new(tx, y0, BANNER_W - PAD - tx, content_h);
            let collapsed = y0 + row_h + PAD;
            let expand = input.expand.clamp(0.0, 1.0);
            let status = input.status.clamp(0.0, 1.0);
            if can_reply {
                let fy = collapsed - PAD + FIELD_GAP;
                let field = Rect::new(PAD, fy, inner_w, FIELD_H);
                g.send = Some(Rect::new(
                    field.right() - 4.0 - SEND_D,
                    fy + (FIELD_H - SEND_D) / 2.0,
                    SEND_D,
                    SEND_D,
                ));
                g.status_line = Some(Rect::new(
                    PAD + 6.0,
                    field.bottom() + 4.0,
                    inner_w - 12.0,
                    14.0,
                ));
                g.field = Some(field);
                g.height = collapsed + expand * (FIELD_GAP + FIELD_H) + status * STATUS_H;
            } else {
                g.height = collapsed;
            }
        }
    }
    g
}

/// The banner-local hit under (`x`, `y`). Interactive parts only count when
/// they are visible enough (`expand`, `status` come from the input).
pub fn hit_test(
    g: &Geometry,
    input: &GeoInput,
    x: f32,
    y: f32,
    close_visible: bool,
) -> Option<Hit> {
    if !Rect::new(0.0, 0.0, BANNER_W, g.height).contains(x, y) {
        return None;
    }
    for (hit, r) in &g.buttons {
        if r.contains(x, y) {
            return Some(*hit);
        }
    }
    if close_visible {
        if let Some(c) = g.close {
            if c.inset(-4.0).contains(x, y) {
                return Some(Hit::Close);
            }
        }
    }
    if input.expand > 0.5 {
        if let Some(s) = g.send {
            if s.inset(-2.0).contains(x, y) {
                return Some(Hit::Send);
            }
        }
        if let Some(f) = g.field {
            if f.contains(x, y) {
                return Some(Hit::Field);
            }
        }
    }
    Some(Hit::Body)
}

/// Top edges of banners stacked from the top, newest first, in surface
/// coordinates. `heights` is in stack order (index 0 is the top banner).
pub fn stack_tops(heights: &[f32]) -> Vec<f32> {
    let mut y = EDGE;
    heights
        .iter()
        .map(|h| {
            let top = y;
            y += h + GAP;
            top
        })
        .collect()
}

/// Left edge of banners in surface coordinates.
pub const BANNER_X: f32 = SHADOW_PAD;
/// Surface width in logical pixels.
pub const SURFACE_W: f32 = SHADOW_PAD + BANNER_W + EDGE;

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: usize, expand: f32, status: f32, can_reply: bool) -> GeoInput {
        GeoInput {
            kind: Kind::Text { can_reply },
            body_lines: lines,
            expand,
            status,
        }
    }

    #[test]
    fn call_banner_has_expected_buttons() {
        let g = geometry(&GeoInput {
            kind: Kind::Ringing,
            body_lines: 0,
            expand: 0.0,
            status: 0.0,
        });
        assert_eq!(g.buttons.len(), 2);
        assert_eq!(g.buttons[0].0, Hit::Decline);
        assert_eq!(g.buttons[1].0, Hit::Answer);
        let (d, a) = (g.buttons[0].1, g.buttons[1].1);
        assert!(d.right() < a.x, "buttons must not overlap");
        assert!((a.h - BUTTON_H).abs() < f32::EPSILON);
        assert!((a.bottom() + PAD - g.height).abs() < 0.01);

        let g2 = geometry(&GeoInput {
            kind: Kind::InCall,
            body_lines: 0,
            expand: 0.0,
            status: 0.0,
        });
        assert_eq!(g2.buttons.len(), 1);
        assert_eq!(g2.buttons[0].0, Hit::HangUp);
        assert_eq!(g2.height, g.height);
    }

    #[test]
    fn everything_stays_inside_the_banner() {
        for input in [
            text(2, 1.0, 1.0, true),
            text(1, 0.0, 0.0, false),
            text(0, 1.0, 0.0, true),
        ] {
            let g = geometry(&input);
            let bounds = Rect::new(0.0, 0.0, BANNER_W, g.height);
            for r in [g.field, g.send, g.status_line, g.close]
                .into_iter()
                .flatten()
            {
                assert!(r.x >= 0.0 && r.right() <= bounds.right() + 0.01, "{r:?}");
            }
            if input.expand >= 1.0 {
                let f = g.field.expect("field");
                let extra = if input.status >= 1.0 { STATUS_H } else { 0.0 };
                assert!(f.bottom() + PAD + extra <= g.height + 0.01);
            }
        }
    }

    #[test]
    fn text_banner_grows_with_expand_and_lines() {
        let one = geometry(&text(1, 0.0, 0.0, true)).height;
        let two = geometry(&text(2, 0.0, 0.0, true)).height;
        assert!(two > one);
        let open = geometry(&text(2, 1.0, 0.0, true)).height;
        assert!((open - two - FIELD_GAP - FIELD_H).abs() < 0.01);
        let mid = geometry(&text(2, 0.5, 0.0, true)).height;
        assert!(mid > two && mid < open);
        let no_reply = geometry(&text(2, 1.0, 1.0, false)).height;
        assert_eq!(no_reply, two);
    }

    #[test]
    fn hit_testing() {
        let input = text(2, 1.0, 0.0, true);
        let g = geometry(&input);
        let f = g.field.expect("field");
        let (fx, fy) = f.center();
        assert_eq!(hit_test(&g, &input, fx, fy, false), Some(Hit::Field));
        let s = g.send.expect("send");
        let (sx, sy) = s.center();
        assert_eq!(hit_test(&g, &input, sx, sy, false), Some(Hit::Send));
        assert_eq!(hit_test(&g, &input, 5.0, 5.0, true), Some(Hit::Body));
        let c = g.close.expect("close").center();
        assert_eq!(hit_test(&g, &input, c.0, c.1, true), Some(Hit::Close));
        assert_eq!(hit_test(&g, &input, c.0, c.1, false), Some(Hit::Body));
        assert_eq!(hit_test(&g, &input, -1.0, 5.0, true), None);
        assert_eq!(hit_test(&g, &input, 5.0, g.height + 1.0, true), None);

        // Collapsed: the field is not clickable yet.
        let closed = text(2, 0.0, 0.0, true);
        let gc = geometry(&closed);
        assert_eq!(hit_test(&gc, &closed, fx, fy, false), None);
        assert_eq!(
            hit_test(&gc, &closed, fx, gc.height - 1.0, false),
            Some(Hit::Body)
        );
    }

    #[test]
    fn call_buttons_hit() {
        let input = GeoInput {
            kind: Kind::Ringing,
            body_lines: 0,
            expand: 0.0,
            status: 0.0,
        };
        let g = geometry(&input);
        for (hit, r) in g.buttons.clone() {
            let (x, y) = r.center();
            assert_eq!(hit_test(&g, &input, x, y, false), Some(hit));
        }
    }

    #[test]
    fn stack_is_top_down_with_gaps() {
        let tops = stack_tops(&[100.0, 50.0, 70.0]);
        assert_eq!(
            tops,
            vec![EDGE, EDGE + 100.0 + GAP, EDGE + 150.0 + 2.0 * GAP]
        );
        assert!(stack_tops(&[]).is_empty());
    }

    #[test]
    fn rect_helpers() {
        let r = Rect::new(10.0, 10.0, 20.0, 10.0);
        assert!(r.contains(10.0, 10.0));
        assert!(!r.contains(30.0, 15.0));
        assert_eq!(r.center(), (20.0, 15.0));
        let s = r.scale_about_center(0.5);
        assert_eq!(s.center(), r.center());
        assert_eq!(s.w, 10.0);
    }
}

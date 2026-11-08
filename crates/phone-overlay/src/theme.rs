// SPDX-License-Identifier: GPL-3.0-or-later

//! Colour tokens. The ink, line and red values are GeekBridge's own
//! (`crates/bridge-apps/src/theme.rs`); the panel is the macOS notification
//! banner treatment of them.

use crate::model::ThemePref;

/// An 8-bit sRGB colour with straight (non-premultiplied) alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    /// Opaque colour from a `0xRRGGBB` value.
    pub const fn hex(v: u32) -> Self {
        Rgba((v >> 16) as u8, (v >> 8) as u8, v as u8, 255)
    }

    /// The same colour with alpha `a` in `0.0..=1.0`.
    pub fn with_alpha(self, a: f32) -> Self {
        Rgba(
            self.0,
            self.1,
            self.2,
            (a.clamp(0.0, 1.0) * 255.0).round() as u8,
        )
    }

    /// The same colour with its alpha scaled by `k`.
    pub fn scaled_alpha(self, k: f32) -> Self {
        Rgba(
            self.0,
            self.1,
            self.2,
            (f32::from(self.3) * k.clamp(0.0, 1.0)).round() as u8,
        )
    }

    /// Move each channel towards white (`k > 0`) or black (`k < 0`).
    pub fn shift(self, k: f32) -> Self {
        let f = |c: u8| -> u8 {
            let c = f32::from(c);
            let v = if k >= 0.0 {
                c + (255.0 - c) * k
            } else {
                c * (1.0 + k)
            };
            v.round().clamp(0.0, 255.0) as u8
        };
        Rgba(f(self.0), f(self.1), f(self.2), self.3)
    }

    /// Linear interpolation between two colours.
    pub fn mix(self, other: Rgba, t: f32) -> Self {
        let l = |a: u8, b: u8| -> u8 {
            (f32::from(a) + (f32::from(b) - f32::from(a)) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Rgba(
            l(self.0, other.0),
            l(self.1, other.1),
            l(self.2, other.2),
            l(self.3, other.3),
        )
    }

    /// Convert to a tiny-skia colour.
    pub fn to_skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.0, self.1, self.2, self.3)
    }

    /// WCAG relative luminance (alpha ignored).
    pub fn luminance(self) -> f32 {
        let lin = |c: u8| {
            let c = f32::from(c) / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.0) + 0.7152 * lin(self.1) + 0.0722 * lin(self.2)
    }

    /// WCAG contrast ratio between two colours (alpha ignored).
    pub fn contrast(self, other: Rgba) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Composite `self` over an opaque backdrop.
    pub fn over(self, backdrop: Rgba) -> Rgba {
        let a = f32::from(self.3) / 255.0;
        let l = |f: u8, b: u8| (f32::from(f) * a + f32::from(b) * (1.0 - a)).round() as u8;
        Rgba(
            l(self.0, backdrop.0),
            l(self.1, backdrop.1),
            l(self.2, backdrop.2),
            255,
        )
    }
}

/// Resolved light or dark look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Theme {
    /// Near-black translucent panel.
    #[default]
    Dark,
    /// Frosted off-white panel.
    Light,
}

/// Every colour the renderer uses.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Banner background (translucent).
    pub panel: Rgba,
    /// 1 px inner highlight border.
    pub border: Rgba,
    /// Drop shadow colour (alpha is the peak strength).
    pub shadow: Rgba,
    /// Primary text.
    pub text: Rgba,
    /// Secondary text.
    pub text2: Rgba,
    /// Faint dots and separators.
    pub dot_off: Rgba,
    /// Flat control fill (reply field, close button).
    pub control: Rgba,
    /// Hovered control fill.
    pub control_hover: Rgba,
    /// GeekBridge red, fills and dots. White text on it is 5.2:1.
    pub red: Rgba,
    /// Text-safe red for small type on the panel.
    pub red_text: Rgba,
    /// Answer button fill.
    pub green: Rgba,
    /// Label colour on the green fill.
    pub green_ink: Rgba,
    /// Send button fill.
    pub blue: Rgba,
    /// Text on red and blue fills.
    pub on_accent: Rgba,
    /// Success text.
    pub ok_text: Rgba,
    /// Focus ring around the reply field.
    pub focus: Rgba,
}

impl Theme {
    /// Resolve a preference. `system_dark` is what the desktop reported, if it did.
    pub fn resolve(pref: ThemePref, system_dark: Option<bool>) -> Theme {
        match pref {
            ThemePref::Dark => Theme::Dark,
            ThemePref::Light => Theme::Light,
            ThemePref::System => {
                if system_dark.unwrap_or(true) {
                    Theme::Dark
                } else {
                    Theme::Light
                }
            }
        }
    }

    /// The colour set for this theme.
    pub fn palette(self) -> Palette {
        match self {
            Theme::Dark => Palette {
                panel: Rgba::hex(0x1C1C1E).with_alpha(0.95),
                border: Rgba::hex(0xFFFFFF).with_alpha(0.10),
                shadow: Rgba::hex(0x000000).with_alpha(0.42),
                text: Rgba::hex(0xF4F4F1),
                text2: Rgba::hex(0xA3A39D),
                dot_off: Rgba::hex(0x3A3A38),
                control: Rgba::hex(0xFFFFFF).with_alpha(0.10),
                control_hover: Rgba::hex(0xFFFFFF).with_alpha(0.16),
                red: Rgba::hex(0xD71921),
                red_text: Rgba::hex(0xFF7878),
                green: Rgba::hex(0x30D158),
                green_ink: Rgba::hex(0x04210C),
                blue: Rgba::hex(0x0A84FF),
                on_accent: Rgba::hex(0xFFFFFF),
                ok_text: Rgba::hex(0x5BE07E),
                focus: Rgba::hex(0x0A84FF).with_alpha(0.85),
            },
            Theme::Light => Palette {
                panel: Rgba::hex(0xF6F5F1).with_alpha(0.96),
                border: Rgba::hex(0x000000).with_alpha(0.08),
                shadow: Rgba::hex(0x000000).with_alpha(0.28),
                text: Rgba::hex(0x0C0C0C),
                text2: Rgba::hex(0x52524D),
                dot_off: Rgba::hex(0xD3D2CB),
                control: Rgba::hex(0x000000).with_alpha(0.06),
                control_hover: Rgba::hex(0x000000).with_alpha(0.10),
                red: Rgba::hex(0xD71921),
                red_text: Rgba::hex(0xB8121A),
                green: Rgba::hex(0x30D158),
                green_ink: Rgba::hex(0x04210C),
                blue: Rgba::hex(0x0A84FF),
                on_accent: Rgba::hex(0xFFFFFF),
                ok_text: Rgba::hex(0x1B7A36),
                focus: Rgba::hex(0x0A6EDB).with_alpha(0.9),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AA: f32 = 4.5;

    /// Text colours must hold AA over the panel on the worst backdrops
    /// (pure white and pure black desktops show through the translucency).
    #[test]
    fn text_contrast_is_aa_on_any_backdrop() {
        for theme in [Theme::Dark, Theme::Light] {
            let p = theme.palette();
            for backdrop in [Rgba::hex(0x000000), Rgba::hex(0xFFFFFF)] {
                let ground = p.panel.over(backdrop);
                for (name, ink) in [
                    ("text", p.text),
                    ("text2", p.text2),
                    ("red_text", p.red_text),
                    ("ok_text", p.ok_text),
                ] {
                    let c = ink.contrast(ground);
                    assert!(c >= AA, "{theme:?} {name} on {backdrop:?}: {c:.2}");
                }
            }
        }
    }

    #[test]
    fn button_labels_are_aa() {
        for theme in [Theme::Dark, Theme::Light] {
            let p = theme.palette();
            assert!(p.on_accent.contrast(p.red) >= AA);
            assert!(p.green_ink.contrast(p.green) >= AA);
        }
    }

    #[test]
    fn resolve_defaults_to_dark() {
        assert_eq!(Theme::resolve(ThemePref::System, None), Theme::Dark);
        assert_eq!(Theme::resolve(ThemePref::System, Some(false)), Theme::Light);
        assert_eq!(Theme::resolve(ThemePref::Light, Some(true)), Theme::Light);
    }

    #[test]
    fn alpha_helpers() {
        assert_eq!(Rgba::hex(0x102030).with_alpha(0.5).3, 128);
        assert_eq!(Rgba(1, 2, 3, 200).scaled_alpha(0.5).3, 100);
        assert_eq!(Rgba(0, 0, 0, 255).shift(1.0), Rgba(255, 255, 255, 255));
    }
}

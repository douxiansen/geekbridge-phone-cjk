// SPDX-License-Identifier: GPL-3.0-or-later
//! The window icon, drawn from code: a dot-matrix handset glyph in white on black with the red dot.

use eframe::egui::IconData;

const SIZE: u32 = 64;

/// A 5x5 dot picture of a phone's keypad, with one red dot.
pub fn app_icon() -> IconData {
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let put = |rgba: &mut Vec<u8>, x: u32, y: u32, c: [u8; 4]| {
        let i = ((y * SIZE + x) * 4) as usize;
        rgba[i..i + 4].copy_from_slice(&c);
    };
    let radius = 14.0_f32;
    for y in 0..SIZE {
        for x in 0..SIZE {
            // Rounded square.
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            let (dx, dy) = ((cx - 32.0).abs() - (32.0 - radius), (cy - 32.0).abs() - (32.0 - radius));
            let inside = dx.max(0.0).hypot(dy.max(0.0)) <= radius;
            if inside {
                put(&mut rgba, x, y, [12, 12, 12, 255]);
            }
        }
    }
    // Three by four keys.
    for row in 0..4u32 {
        for col in 0..3u32 {
            let (cx, cy) = (18.0 + col as f32 * 14.0, 12.0 + row as f32 * 13.0);
            let colour = if row == 3 && col == 1 { [215, 25, 33, 255] } else { [244, 244, 241, 255] };
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let (px, py) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                    if px * px + py * py <= 5.0 * 5.0 {
                        put(&mut rgba, x, y, colour);
                    }
                }
            }
        }
    }
    IconData { rgba, width: SIZE, height: SIZE }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_has_the_right_size_and_shows_the_red_dot() {
        let icon = app_icon();
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
        let reds = icon.rgba.chunks(4).filter(|p| p == &[215, 25, 33, 255]).count();
        assert!(reds > 20);
        // Corners are transparent: it is a rounded square.
        assert_eq!(icon.rgba[3], 0);
    }
}

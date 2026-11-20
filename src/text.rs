//! TrueType text (FantasqueSansM Nerd Font Mono, built in) with a glyph cache; falls back
//! to the built-in 8x8 font if the font can't be read.
//!
//! The font is a cut-down copy: Latin, Greek, Cyrillic, punctuation, arrows and the icons
//! CUT draws, about 60 KB a weight. Its licence is `assets/fonts/OFL.txt`. To add an icon,
//! make the files again with fontTools' `pyftsubset` from the full Nerd Font, adding the
//! icon's code point to the list (see `assets/fonts/README.md`).

use std::collections::HashMap;

use fontdue::{Font, FontSettings, Metrics};

use crate::ui::Canvas;

const REGULAR: &[u8] = include_bytes!("../assets/fonts/FantasqueSansMNerdFontMono-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../assets/fonts/FantasqueSansMNerdFontMono-Bold.ttf");

struct Face {
    font: Font,
    cache: HashMap<(char, u32), (Metrics, Vec<u8>)>,
}

#[derive(Default)]
pub struct Fonts {
    /// [regular, bold]
    faces: [Option<Face>; 2],
}

impl Fonts {
    pub fn load() -> Self {
        let face = |bytes: &[u8]| {
            Font::from_bytes(bytes, FontSettings { scale: 32.0, ..Default::default() }).ok().map(|font| Face { font, cache: HashMap::new() })
        };
        Fonts { faces: [face(REGULAR), face(BOLD)] }
    }

    fn face(&mut self, bold: bool) -> Option<&mut Face> {
        let [reg, b] = &mut self.faces;
        if bold && b.is_some() {
            b.as_mut()
        } else {
            reg.as_mut()
        }
    }

    /// Width of one character cell (the font is monospaced).
    pub fn advance(&mut self, px: f32, bold: bool) -> f32 {
        match self.face(bold) {
            Some(f) => f.font.metrics('M', px).advance_width,
            None => 8.0 * (px / 8.0).round().max(1.0),
        }
    }

    pub fn width(&mut self, s: &str, px: f32, bold: bool) -> i32 {
        (s.chars().count() as f32 * self.advance(px, bold)).ceil() as i32
    }

    fn ascent(&mut self, px: f32, bold: bool) -> f32 {
        self.face(bold).and_then(|f| f.font.horizontal_line_metrics(px)).map(|m| m.ascent).unwrap_or(px * 0.8)
    }

    /// Calls `put(x, y, alpha)` for every covered pixel of `s`, with `top` as the top of
    /// the line box. Returns the advance width.
    fn render(&mut self, x: f32, top: f32, s: &str, px: f32, bold: bool, mut put: impl FnMut(i32, i32, u8)) -> i32 {
        let adv = self.advance(px, bold);
        let base = top + self.ascent(px, bold);
        let Some(face) = self.face(bold) else { return 0 };
        let key_px = (px * 4.0).round() as u32;
        let mut pen = x;
        for ch in s.chars() {
            let font = &face.font;
            let (m, bm) = face.cache.entry((ch, key_px)).or_insert_with(|| font.rasterize(ch, px));
            let gx = (pen + m.xmin as f32).round() as i32;
            let gy = (base - m.height as f32 - m.ymin as f32).round() as i32;
            for row in 0..m.height {
                for col in 0..m.width {
                    let a = bm[row * m.width + col];
                    if a > 0 {
                        put(gx + col as i32, gy + row as i32, a);
                    }
                }
            }
            pen += adv;
        }
        (pen - x).ceil() as i32
    }

    /// Draws `s` with its line box starting at `top`. Returns the width drawn.
    pub fn draw(&mut self, c: &mut Canvas, x: i32, top: i32, s: &str, color: u32, px: f32, bold: bool) -> i32 {
        if self.faces[0].is_none() {
            let scale = (px / 10.0).round().max(1.0) as i32;
            c.text(x, top + ((px as i32 - 8 * scale) / 2).max(0), s, color, scale);
            return s.chars().count() as i32 * 8 * scale;
        }
        self.render(x as f32, top as f32, s, px, bold, |gx, gy, a| c.blend(gx, gy, color, a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both weights load, and every icon CUT draws is in the cut-down font (an icon
    /// missing from it would draw as nothing).
    #[test]
    fn built_in_font_has_the_icons() {
        let fonts = Fonts::load();
        let icons: Vec<char> = ["src/main.rs", "src/devmenu.rs"]
            .iter()
            .flat_map(|f| {
                let s = std::fs::read_to_string(f).unwrap();
                s.match_indices("\\u{").filter_map(|(i, _)| {
                    let hex = &s[i + 3..i + 3 + s[i + 3..].find('}')?];
                    char::from_u32(u32::from_str_radix(hex, 16).ok()?)
                })
                // Control characters (the save form's field separator) aren't drawn.
                .filter(|c| !c.is_control())
                .collect::<Vec<_>>()
            })
            .chain(['•', '←', '→', '↑', '↓', 'é', 'ß', 'Ж'])
            .collect();
        assert!(!icons.is_empty());
        for (i, face) in fonts.faces.iter().enumerate() {
            let face = face.as_ref().expect("font loads");
            for &c in &icons {
                assert!(face.font.lookup_glyph_index(c) != 0, "U+{:04X} missing from face {i}", c as u32);
            }
        }
    }
}

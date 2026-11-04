//! TrueType text (FantasqueSansM Nerd Font Mono) with a glyph cache; falls back to the
//! built-in 8x8 font if the font can't be found.

use std::collections::HashMap;

use fontdue::{Font, FontSettings, Metrics};

use crate::ui::Canvas;

const FAMILY: &str = "FantasqueSansM Nerd Font Mono";

struct Face {
    font: Font,
    cache: HashMap<(char, u32), (Metrics, Vec<u8>)>,
}

#[derive(Default)]
pub struct Fonts {
    /// [regular, bold]
    faces: [Option<Face>; 2],
}

fn find(style: &str, fallback: &str) -> Option<Vec<u8>> {
    let path = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", &format!("{FAMILY}:{style}")])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|p| p.contains("Fantasque"))
        .unwrap_or_else(|| fallback.to_string());
    std::fs::read(path).ok()
}

impl Fonts {
    pub fn load() -> Self {
        let face = |style, fallback| {
            find(style, fallback)
                .and_then(|b| Font::from_bytes(b, FontSettings { scale: 32.0, ..Default::default() }).ok())
                .map(|font| Face { font, cache: HashMap::new() })
        };
        Fonts {
            faces: [
                face("style=Regular", "/usr/share/fonts/TTF/FantasqueSansMNerdFontMono-Regular.ttf"),
                face("style=Bold", "/usr/share/fonts/TTF/FantasqueSansMNerdFontMono-Bold.ttf"),
            ],
        }
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

//! Tiny software renderer: rects, lines, the platter, and an 8x8 fallback font.

use font8x8::{UnicodeFonts, BASIC_FONTS};

pub const BG: u32 = 0x0e0e12;
pub const PANEL: u32 = 0x17171d;
pub const CHIP: u32 = 0x1d1d26;
pub const DIM: u32 = 0x6c6c84;
pub const TEXT: u32 = 0xdcdce8;
pub const LILAC: u32 = 0xb48cff;
pub const PURPLE: u32 = 0x7c23ff;
pub const GREEN: u32 = 0x0aff02;
pub const WHITE: u32 = 0xf4f4fa;
pub const RED: u32 = 0xff3b4a;
const LABEL: u32 = 0x08080b;
/// The record's face with the groove styling turned off.
const FLAT: u32 = 0x17171d;

pub fn mix(bg: u32, fg: u32, a: u32) -> u32 {
    let ch = |s: u32| {
        let (b, f) = ((bg >> s) & 0xff, (fg >> s) & 0xff);
        ((b * (255 - a) + f * a) / 255) << s
    };
    ch(16) | ch(8) | ch(0)
}

pub struct Canvas<'a> {
    pub px: &'a mut [u32],
    pub w: usize,
    pub h: usize,
}

impl Canvas<'_> {
    pub fn fill(&mut self, c: u32) {
        self.px.fill(c);
    }

    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, c: u32, a: u8) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let p = &mut self.px[y as usize * self.w + x as usize];
            *p = if a == 255 { c } else { mix(*p, c, a as u32) };
        }
    }

    /// A filled box with a 1px outline.
    pub fn boxed(&mut self, x: i32, y: i32, w: i32, h: i32, fill: u32, edge: u32) {
        self.rect(x, y, w, h, edge);
        self.rect(x + 1, y + 1, w - 2, h - 2, fill);
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        let x0 = x.max(0) as usize;
        let y0 = y.max(0) as usize;
        let x1 = ((x + w).max(0) as usize).min(self.w);
        let y1 = ((y + h).max(0) as usize).min(self.h);
        for yy in y0..y1 {
            let row = yy * self.w;
            self.px[row + x0.min(x1)..row + x1].fill(c);
        }
    }

    pub fn vline(&mut self, x: i32, y0: i32, y1: i32, c: u32) {
        let (a, b) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        self.rect(x, a, 1, b - a + 1, c);
    }

    pub fn line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, thick: i32, c: u32) {
        let len = ((x1 - x0).hypot(y1 - y0)).max(1.0);
        let n = len.ceil() as i32;
        for i in 0..=n {
            let t = i as f64 / n as f64;
            let x = x0 + (x1 - x0) * t;
            let y = y0 + (y1 - y0) * t;
            self.rect(x as i32 - thick / 2, y as i32 - thick / 2, thick, thick, c);
        }
    }

    pub fn text(&mut self, x: i32, y: i32, s: &str, c: u32, scale: i32) {
        let mut cx = x;
        for ch in s.chars() {
            if let Some(g) = BASIC_FONTS.get(ch) {
                for (row, bits) in g.iter().enumerate() {
                    for col in 0..8 {
                        if bits & (1 << col) != 0 {
                            self.rect(cx + col * scale, y + row as i32 * scale, scale, scale, c);
                        }
                    }
                }
            }
            cx += 8 * scale;
        }
    }

    pub fn dot(&mut self, cx: f64, cy: f64, r: f64, c: u32) {
        let (x0, x1) = ((cx - r).floor() as i32, (cx + r).ceil() as i32);
        let (y0, y1) = ((cy - r).floor() as i32, (cy + r).ceil() as i32);
        for y in y0.max(0)..y1.min(self.h as i32) {
            for x in x0.max(0)..x1.min(self.w as i32) {
                if (x as f64 - cx).hypot(y as f64 - cy) <= r {
                    self.px[y as usize * self.w + x as usize] = c;
                }
            }
        }
    }

    /// Copies the pre-drawn record in at (cx, cy), then on top: your picture (`art`: a
    /// square of `img.side()` pixels each way, showing across the whole record or only on
    /// its label, as `img` was made) and the cue line, both turned to `angle` (radians,
    /// 0 = 12 o'clock). Only the record's own circle is
    /// touched, so whatever is behind its corners stays.
    pub fn platter(&mut self, cx: i32, cy: i32, img: &PlatterImage, angle: f64, art: Option<&[u32]>, cue: bool) {
        let half = img.size as i32 / 2;
        let art = art.filter(|a| a.len() == img.size * img.size);
        let (sn, co) = angle.sin_cos();
        // Positions on the picture in 1/65536ths of a pixel, so the turn is smooth.
        let turn = ((sn * 65536.0) as i32, (co * 65536.0) as i32);
        // The canvas rows the record covers.
        let (ya, yb) = ((cy - half).max(0), (cy + half + 1).min(self.h as i32));
        if ya < yb && self.w > 0 {
            let w = self.w;
            let band = &mut self.px[ya as usize * w..yb as usize * w];
            // Turning a picture is real work for every pixel, so it's shared between cores.
            let cores = if art.is_some() { std::thread::available_parallelism().map_or(1, |n| n.get()).min(4) } else { 1 };
            let rows_each = ((yb - ya) as usize).div_ceil(cores).max(1);
            if cores == 1 {
                platter_rows(band, w, ya, cx, cy, img, art, turn);
            } else {
                std::thread::scope(|sc| {
                    for (i, part) in band.chunks_mut(rows_each * w).enumerate() {
                        let y = ya + (i * rows_each) as i32;
                        sc.spawn(move || platter_rows(part, w, y, cx, cy, img, art, turn));
                    }
                });
            }
        }
        let (r, label) = (img.r, img.r * LABEL_PART);
        if cue {
            let a = angle - std::f64::consts::FRAC_PI_2;
            let (s, co) = a.sin_cos();
            let (cx, cy) = (cx as f64, cy as f64);
            // The cue sticker line runs from the label's edge to the rim.
            self.line(cx + co * (label + 3.0), cy + s * (label + 3.0), cx + co * (r - 6.0), cy + s * (r - 6.0), 4, GREEN);
        }
    }
}

/// The colour at (x, y) in a `w` x `h` picture, blended from the four pixels around it.
pub fn bilerp(px: &[u32], w: usize, h: usize, x: f64, y: f64) -> u32 {
    let x = x.clamp(0.0, (w - 1) as f64);
    let y = y.clamp(0.0, (h - 1) as f64);
    let (xa, ya) = (x as usize, y as usize);
    let (xb, yb) = ((xa + 1).min(w - 1), (ya + 1).min(h - 1));
    let (fx, fy) = (((x - xa as f64) * 255.0) as u32, ((y - ya as f64) * 255.0) as u32);
    let top = mix(px[ya * w + xa], px[ya * w + xb], fx);
    let bottom = mix(px[yb * w + xa], px[yb * w + xb], fx);
    mix(top, bottom, fy)
}

/// Draws the record into `rows` (whole canvas rows, `w` wide, the first being row `y0`).
#[allow(clippy::too_many_arguments)]
fn platter_rows(rows: &mut [u32], w: usize, y0: i32, cx: i32, cy: i32, img: &PlatterImage, art: Option<&[u32]>, (fsn, fco): (i32, i32)) {
    let half = img.size as i32 / 2;
    for (k, row) in rows.chunks_mut(w).enumerate() {
        // This row's slice of the circle.
        let dy = y0 + k as i32 - cy;
        if (dy as f64).abs() > img.r {
            continue;
        }
        let hw = (img.r * img.r - (dy * dy) as f64).max(0.0).sqrt() as i32;
        let xa = (cx - hw).max(0);
        let xb = (cx + hw + 1).min(w as i32);
        if xa >= xb {
            continue;
        }
        let src = (dy + half) as usize * img.size + (xa - cx + half) as usize;
        let n = (xb - xa) as usize;
        let out = &mut row[xa as usize..xb as usize];
        let Some(art) = art else {
            out.copy_from_slice(&img.px[src..src + n]);
            continue;
        };
        // Each screen point turned back to where it sits on the unturned picture. Turning
        // keeps it inside the record, so inside the picture.
        let dx = xa - cx;
        let mut u = (half << 16) + dx * fco + dy * fsn;
        let mut v = (half << 16) - dx * fsn + dy * fco;
        for ((o, &veil), &plain) in out.iter_mut().zip(&img.veil[src..src + n]).zip(&img.px[src..src + n]) {
            *o = if veil == NO_ART {
                plain
            } else {
                let at = (v >> 16) as usize * img.size + (u >> 16) as usize;
                let (fx, fy) = ((u >> 8 & 255) as u32, (v >> 8 & 255) as u32);
                let (a, b) = (&art[at..at + 2], &art[at + img.size..at + img.size + 2]);
                lerp256(lerp256(lerp256(a[0], a[1], fx), lerp256(b[0], b[1], fx), fy), 0, veil as u32)
            };
            u += fco;
            v -= fsn;
        }
    }
}

/// `a` moved `f`/256 of the way to `b`, two colour channels at a time.
#[inline]
fn lerp256(a: u32, b: u32, f: u32) -> u32 {
    let rb = ((a & 0xff00ff) * (256 - f) + (b & 0xff00ff) * f) >> 8;
    let g = ((a & 0x00ff00) * (256 - f) + (b & 0x00ff00) * f) >> 8;
    (rb & 0xff00ff) | (g & 0x00ff00)
}

/// The label's radius, as a part of the record's.
const LABEL_PART: f64 = 0.33;
/// In a record's veil: your picture doesn't show here (rim, spindle, outside the record).
const NO_ART: u8 = 255;
/// How much the two groove rings darken your picture, out of 256.
const GROOVE_VEIL: (u8, u8) = (30, 78);

/// The record without its cue line: grooves, label, rim. It looks the same at every
/// angle, so it's drawn once per size (and hand-on rim colour, grooves on or off, and
/// where a picture goes) and copied each frame.
pub struct PlatterImage {
    pub r: f64,
    pub held: bool,
    pub grooves: bool,
    /// A picture shows only on the label, not across the whole record.
    pub label_art: bool,
    size: usize,
    px: Vec<u32>,
    /// For a picture on the record: how much each pixel darkens it (the groove rings), or
    /// `NO_ART` where the record itself shows instead.
    veil: Vec<u8>,
}

impl PlatterImage {
    /// The side of the square picture that covers the record.
    pub fn side(&self) -> usize {
        self.size
    }

    /// The side of the square that covers just the label, centred in the one above.
    pub fn label_side(&self) -> usize {
        2 * (self.r * LABEL_PART).ceil() as usize + 1
    }

    pub fn new(r: f64, held: bool, grooves: bool, label_art: bool) -> Self {
        let c = r.ceil() as i32;
        let size = (2 * c + 1) as usize;
        let label = r * LABEL_PART;
        let mut px = vec![BG; size * size];
        let mut veil = vec![NO_ART; size * size];
        for iy in 0..size {
            for ix in 0..size {
                let d = ((ix as i32 - c) as f64).hypot((iy as i32 - c) as f64);
                if d > r {
                    continue;
                }
                if label_art {
                    // Inside the label's thin rim, with nothing over the picture.
                    if d >= 4.0 && d <= label - 2.0 {
                        veil[iy * size + ix] = 0;
                    }
                } else if d >= 4.0 && d <= r - 3.0 {
                    veil[iy * size + ix] = match (grooves, (d as i32 / 3) % 2 == 0) {
                        (false, _) => 0,
                        (true, true) => GROOVE_VEIL.0,
                        (true, false) => GROOVE_VEIL.1,
                    };
                }
                px[iy * size + ix] = if d < 4.0 {
                    0x9a9aa8
                } else if d < label {
                    // A thin rim so the black label reads against the grooves.
                    if d > label - 2.0 { 0x2a2a34 } else { LABEL }
                } else if d > r - 3.0 {
                    if held { LILAC } else { 0x2a2a34 }
                } else if !grooves {
                    FLAT
                } else if (d as i32 / 3) % 2 == 0 {
                    0x1b1b22
                } else {
                    0x141419
                };
            }
        }
        PlatterImage { r, held, grooves, label_art, size, px, veil }
    }
}

/// Min/max peaks per column for samples `v0..v1` (fractional, so it zooms smoothly; when
/// zoomed past one sample per column each column spans the next sample too, giving a
/// joined-up line).
pub fn peaks_range(l: &[f32], r: &[f32], v0: f64, v1: f64, cols: usize) -> Vec<(f32, f32)> {
    let n = l.len();
    let spp = (v1 - v0) / cols as f64;
    (0..cols)
        .map(|c| {
            let a = (v0 + c as f64 * spp).floor().max(0.0) as usize;
            let b = ((v0 + (c + 1) as f64 * spp).ceil() as usize).max(a + 2).min(n);
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for i in a.min(n)..b {
                let v = 0.5 * (l[i] + r[i]);
                lo = lo.min(v);
                hi = hi.max(v);
            }
            if lo > hi { (0.0, 0.0) } else { (lo.min(0.0).min(hi), hi.max(lo)) }
        })
        .collect()
}

/// The app icon: a record cut to a crescent (opening to the upper right), split across
/// its band by a zig-zag, green towards the upper left and purple towards the lower right,
/// with a white outline. The outline stays visible at small sizes: about 1 px at 22,
/// 2 px at 64 and 6 px at 220; the crescent shrinks inside it to keep the same overall
/// size. Returns `size` x `size` pixels as 0xAARRGGBB, transparent outside.
pub fn record_icon(size: usize) -> Vec<u32> {
    // Worked out on a 100-unit square, centred on the record.
    let unit = 100.0 / size as f64;
    let outline = icon_outline_px(size as f64) * unit;
    // The outline's outer edge sits at 49; the crescent is scaled by k inside it.
    let k = (49.0 - outline) / 47.0;
    let (cx, cy) = (15.0 * k, -13.0 * k);
    let (disc, cut) = (47.0 * k, 41.0 * k);
    // The zig-zag on the unscaled record: a triangle wave down the middle, four points
    // tall, turned 45 degrees so it crosses the crescent's band.
    let zig = |y: f64| -> f64 {
        let t = (y + 47.0) / (2.0 * 47.0 / 4.0);
        let tri = 1.0 - 4.0 * (t.fract() - 0.5).abs();
        tri * 47.0 * 0.11
    };
    let s45 = std::f64::consts::FRAC_1_SQRT_2;
    let ss = 4;
    let mut out = vec![0u32; size * size];
    for py in 0..size {
        for px in 0..size {
            let (mut cov, mut rgb) = (0u32, [0u32; 3]);
            for sy in 0..ss {
                for sx in 0..ss {
                    let x = (px as f64 + (sx as f64 + 0.5) / ss as f64) * unit - 50.0;
                    let y = (py as f64 + (sy as f64 + 0.5) / ss as f64) * unit - 50.0;
                    let d = x.hypot(y);
                    let dc = (x - cx).hypot(y - cy);
                    let col = if d <= disc && dc >= cut {
                        // Back on the unscaled record, then turned back by 45 degrees.
                        let (ux, uy) = (x / k, y / k);
                        let (qx, qy) = (ux * s45 + uy * s45, -ux * s45 + uy * s45);
                        if qx < zig(qy) { GREEN } else { PURPLE }
                    } else if d <= 49.0 && dc >= cut - outline {
                        0xffffff
                    } else {
                        continue;
                    };
                    cov += 1;
                    for (i, c) in rgb.iter_mut().enumerate() {
                        *c += (col >> (16 - 8 * i)) & 0xff;
                    }
                }
            }
            if cov > 0 {
                let a = cov * 255 / (ss * ss) as u32;
                let [r, g, b] = rgb.map(|c| c / cov);
                out[py * size + px] = (a << 24) | (r << 16) | (g << 8) | b;
            }
        }
    }
    out
}

/// The icon's outline width in pixels at a given size: 1 at 22, 1.25 at 32, 2 at 64 and
/// 6 at 220, in straight lines between, never under 1, and growing in step with the icon
/// past 220.
fn icon_outline_px(size: f64) -> f64 {
    const AT: [(f64, f64); 4] = [(22.0, 1.0), (32.0, 1.25), (64.0, 2.0), (220.0, 6.0)];
    if size <= AT[0].0 {
        return 1.0;
    }
    for w in AT.windows(2) {
        let ((s0, p0), (s1, p1)) = (w[0], w[1]);
        if size <= s1 {
            return p0 + (p1 - p0) * (size - s0) / (s1 - s0);
        }
    }
    6.0 * size / 220.0
}

impl Canvas<'_> {
    /// Draws a 0xAARRGGBB image (such as `record_icon`) with its transparency.
    pub fn image(&mut self, x: i32, y: i32, size: usize, img: &[u32]) {
        for iy in 0..size {
            for ix in 0..size {
                let p = img[iy * size + ix];
                let a = (p >> 24) as u8;
                if a > 0 {
                    self.blend(x + ix as i32, y + iy as i32, p & 0xffffff, a);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// Drawing the record changes exactly the pixels of its circle and leaves everything
    /// outside it alone, so a background behind the record's corners survives.
    #[test]
    fn platter_stays_in_its_circle() {
        use super::*;
        let (w, h, r) = (200usize, 180usize, 61.5);
        let img = PlatterImage::new(r, false, true, false);
        let mark = 0x123456;
        let mut px = vec![mark; w * h];
        let (cx, cy) = (90, 80);
        Canvas { px: &mut px, w, h }.platter(cx, cy, &img, 0.7, None, false);
        let c = r.ceil() as i32;
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let d = ((x - cx) as f64).hypot((y - cy) as f64);
                let got = px[y as usize * w + x as usize];
                if d > r {
                    assert_eq!(got, mark, "outside the record at {x},{y}");
                } else {
                    let want = img.px[(y - cy + c) as usize * img.size + (x - cx + c) as usize];
                    assert_eq!(got, want, "on the record at {x},{y}");
                }
            }
        }
    }

    /// `ICON_DUMP=path cargo test icon` writes the icon at 256 px as raw ARGB, for looking at.
    #[test]
    fn icon_renders() {
        let img = super::record_icon(256);
        assert!(img.iter().any(|p| p >> 24 == 255) && img.iter().any(|p| p >> 24 == 0));
        if let Some(path) = std::env::var_os("ICON_DUMP") {
            let bytes: Vec<u8> = img.iter().flat_map(|p| p.to_be_bytes()).collect();
            std::fs::write(path, bytes).unwrap();
        }
    }
}

/// Writes 0xAARRGGBB pixels as an RGBA PNG (uncompressed deflate: icons are tiny, and
/// this keeps CUT free of an image library).
pub fn write_png(path: &std::path::Path, size: usize, px: &[u32]) -> std::io::Result<()> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    let mut raw = Vec::with_capacity(size * (size * 4 + 1));
    for row in px.chunks(size) {
        raw.push(0);
        for p in row {
            raw.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8]);
        }
    }
    // zlib stream of stored (uncompressed) blocks.
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        let n = block.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let chunk = |out: &mut Vec<u8>, kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(size as u32).to_be_bytes());
    ihdr.extend_from_slice(&(size as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, png)
}

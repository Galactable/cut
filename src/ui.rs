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

fn mix(bg: u32, fg: u32, a: u32) -> u32 {
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

    /// Copies the pre-drawn record in at (cx, cy), then draws the cue line at `angle`
    /// (radians, 0 = 12 o'clock) on top.
    pub fn platter(&mut self, cx: i32, cy: i32, img: &PlatterImage, angle: f64) {
        let half = img.size as i32 / 2;
        let (x0, y0) = (cx - half, cy - half);
        let xa = x0.max(0);
        let xb = (x0 + img.size as i32).min(self.w as i32);
        if xa < xb {
            for iy in 0..img.size as i32 {
                let y = y0 + iy;
                if y < 0 || y >= self.h as i32 {
                    continue;
                }
                let src = iy as usize * img.size + (xa - x0) as usize;
                let dst = y as usize * self.w + xa as usize;
                let n = (xb - xa) as usize;
                self.px[dst..dst + n].copy_from_slice(&img.px[src..src + n]);
            }
        }
        let (r, label) = (img.r, img.r * 0.33);
        let a = angle - std::f64::consts::FRAC_PI_2;
        let (s, co) = a.sin_cos();
        let (cx, cy) = (cx as f64, cy as f64);
        // The cue sticker line runs from the label's edge to the rim.
        self.line(cx + co * (label + 3.0), cy + s * (label + 3.0), cx + co * (r - 6.0), cy + s * (r - 6.0), 4, GREEN);
    }
}

/// The record without its cue line: grooves, label, rim. It looks the same at every
/// angle, so it's drawn once per size (and hand-on rim colour) and copied each frame.
pub struct PlatterImage {
    pub r: f64,
    pub held: bool,
    size: usize,
    px: Vec<u32>,
}

impl PlatterImage {
    pub fn new(r: f64, held: bool) -> Self {
        let c = r.ceil() as i32;
        let size = (2 * c + 1) as usize;
        let label = r * 0.33;
        let mut px = vec![BG; size * size];
        for iy in 0..size {
            for ix in 0..size {
                let d = ((ix as i32 - c) as f64).hypot((iy as i32 - c) as f64);
                if d > r {
                    continue;
                }
                px[iy * size + ix] = if d < 4.0 {
                    0x9a9aa8
                } else if d < label {
                    // A thin rim so the black label reads against the grooves.
                    if d > label - 2.0 { 0x2a2a34 } else { LABEL }
                } else if d > r - 3.0 {
                    if held { LILAC } else { 0x2a2a34 }
                } else if (d as i32 / 3) % 2 == 0 {
                    0x1b1b22
                } else {
                    0x141419
                };
            }
        }
        PlatterImage { r, held, size, px }
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

/// The app icon: a plain flat record with a bite taken out of its edge (upper right) and
/// the spindle hole, split down the middle by a zig-zag: green left, purple right. Returns `size` x `size` pixels as 0xAARRGGBB,
/// transparent outside the record.
pub fn record_icon(size: usize) -> Vec<u32> {
    let r = size as f64 * 0.47;
    let c = size as f64 / 2.0;
    // The bite: overlapping round tooth marks just past the rim, like a bitten cookie.
    let bites: Vec<(f64, f64, f64)> = [(-1.13f64, 0.20f64), (-0.79, 0.25), (-0.45, 0.20)]
        .iter()
        .map(|&(a, rad)| (a.cos() * r * 0.98, a.sin() * r * 0.98, rad * r))
        .collect();
    // The zig-zag: a sharp triangle wave down the middle, four points tall.
    let zig = |y: f64| -> f64 {
        let t = (y + r) / (2.0 * r / 4.0);
        let tri = 1.0 - 4.0 * (t.fract() - 0.5).abs();
        tri * r * 0.11
    };
    let ss = 4;
    let mut out = vec![0u32; size * size];
    for py in 0..size {
        for px in 0..size {
            let (mut cov, mut green) = (0u32, 0u32);
            for sy in 0..ss {
                for sx in 0..ss {
                    let x = px as f64 + (sx as f64 + 0.5) / ss as f64 - c;
                    let y = py as f64 + (sy as f64 + 0.5) / ss as f64 - c;
                    let d = x.hypot(y);
                    let bitten = bites.iter().any(|&(bx, by, br)| (x - bx).hypot(y - by) < br);
                    if d <= r && d >= r * 0.09 && !bitten {
                        cov += 1;
                        if x < zig(y) {
                            green += 1;
                        }
                    }
                }
            }
            if cov > 0 {
                let a = cov * 255 / (ss * ss) as u32;
                // Blend along the zig-zag so its edge is smooth at small sizes.
                let mixc = |s: u32| (((GREEN >> s) & 0xff) * green + ((PURPLE >> s) & 0xff) * (cov - green)) / cov;
                out[py * size + px] = (a << 24) | (mixc(16) << 16) | (mixc(8) << 8) | mixc(0);
            }
        }
    }
    out
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

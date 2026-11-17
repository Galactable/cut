//! Your own pictures: one behind everything, one on the record.

use std::path::Path;

use image::ImageDecoder;

use crate::ui::{bilerp, mix};

/// Longest side kept after loading: enough for a 4K screen.
const MAX_SIDE: u32 = 3840;

/// A decoded picture as 0xRRGGBB pixels: full size first, then each level half the one
/// before, so shrinking it a lot stays smooth.
pub struct Picture {
    levels: Vec<(usize, usize, Vec<u32>)>,
}

pub fn load(path: &Path) -> Result<Picture, String> {
    let err = |e: image::ImageError| e.to_string();
    let reader = image::ImageReader::open(path).and_then(|r| r.with_guessed_format()).map_err(|e| e.to_string())?;
    let mut decoder = reader.into_decoder().map_err(err)?;
    // Phone photos are often stored sideways with a note saying which way is up.
    let orientation = decoder.orientation().map_err(err)?;
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(err)?;
    img.apply_orientation(orientation);
    if img.width().max(img.height()) > MAX_SIDE {
        img = img.thumbnail(MAX_SIDE, MAX_SIDE);
    }
    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width() as usize, rgb.height() as usize);
    if w == 0 || h == 0 {
        return Err("the picture is empty".into());
    }
    let px: Vec<u32> = rgb.pixels().map(|p| (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32).collect();
    let mut levels = vec![(w, h, px)];
    loop {
        let (w, h, px) = levels.last().unwrap();
        if (*w).min(*h) < 32 {
            break;
        }
        let (w2, h2) = (w / 2, h / 2);
        let mut half = Vec::with_capacity(w2 * h2);
        for y in 0..h2 {
            for x in 0..w2 {
                let at = |dx: usize, dy: usize| px[(2 * y + dy) * w + 2 * x + dx];
                let (a, b, c, d) = (at(0, 0), at(1, 0), at(0, 1), at(1, 1));
                let ch = |s: u32| ((((a >> s) & 0xff) + ((b >> s) & 0xff) + ((c >> s) & 0xff) + ((d >> s) & 0xff) + 2) / 4) << s;
                half.push(ch(16) | ch(8) | ch(0));
            }
        }
        levels.push((w2, h2, half));
    }
    Ok(Picture { levels })
}

impl Picture {
    /// Scaled to fill `w` x `h` (cropped around the middle if the shape differs), then
    /// faded `dim` percent towards the colour `toward`.
    pub fn cover(&self, w: usize, h: usize, dim: u32, toward: u32) -> Vec<u32> {
        let (w, h) = (w.max(1), h.max(1));
        let fit = |lw: usize, lh: usize| (lw as f64 / w as f64).min(lh as f64 / h as f64);
        // The smallest level that still has at least a pixel per pixel.
        let (lw, lh, px) = self.levels.iter().rev().find(|(lw, lh, _)| fit(*lw, *lh) >= 1.0).unwrap_or(&self.levels[0]);
        let scale = fit(*lw, *lh);
        let (ox, oy) = ((*lw as f64 - w as f64 * scale) / 2.0, (*lh as f64 - h as f64 * scale) / 2.0);
        let a = dim.min(100) * 255 / 100;
        let mut out = Vec::with_capacity(w * h);
        for y in 0..h {
            let sy = oy + (y as f64 + 0.5) * scale - 0.5;
            for x in 0..w {
                let p = bilerp(px, *lw, *lh, ox + (x as f64 + 0.5) * scale - 0.5, sy);
                out.push(if a == 0 { p } else { mix(p, toward, a) });
            }
        }
        out
    }

    /// A square `side` each way, plain `around` except for the middle `inner` x `inner`,
    /// which the picture fills: for a picture on just the record's label.
    pub fn cover_middle(&self, side: usize, inner: usize, around: u32) -> Vec<u32> {
        let inner = inner.min(side);
        let small = self.cover(inner, inner, 0, around);
        let mut out = vec![around; side * side];
        let off = (side - inner) / 2;
        for (y, row) in small.chunks(inner).enumerate() {
            out[(y + off) * side + off..][..inner].copy_from_slice(row);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    /// A PNG written to disk loads back, fills a differently shaped area from its middle,
    /// and dims towards the given colour. `PIC_DUMP=dir cargo test pics` also writes a
    /// record with the picture on it, for looking at.
    #[test]
    fn loads_covers_and_dims() {
        use crate::ui::{write_png, Canvas, PlatterImage};
        let dir = std::env::temp_dir().join(format!("cut-pics-test-{}", std::process::id()));
        let path = dir.join("quarters.png");
        // Four quarters: red, green over blue, white.
        let side = 64;
        let quarter = |x: usize, y: usize| match (x < side / 2, y < side / 2) {
            (true, true) => 0xffff0000u32,
            (false, true) => 0xff00ff00,
            (true, false) => 0xff0000ff,
            (false, false) => 0xffffffff,
        };
        let px: Vec<u32> = (0..side * side).map(|i| quarter(i % side, i / side)).collect();
        write_png(&path, side, &px).unwrap();
        let pic = super::load(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        // Twice as wide as tall: the middle band of the picture, so all four colours show.
        let out = pic.cover(200, 100, 0, 0);
        assert_eq!(out.len(), 200 * 100);
        assert_eq!(out[10 * 200 + 20], 0xff0000);
        assert_eq!(out[10 * 200 + 180], 0x00ff00);
        assert_eq!(out[90 * 200 + 20], 0x0000ff);
        assert_eq!(out[90 * 200 + 180], 0xffffff);
        // Shrunk a lot it still lands on the right colours, and full dim is the plain colour.
        assert_eq!(pic.cover(8, 8, 0, 0)[0], 0xff0000);
        assert_eq!(pic.cover(8, 8, 100, 0x0e0e12)[0], 0x0e0e12);
        assert_eq!(pic.cover(8, 8, 50, 0)[0], 0x800000);

        // On the label only: the picture shows in the middle of the record and the
        // grooves around it are the record's own.
        let img = PlatterImage::new(100.0, false, true, true);
        let (side, mid) = (img.side(), img.side() as i32 / 2);
        let art = pic.cover_middle(side, img.label_side(), 0);
        let mut label = vec![0u32; side * side];
        Canvas { px: &mut label, w: side, h: side }.platter(mid, mid, &img, 0.0, Some(&art), false);
        let mut bare = vec![0u32; side * side];
        Canvas { px: &mut bare, w: side, h: side }.platter(mid, mid, &img, 0.0, None, false);
        let at = |dx: i32, dy: i32| ((mid + dy) * side as i32 + mid + dx) as usize;
        assert_eq!(label[at(-15, -15)], 0xff0000);
        assert_eq!(label[at(15, 15)], 0xffffff);
        assert_eq!(label[at(60, 0)], bare[at(60, 0)]);
        assert_eq!(label[at(0, -90)], bare[at(0, -90)]);
        assert_ne!(label[at(-15, -15)], bare[at(-15, -15)]);

        if let Some(out_dir) = std::env::var_os("PIC_DUMP") {
            let (w, r) = (420usize, 200.0);
            for (name, grooves, cue, angle, label) in [("whole", true, true, 0.0, false), ("turned", true, true, 0.9, false), ("plain", false, false, 0.9, false), ("label", true, true, 0.9, true)] {
                let img = PlatterImage::new(r, false, grooves, label);
                let art = if label { pic.cover_middle(img.side(), img.label_side(), 0) } else { pic.cover(img.side(), img.side(), 0, 0) };
                let mut px = pic.cover(w, w, 60, 0x0e0e12);
                Canvas { px: &mut px, w, h: w }.platter(210, 210, &img, angle, Some(&art), cue);
                let argb: Vec<u32> = px.iter().map(|p| 0xff000000 | p).collect();
                write_png(&std::path::Path::new(&out_dir).join(format!("{name}.png")), w, &argb).unwrap();
            }
            // How long a fullscreen-sized record with a picture takes to draw.
            let (w, r) = (1100usize, 520.0);
            let img = PlatterImage::new(r, false, true, false);
            let art = pic.cover(img.side(), img.side(), 0, 0);
            let mut px = vec![0u32; w * w];
            for (what, base, step, with, cue) in [("turning", 0.0, 0.1, true, true), ("flat", 0.0, 0.0, true, false), ("sideways", 1.57, 0.0, true, false), ("no picture", 0.0, 0.1, false, false), ("line only", 0.0, 0.1, false, true)] {
                let t = std::time::Instant::now();
                for i in 0..50 {
                    Canvas { px: &mut px, w, h: w }.platter(550, 550, &img, base + i as f64 * step, Some(&art[..]).filter(|_| with), cue);
                }
                eprintln!("record r=520, {what}: {:.2} ms a frame", t.elapsed().as_secs_f64() * 1e3 / 50.0);
            }
        }
    }
}

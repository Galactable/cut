//! Loading samples and pressing them onto a virtual record.

use std::path::Path;

use anyhow::{anyhow, Context};
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error as SymError, formats::FormatOptions,
    io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

use crate::dsp::{riaa_cut, MAX_STRETCH, NZ};

pub struct Source {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
    pub rate: u32,
    pub name: String,
}

impl Source {
    pub fn silent() -> Self {
        Source { l: vec![0.0; 22050], r: vec![0.0; 22050], rate: 44100, name: String::new() }
    }

    pub fn len(&self) -> usize {
        self.l.len()
    }
}

pub fn load(path: &Path) -> anyhow::Result<Source> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .context("unrecognised audio format")?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("no audio track"))?;
    let track_id = track.id;
    let params = track.codec_params.clone();
    let rate = params.sample_rate.unwrap_or(44100);
    let mut decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default())?;

    let (mut l, mut r) = (Vec::new(), Vec::new());
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(_)) | Err(SymError::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let spec = *buf.spec();
        let ch = spec.channels.count().max(1);
        let mut sb = SampleBuffer::<f32>::new(buf.capacity() as u64, spec);
        sb.copy_interleaved_ref(buf);
        for f in sb.samples().chunks(ch) {
            l.push(f[0]);
            r.push(if ch > 1 { f[1] } else { f[0] });
        }
    }
    if l.is_empty() {
        return Err(anyhow!("file decoded to no audio"));
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(Source { l, r, rate, name })
}

/// One turn of a 33 1/3 record, in seconds.
const SEC_PER_REV: f64 = 1.8;

/// Samples kept clear at each end so the widest resampling kernel never runs off the buffer.
pub const MARGIN: usize = NZ * MAX_STRETCH as usize + 16;

/// The virtual record: lead-in, the chosen section, then run-out groove.
/// `l`/`r` hold groove velocity (RIAA pre-emphasised when `eq` is on); `mono` is the
/// dry audio for drawing.
pub struct Record {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
    pub mono: Vec<f32>,
    pub rate: f64,
    pub cue: f64,
    pub region_len: usize,
    pub lo: f64,
    pub hi: f64,
    pub eq: bool,
    /// A locked groove: the needle wraps from `hi` back to `lo` instead of stopping.
    pub looped: bool,
}

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// An empty record sitting in its run-out groove: one revolution of hiss and crackle,
/// with a scuff that ticks once per turn, looping forever. What you hear before a sample
/// is loaded.
fn build_empty(rate: f64) -> Record {
    let cycle = (SEC_PER_REV * rate) as usize;
    let mut rng = Rng(0x2545_f491);
    let mut groove = vec![0f32; cycle];
    let mut pop = 0f32;
    for (i, v) in groove.iter_mut().enumerate() {
        if rng.next() > 1.0 - 2.0 * 7.0 / rate as f32 {
            pop = rng.next() * 0.4;
        }
        // The scuff: same place every revolution.
        if i == cycle / 3 {
            pop = 0.7;
        }
        pop *= 0.55;
        *v = rng.next() * 0.007 + pop;
    }
    let other: Vec<f32> = (0..cycle).map(|i| groove[i] * 0.85 + groove[(i * 7 + 13) % cycle] * 0.15).collect();
    let (l, r) = (wrap_cycle(&groove), wrap_cycle(&other));
    let mono = l.iter().map(|v| v * 0.5).collect();
    Record {
        l,
        r,
        mono,
        rate,
        cue: MARGIN as f64,
        region_len: 0,
        lo: MARGIN as f64,
        hi: (MARGIN + cycle) as f64,
        eq: true,
        looped: true,
    }
}

/// Lays one cycle out with wrapped margins, so the resampler reads straight across the
/// loop point and the engine can wrap the needle without a seam.
fn wrap_cycle(c: &[f32]) -> Vec<f32> {
    let n = c.len();
    (0..n + 2 * MARGIN).map(|i| c[(i + n - MARGIN % n) % n]).collect()
}

/// The section as a locked groove: it repeats seamlessly with no lead-in or run-out.
fn build_loop(src: &Source, start: usize, end: usize, eq: bool, noise: bool) -> Record {
    let rate = src.rate as f64;
    let len = end - start;
    // Crossfade the tail into the head so the loop point doesn't click.
    let x = ((0.005 * rate) as usize).min(len / 4);
    let cycle = (len - x).max(1);
    let take = |ch: &[f32]| -> Vec<f32> {
        (0..cycle)
            .map(|i| {
                let v = ch[start + i];
                if i < x {
                    let t = (i as f32 + 0.5) / x as f32;
                    let (fi, fo) = ((t * std::f32::consts::FRAC_PI_2).sin(), (t * std::f32::consts::FRAC_PI_2).cos());
                    v * fi + ch[start + cycle + i] * fo
                } else {
                    v
                }
            })
            .collect()
    };
    let (mut cl, mut cr) = (take(&src.l), take(&src.r));
    let mono: Vec<f32> = cl.iter().zip(&cr).map(|(a, b)| 0.5 * (a + b)).collect();
    if eq {
        // Run the cut filter round the loop twice so its state matches at the seam.
        for ch in [&mut cl, &mut cr] {
            let mut f = riaa_cut(rate);
            for v in ch.iter() {
                f.process(*v as f64);
            }
            for v in ch.iter_mut() {
                *v = f.process(*v as f64) as f32;
            }
        }
    }
    if noise {
        let mut rng = Rng(0x9e37_79b9);
        let mut pop = 0f32;
        for i in 0..cycle {
            if rng.next() > 1.0 - 2.0 * 3.0 / rate as f32 {
                pop = rng.next() * 0.25;
            }
            pop *= 0.6;
            let v = rng.next() * 0.0025 + pop;
            cl[i] += v;
            cr[i] += v * 0.8;
        }
    }
    Record {
        l: wrap_cycle(&cl),
        r: wrap_cycle(&cr),
        mono: wrap_cycle(&mono),
        rate,
        cue: MARGIN as f64,
        region_len: cycle,
        lo: MARGIN as f64,
        hi: (MARGIN + cycle) as f64,
        eq,
        looped: true,
    }
}

pub fn build(src: &Source, start: usize, end: usize, eq: bool, noise: bool, looped: bool) -> Record {
    if src.name.is_empty() {
        return build_empty(src.rate as f64);
    }
    let end = end.min(src.len()).max(start + 1);
    if looped && end - start > 64 {
        return build_loop(src, start, end, eq, noise);
    }
    let rate = src.rate as f64;
    let region_len = end - start;
    let pre = (1.0 * rate) as usize + MARGIN;
    let post = (3.0 * rate) as usize + MARGIN;
    let n = pre + region_len + post;
    let mut l = vec![0f32; n];
    let mut r = vec![0f32; n];

    // Short fades so the section edges don't click when you cut a sample out of a loop.
    let fade = ((0.002 * rate) as usize).min(region_len / 2).max(1);
    for i in 0..region_len {
        let g = (i.min(region_len - 1 - i) as f32 / fade as f32).min(1.0);
        l[pre + i] = src.l[start + i] * g;
        r[pre + i] = src.r[start + i] * g;
    }
    let mono: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect();

    if eq {
        for ch in [&mut l, &mut r] {
            let mut f = riaa_cut(rate);
            for v in ch.iter_mut() {
                *v = f.process(*v as f64) as f32;
            }
        }
    }

    if noise {
        // Surface noise lives in the groove, so it gets scratched along with the sample.
        let mut rng = Rng(0x9e37_79b9);
        let mut pop = 0f32;
        for i in 0..n {
            let hiss = rng.next() * 0.0025;
            if rng.next() > 1.0 - 2.0 * 3.0 / rate as f32 {
                pop = rng.next() * 0.25;
            }
            pop *= 0.6;
            let v = hiss + pop;
            l[i] += v;
            r[i] += v * 0.8 + rng.next() * 0.0008;
        }
    }

    Record {
        l,
        r,
        mono,
        rate,
        cue: pre as f64,
        region_len,
        lo: MARGIN as f64,
        hi: (n - MARGIN) as f64,
        eq,
        looped: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_seam_is_smooth() {
        // A section that isn't a whole number of cycles, so a plain loop would click.
        let rate = 44100u32;
        let l: Vec<f32> = (0..30_000).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.8).collect();
        let src = Source { r: l.clone(), l, rate, name: "sine".into() };
        let rec = build(&src, 1234, 23_456, false, false, true);
        assert!(rec.looped);
        let worst = rec.l.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
        // The sine itself moves up to ~0.05 per sample; a click would be far bigger.
        assert!(worst < 0.08, "biggest jump {worst}");
        let plain = build(&src, 1234, 23_456, false, false, false);
        assert!(!plain.looped && plain.cue > plain.lo);
    }
}

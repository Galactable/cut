//! Finding the beat of a backing track: its tempo and where the beats fall.
//!
//! The track is assumed to keep one steady tempo (produced beats and loops do). The
//! result is a grid: a BPM and the time of a bar's first beat.

use crate::deck::Source;

/// Onset-strength frames per second.
const ER: f64 = 250.0;
/// Slices of one beat when folding the track onto itself.
const BINS: usize = 96;
/// How much stronger the best beat position must be than the average for the grid to
/// be trusted. Noise and beatless audio sit near 1.
const TRUST: f64 = 1.6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub bpm: f64,
    /// A bar's first beat, in seconds from the start of the track.
    pub first: f64,
}

impl Grid {
    pub fn period(&self) -> f64 {
        60.0 / self.bpm
    }

    /// Keeps `first` as the earliest bar start at or after 0:00.
    pub fn tidy(mut self) -> Self {
        self.first = self.first.rem_euclid(4.0 * self.period());
        self
    }

    /// The same beats at another tempo, keeping the beat nearest `t` where it is.
    pub fn retimed(self, bpm: f64, t: f64) -> Self {
        let k = ((t - self.first) / self.period()).round();
        let anchor = self.first + k * self.period();
        Grid { bpm, first: anchor - k * 60.0 / bpm }.tidy()
    }
}

/// What was heard in a track: how strongly sounds start at each moment, overall and in
/// the bass (kicks), plus the grid found from it.
pub struct Analysis {
    env: Vec<f32>,
    low: Vec<f32>,
    pub grid: Option<Grid>,
}

impl Analysis {
    pub fn of(src: &Source) -> Self {
        let (env, low) = onsets(src);
        let mut a = Analysis { env, low, grid: None };
        a.grid = a.detect();
        a
    }

    fn detect(&self) -> Option<Grid> {
        let coarse = self.coarse_bpm()?;
        let (bpm, phase, trust) = self.fold(coarse, 0.015)?;
        if trust < TRUST {
            return None;
        }
        let period = 60.0 / bpm;
        // Beat one: the beat of the bar where the bass hits hardest.
        let frames = self.low.len();
        let mut weight = [0f64; 4];
        let mut k = 0usize;
        loop {
            let i = ((phase + k as f64 * period) * ER) as usize;
            if i >= frames {
                break;
            }
            let (a, b) = (i.saturating_sub(2), (i + 3).min(frames));
            weight[k % 4] += self.low[a..b].iter().fold(0f32, |m, v| m.max(*v)) as f64;
            k += 1;
        }
        let one = (0..4).max_by(|a, b| weight[*a].total_cmp(&weight[*b])).unwrap_or(0);
        Some(Grid { bpm, first: phase + one as f64 * period }.tidy())
    }

    /// A first guess from how the track repeats, good to a fraction of a percent.
    fn coarse_bpm(&self) -> Option<f64> {
        let n = self.env.len();
        let max_lag = (ER * 60.0 / 60.0) as usize * 2 + 2;
        if n < max_lag * 3 {
            return None;
        }
        let mean = self.env.iter().sum::<f32>() / n as f32;
        let x: Vec<f32> = self.env.iter().map(|v| v - mean).collect();
        let ac: Vec<f64> = (0..=max_lag)
            .map(|lag| x[..n - lag].iter().zip(&x[lag..]).map(|(a, b)| (a * b) as f64).sum::<f64>() / (n - lag) as f64)
            .collect();
        // 60 to 180 BPM, leaning towards the middle of where scratch beats sit.
        let (lo, hi) = ((ER * 60.0 / 180.0) as usize, (ER * 60.0 / 60.0) as usize);
        let score = |lag: usize| {
            let bpm = ER * 60.0 / lag as f64;
            let prior = (-0.5 * ((bpm / 96.0).log2() / 0.7).powi(2)).exp();
            (ac[lag] + 0.5 * ac[lag * 2]) * prior
        };
        let best = (lo..=hi).max_by(|a, b| score(*a).total_cmp(&score(*b)))?;
        if score(best) <= 0.0 {
            return None;
        }
        // Sharpen between frames with a parabola through the peak.
        let (a, b, c) = (score(best - 1), score(best), score(best + 1));
        let d = a - 2.0 * b + c;
        let off = if d < 0.0 { (0.5 * (a - c) / d).clamp(-0.5, 0.5) } else { 0.0 };
        Some(ER * 60.0 / (best as f64 + off))
    }

    /// Lays every beat of the track on top of the others at each tempo near `bpm` and
    /// keeps the tempo where the hits pile up sharpest. Gives (bpm, time of a beat,
    /// how far the pile stands above the average).
    fn fold(&self, bpm: f64, range: f64) -> Option<(f64, f64, f64)> {
        let n = self.env.len();
        if n < 8 {
            return None;
        }
        let secs = n as f64 / ER;
        // Fine enough that the last beat of the track moves under half a slice per step.
        let step = (0.5 * (60.0 / bpm / BINS as f64) / secs).max(range / 3000.0);
        let steps = (range / step).ceil() as i64;
        let total: f64 = self.env.iter().map(|v| *v as f64).sum();
        if total <= 0.0 {
            return None;
        }
        let mut best = (0.0f64, bpm, 0.0f64);
        let mut hist = [0f64; BINS];
        for s in -steps..=steps {
            let b = bpm * (1.0 + s as f64 * step);
            let inc = b / 60.0 / ER;
            hist.fill(0.0);
            let mut ph = 0.5 * inc;
            for v in &self.env {
                hist[(ph * BINS as f64) as usize % BINS] += *v as f64;
                ph += inc;
                if ph >= 1.0 {
                    ph -= 1.0;
                }
            }
            let at = |i: usize| hist[(i + BINS - 1) % BINS] + hist[i] + hist[(i + 1) % BINS];
            let top = (0..BINS).max_by(|a, b| at(*a).total_cmp(&at(*b))).unwrap_or(0);
            if at(top) > best.0 {
                let (l, m, r) = (at((top + BINS - 1) % BINS), at(top), at((top + 1) % BINS));
                let d = l - 2.0 * m + r;
                let off = if d < 0.0 { (0.5 * (l - r) / d).clamp(-0.5, 0.5) } else { 0.0 };
                let frac = (top as f64 + 0.5 + off) / BINS as f64;
                best = (at(top), b, frac * 60.0 / b);
            }
        }
        let trust = best.0 / (3.0 * total / BINS as f64);
        Some((best.1, best.2, trust))
    }

    /// A grid from tapping along: `taps` are track times in seconds, the first one on
    /// beat one. The taps choose the tempo and which beat is one; the audio then pulls
    /// both exactly onto the track's own beats when it has a clear one.
    pub fn from_taps(&self, taps: &[f64]) -> Option<Grid> {
        if taps.len() < 4 {
            return None;
        }
        let mut gaps: Vec<f64> = taps.windows(2).map(|w| w[1] - w[0]).collect();
        gaps.sort_by(f64::total_cmp);
        let gap = gaps[gaps.len() / 2];
        if !(0.2..=2.0).contains(&gap) {
            return None;
        }
        let tapped = Grid { bpm: 60.0 / gap, first: taps[0] };
        let Some((bpm, phase, trust)) = self.fold(tapped.bpm, 0.04) else { return Some(tapped.tidy()) };
        if trust < TRUST {
            return Some(tapped.tidy());
        }
        // The track's beat nearest the first tap is beat one.
        let period = 60.0 / bpm;
        let k = ((taps[0] - phase) / period).round();
        Some(Grid { bpm, first: phase + k * period }.tidy())
    }
}

/// How strongly sounds start in each 4 ms of the track: (everything, bass only).
fn onsets(src: &Source) -> (Vec<f32>, Vec<f32>) {
    let rate = src.rate as f64;
    let hop = rate / ER;
    let frames = (src.len() as f64 / hop) as usize;
    // Bass below about 150 Hz, split off with two one-pole filters.
    let k = (1.0 - (-std::f64::consts::TAU * 150.0 / rate).exp()) as f32;
    let (mut s1, mut s2) = (0f32, 0f32);
    let (mut e_low, mut e_rest) = (vec![0f32; frames], vec![0f32; frames]);
    for f in 0..frames {
        let (a, b) = ((f as f64 * hop) as usize, (((f + 1) as f64 * hop) as usize).min(src.len()));
        let (mut lo, mut re) = (0f32, 0f32);
        for i in a..b {
            let x = 0.5 * (src.l[i] + src.r[i]);
            s1 += (x - s1) * k;
            s2 += (s1 - s2) * k;
            lo += s2 * s2;
            re += (x - s2) * (x - s2);
        }
        let n = (b - a).max(1) as f32;
        e_low[f] = lo / n;
        e_rest[f] = re / n;
    }
    // Loudness over the last 20 ms (so a bass wave's own cycle doesn't read as a hit),
    // then how much it jumped since the frame before.
    let rise = |e: &[f32]| -> Vec<f32> {
        const WIN: usize = 5;
        let mut out = vec![0f32; e.len()];
        let (mut sum, mut prev) = (0f32, 0f32);
        for i in 0..e.len() {
            sum += e[i];
            if i >= WIN {
                sum -= e[i - WIN];
            }
            let level = (1.0 + 1000.0 * (sum.max(0.0) / WIN as f32).sqrt()).ln();
            if i > 0 {
                out[i] = (level - prev).max(0.0);
            }
            prev = level;
        }
        out
    };
    let low = rise(&e_low);
    let rest = rise(&e_rest);
    let env = low.iter().zip(&rest).map(|(a, b)| a + b).collect();
    (env, low)
}

/// Remembered grids, so a track you've corrected stays corrected: one line per track,
/// `bpm <tab> first <tab> length in samples <tab> path`.
pub mod saved {
    use super::Grid;
    use std::path::{Path, PathBuf};

    fn file() -> PathBuf {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| ".".into());
        home.join(".config").join("cut").join("beats.txt")
    }

    fn parse(line: &str) -> Option<(Grid, usize, &str)> {
        let mut it = line.splitn(4, '\t');
        let (bpm, first, len) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
        Some((Grid { bpm, first }, len, it.next()?))
    }

    pub fn get(path: &Path, len: usize) -> Option<Grid> {
        let text = std::fs::read_to_string(file()).ok()?;
        let want = path.to_string_lossy();
        text.lines().filter_map(parse).find(|(_, l, p)| *l == len && *p == want).map(|(g, _, _)| g)
    }

    /// Stores the grid for a track, or forgets it with `None`.
    pub fn set(path: &Path, len: usize, grid: Option<Grid>) {
        let want = path.to_string_lossy();
        let old = std::fs::read_to_string(file()).unwrap_or_default();
        let mut lines: Vec<String> = old.lines().filter(|l| parse(l).is_some_and(|(_, _, p)| p != want)).map(str::to_string).collect();
        if let Some(g) = grid {
            lines.push(format!("{}\t{}\t{}\t{}", g.bpm, g.first, len, want));
        }
        let f = file();
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(f, lines.join("\n") + "\n");
    }
}

/// `--beat <file>`: prints what was found, for checking tracks from a terminal.
pub fn report(path: &str) -> anyhow::Result<()> {
    let src = crate::deck::load(std::path::Path::new(path))?;
    let started = std::time::Instant::now();
    let a = Analysis::of(&src);
    let took = started.elapsed().as_secs_f64();
    let secs = src.len() as f64 / src.rate as f64;
    match a.grid {
        Some(g) => println!("{:.2} BPM, beat one at {:.3}s  ({secs:.0}s track, analysed in {took:.2}s)", g.bpm, g.first),
        None => println!("no steady beat found  ({secs:.0}s track, analysed in {took:.2}s)"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A drum loop: kick on 1 and 3 (louder on 1), snare on 2 and 4, hats on the
    /// off-beats, over a little noise.
    fn drums(bpm: f64, first: f64, secs: f64) -> Source {
        let rate = 44100u32;
        let n = (secs * rate as f64) as usize;
        let mut l = vec![0f32; n];
        let mut seed = 0x1234_5678u32;
        let mut noise = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32 * 2.0 - 1.0
        };
        for v in l.iter_mut() {
            *v = noise() * 0.01;
        }
        let period = 60.0 / bpm;
        let mut k = -8i64;
        loop {
            for half in 0..2 {
                let t = first + (k as f64 + 0.5 * half as f64) * period;
                let at = (t * rate as f64).round() as i64;
                if at >= n as i64 {
                    return Source { r: l.clone(), l, rate, name: "drums".into() };
                }
                for i in 0..(0.12 * rate as f64) as i64 {
                    let j = at + i;
                    if j < 0 || j >= n as i64 {
                        continue;
                    }
                    let s = i as f32 / rate as f32;
                    let beat = k.rem_euclid(4);
                    l[j as usize] += if half == 1 {
                        noise() * 0.15 * (-s * 120.0).exp()
                    } else if beat % 2 == 0 {
                        (std::f32::consts::TAU * 60.0 * s).sin() * (-s * 25.0).exp() * if beat == 0 { 0.9 } else { 0.6 }
                    } else {
                        noise() * 0.4 * (-s * 40.0).exp()
                    };
                }
            }
            k += 1;
        }
    }

    /// Distance between two times, ignoring whole steps of `step`.
    fn apart(a: f64, b: f64, step: f64) -> f64 {
        let d = (a - b).rem_euclid(step);
        d.min(step - d)
    }

    #[test]
    fn finds_tempo_and_beat_one() {
        for (bpm, first) in [(93.7, 0.21), (86.0, 0.0), (120.0, 0.4), (72.3, 1.1)] {
            let g = Analysis::of(&drums(bpm, first, 60.0)).grid.expect("a clear beat");
            assert!((g.bpm - bpm).abs() < 0.03, "{bpm}: got {}", g.bpm);
            let bar = 4.0 * 60.0 / bpm;
            assert!(apart(g.first, first, bar) < 0.012, "{bpm}: beat one at {} not {first}", g.first);
        }
    }

    #[test]
    fn stays_on_the_beat_to_the_end_of_a_long_track() {
        let (bpm, secs) = (91.37, 240.0);
        let g = Analysis::of(&drums(bpm, 0.3, secs)).grid.unwrap();
        let drift = (g.bpm - bpm).abs() / bpm * secs;
        assert!(drift < 0.015, "{} drifts {drift}s over the track", g.bpm);
    }

    #[test]
    fn noise_has_no_beat() {
        let mut seed = 99u32;
        let l: Vec<f32> = (0..44100 * 30)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as f32 / u32::MAX as f32 - 0.5
            })
            .collect();
        let src = Source { r: l.clone(), l, rate: 44100, name: "noise".into() };
        assert_eq!(Analysis::of(&src).grid, None);
    }

    #[test]
    fn taps_pick_the_tempo_and_beat_one() {
        // The track is at 80; detection alone could just as well say 160. Sloppy taps
        // at 80, starting on the third beat of the bar, settle both.
        let a = Analysis::of(&drums(80.0, 0.25, 60.0));
        let taps: Vec<f64> = (0..6).map(|i| 0.25 + (2 + i) as f64 * 0.75 + 0.04 + 0.02 * (i % 2) as f64).collect();
        let g = a.from_taps(&taps).unwrap();
        assert!((g.bpm - 80.0).abs() < 0.05, "got {}", g.bpm);
        assert!(apart(g.first, 0.25 + 2.0 * 0.75, 3.0) < 0.012, "beat one at {}", g.first);
    }

    #[test]
    fn retiming_keeps_the_nearest_beat_in_place() {
        let g = Grid { bpm: 90.0, first: 0.1 };
        let t = 0.1 + 40.0 * g.period();
        let r = g.retimed(91.0, t + 0.01);
        assert!(apart(r.first, t, r.period()) < 1e-9);
    }
}

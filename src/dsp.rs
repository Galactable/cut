//! Filters and the band-limited resampler.

use std::f64::consts::PI;

#[derive(Clone, Copy, Default)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    /// Bilinear transform of H(s) = (b0 + b1 s + b2 s^2) / (a0 + a1 s + a2 s^2).
    pub fn bilinear(b: [f64; 3], a: [f64; 3], fs: f64) -> Self {
        let k = 2.0 * fs;
        let k2 = k * k;
        let d = |c: [f64; 3]| {
            [
                c[2] * k2 + c[1] * k + c[0],
                2.0 * c[0] - 2.0 * c[2] * k2,
                c[2] * k2 - c[1] * k + c[0],
            ]
        };
        let nb = d(b);
        let na = d(a);
        Biquad {
            b0: nb[0] / na[0],
            b1: nb[1] / na[0],
            b2: nb[2] / na[0],
            a1: na[1] / na[0],
            a2: na[2] / na[0],
            ..Default::default()
        }
    }

    pub fn highpass(fc: f64, q: f64, fs: f64) -> Self {
        let w = 2.0 * PI * fc / fs;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        let a0 = 1.0 + alpha;
        Biquad {
            b0: (1.0 + c) / 2.0 / a0,
            b1: -(1.0 + c) / a0,
            b2: (1.0 + c) / 2.0 / a0,
            a1: -2.0 * c / a0,
            a2: (1.0 - alpha) / a0,
            ..Default::default()
        }
    }

    pub fn gain_at(&self, f: f64, fs: f64) -> f64 {
        let w = 2.0 * PI * f / fs;
        let (c1, s1) = (w.cos(), -w.sin());
        let (c2, s2) = ((2.0 * w).cos(), -(2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = self.b1 * s1 + self.b2 * s2;
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = self.a1 * s1 + self.a2 * s2;
        ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt()
    }

    pub fn scaled(mut self, g: f64) -> Self {
        self.b0 *= g;
        self.b1 *= g;
        self.b2 *= g;
        self
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

// RIAA time constants, plus the 3.18 us Neumann pole so the cut curve is proper.
const T1: f64 = 3180e-6;
const T2: f64 = 318e-6;
const T3: f64 = 75e-6;
const T4: f64 = 3.18e-6;

/// Cutting-lathe pre-emphasis: what the groove velocity looks like for a given audio signal.
pub fn riaa_cut(fs: f64) -> Biquad {
    let f = Biquad::bilinear([1.0, T1 + T3, T1 * T3], [1.0, T2 + T4, T2 * T4], fs);
    let g = f.gain_at(1000.0, fs);
    f.scaled(1.0 / g)
}

/// Phono-preamp de-emphasis, exact inverse of `riaa_cut` at normal speed.
pub fn riaa_play(fs: f64) -> Biquad {
    let f = Biquad::bilinear([1.0, T2 + T4, T2 * T4], [1.0, T1 + T3, T1 * T3], fs);
    let g = f.gain_at(1000.0, fs);
    f.scaled(1.0 / g)
}

/// Zero crossings each side of the sinc kernel at normal speed.
pub const NZ: usize = 10;
const OS: usize = 512;
/// Highest speed (source samples per output sample) the anti-alias filter follows.
pub const MAX_STRETCH: f64 = 24.0;

fn bessel_i0(x: f64) -> f64 {
    let y = x * x / 4.0;
    let (mut sum, mut term) = (1.0, 1.0);
    for k in 1..60 {
        term *= y / ((k * k) as f64);
        sum += term;
        if term < 1e-14 * sum {
            break;
        }
    }
    sum
}

/// Kaiser-windowed sinc whose cutoff drops with playback speed, so fast scratches
/// don't alias the way plain linear/cubic resamplers do.
pub struct Sinc {
    t: Vec<f32>,
}

impl Sinc {
    pub fn new() -> Self {
        let beta = 8.0;
        let n = NZ * OS;
        let mut t = vec![0f32; n + 2];
        for (i, v) in t.iter_mut().enumerate().take(n + 1) {
            let x = i as f64 / OS as f64;
            let s = if i == 0 { 1.0 } else { (PI * x).sin() / (PI * x) };
            let r = x / NZ as f64;
            let w = bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(beta);
            *v = (s * w) as f32;
        }
        Sinc { t }
    }

    #[inline]
    fn k(&self, x: f64) -> f32 {
        let f = x * OS as f64;
        let i = f as usize;
        if i >= NZ * OS {
            return 0.0;
        }
        let fr = (f - i as f64) as f32;
        let a = self.t[i];
        a + (self.t[i + 1] - a) * fr
    }

    /// Read stereo at fractional position `p`, low-passed for a playback speed of `stretch`.
    #[inline]
    pub fn read(&self, l: &[f32], r: &[f32], p: f64, stretch: f64) -> (f32, f32) {
        let half = NZ as f64 * stretch;
        let inv = 1.0 / stretch;
        let len = l.len() as isize;
        let n0 = ((p - half).floor() as isize + 1).max(0);
        let n1 = ((p + half).floor() as isize).min(len - 1);
        let (mut sl, mut sr, mut ws) = (0f32, 0f32, 0f32);
        let mut n = n0;
        while n <= n1 {
            let w = self.k(((n as f64 - p) * inv).abs());
            sl += w * l[n as usize];
            sr += w * r[n as usize];
            ws += w;
            n += 1;
        }
        if ws.abs() < 1e-6 {
            return (0.0, 0.0);
        }
        (sl / ws, sr / ws)
    }
}

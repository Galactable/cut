//! The turntable: platter + slipmat + hand physics, cartridge, phono stage and fader.

use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed},
    Arc,
};

use rtrb::{Consumer, Producer};

use crate::deck::{Record, Source};
use crate::dsp::{riaa_play, Biquad, Sinc, MAX_STRETCH};

pub enum Msg {
    /// New record; `true` keeps the needle where it is (EQ/noise toggles).
    SetRecord(Arc<Record>, bool),
    HandDown { t: u64 },
    /// Hand position in source samples relative to where it touched down, stamped in microseconds.
    HandMove { t: u64, u: f64 },
    HandUp,
    Fader(bool),
    Motor(bool),
    Seek(f64),
    Rpm(f64),
    Volume(f32),
    Smooth(f64),
    Metronome(bool),
    Bpm(f64),
    /// The backing track (None to remove it). The UI keeps its own clone of the track,
    /// so dropping the old one here never frees memory on the audio thread.
    Backing(Option<Arc<Source>>),
    BackingPlay(bool),
    BackingVolume(f32),
    /// Put the backing track into takes too (off: takes are the scratch alone).
    BackingInTake(bool),
}

#[derive(Default)]
pub struct Shared {
    pos: AtomicU64,
    speed: AtomicU64,
    pub level: AtomicU32,
    /// What the UI wants; the engine starts the take (on the next bar if the click is on).
    pub rec_request: AtomicBool,
    /// Whether the engine is actually writing take audio.
    pub recording: AtomicBool,
    pub rec_frames: AtomicU64,
    /// The app is closing: the recorder finishes any take and sets `rec_closed`.
    pub quitting: AtomicBool,
    pub rec_closed: AtomicBool,
}

impl Shared {
    /// Closes an open take cleanly before the app exits (waits at most a couple of seconds).
    pub fn close_take(&self) {
        self.quitting.store(true, Relaxed);
        let t0 = std::time::Instant::now();
        while !self.rec_closed.load(Relaxed) && t0.elapsed() < std::time::Duration::from_secs(3) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

impl Shared {
    pub fn pos(&self) -> f64 {
        f64::from_bits(self.pos.load(Relaxed))
    }
    pub fn speed(&self) -> f64 {
        f64::from_bits(self.speed.load(Relaxed))
    }
}

/// Record catching up to the platter through the slipmat.
const SLIP_TAU: f64 = 0.012;
const START_TAU: f64 = 0.18;
const STOP_TAU: f64 = 0.12;
const FADER_RAMP: f64 = 0.0008;

pub struct Engine {
    rec: Arc<Record>,
    rx: Consumer<Msg>,
    trash: Producer<Arc<Record>>,
    shared: Arc<Shared>,
    take: Producer<f32>,
    out_rate: f64,
    dt: f64,
    sinc: Sinc,

    p: f64,
    /// Record velocity in source samples per second.
    vel: f64,
    platter: f64,
    motor: bool,
    rpm: f64,

    hand: bool,
    base: f64,
    last_u: f64,
    h_from: f64,
    h_to: f64,
    ramp_i: u32,
    ramp_n: u32,
    last_t: u64,
    omega: f64,

    fader: f32,
    fader_target: f32,
    volume: f32,
    play: [Biquad; 2],
    hpf: [Biquad; 2],

    click_on: bool,
    bpm: f64,
    beat_phase: f64,
    beat: u64,
    click_env: f32,
    click_ph: f32,
    click_hz: f32,
    recording: bool,
    rec_frames: u64,

    backing: Option<Arc<Source>>,
    backing_on: bool,
    backing_vol: f32,
    backing_in_take: bool,
    /// Position in the backing track's own samples, and its samples per output sample.
    backing_pos: f64,
}

impl Engine {
    pub fn new(
        rec: Arc<Record>,
        out_rate: f64,
        rx: Consumer<Msg>,
        trash: Producer<Arc<Record>>,
        shared: Arc<Shared>,
        take: Producer<f32>,
    ) -> Self {
        let p = rec.cue;
        Engine {
            rec,
            rx,
            trash,
            shared,
            take,
            out_rate,
            dt: 1.0 / out_rate,
            sinc: Sinc::new(),
            p,
            vel: 0.0,
            platter: 0.0,
            motor: false,
            rpm: 1.0,
            hand: false,
            base: 0.0,
            last_u: 0.0,
            h_from: 0.0,
            h_to: 0.0,
            ramp_i: 0,
            ramp_n: 0,
            last_t: 0,
            omega: 2.0 * std::f64::consts::PI * 70.0,
            fader: 1.0,
            fader_target: 1.0,
            volume: 0.7,
            play: [riaa_play(out_rate); 2],
            hpf: [Biquad::highpass(14.0, 0.707, out_rate); 2],
            click_on: false,
            bpm: 90.0,
            beat_phase: 0.0,
            beat: 0,
            click_env: 0.0,
            click_ph: 0.0,
            click_hz: 1500.0,
            recording: false,
            backing: None,
            backing_on: false,
            backing_vol: 0.7,
            backing_in_take: false,
            backing_pos: 0.0,
            rec_frames: 0,
        }
    }

    fn hand_target(&self) -> f64 {
        if self.ramp_n == 0 {
            self.h_to
        } else {
            self.h_from + (self.h_to - self.h_from) * (self.ramp_i as f64 / self.ramp_n as f64)
        }
    }

    fn handle(&mut self, m: Msg) {
        match m {
            Msg::SetRecord(r, keep) => {
                let off = self.p - self.rec.cue;
                let old = std::mem::replace(&mut self.rec, r);
                let _ = self.trash.push(old);
                self.p = if keep { (self.rec.cue + off).clamp(self.rec.lo, self.rec.hi) } else { self.rec.cue };
                if !keep {
                    self.vel = 0.0;
                }
                self.base = self.p - self.last_u;
                self.h_from = self.p;
                self.h_to = self.p;
                self.ramp_n = 0;
                for f in self.play.iter_mut().chain(self.hpf.iter_mut()) {
                    f.reset();
                }
            }
            Msg::HandDown { t } => {
                self.hand = true;
                self.base = self.p;
                self.last_u = 0.0;
                self.h_from = self.p;
                self.h_to = self.p;
                self.ramp_i = 0;
                self.ramp_n = 0;
                self.last_t = t;
            }
            Msg::HandMove { t, u } => {
                if !self.hand {
                    return;
                }
                // Glide to each new pen sample over the gap since the previous one, so
                // 100-200 Hz tablet reports become a smooth motion instead of a staircase.
                let gap = t.saturating_sub(self.last_t) as f64 * 1e-6;
                self.last_t = t;
                self.h_from = self.hand_target();
                self.h_to = self.base + u;
                self.last_u = u;
                self.ramp_i = 0;
                self.ramp_n = ((gap.clamp(0.0005, 0.012) * self.out_rate) as u32).max(1);
            }
            Msg::HandUp => self.hand = false,
            Msg::Fader(open) => self.fader_target = if open { 1.0 } else { 0.0 },
            Msg::Motor(on) => self.motor = on,
            Msg::Seek(p) => {
                self.p = p.clamp(self.rec.lo, self.rec.hi);
                self.vel = 0.0;
                self.base = self.p - self.last_u;
                self.h_from = self.p;
                self.h_to = self.p;
                self.ramp_n = 0;
            }
            Msg::Rpm(r) => self.rpm = r,
            Msg::Volume(v) => self.volume = v,
            Msg::Smooth(hz) => self.omega = 2.0 * std::f64::consts::PI * hz,
            Msg::Metronome(on) => {
                self.click_on = on;
                // Next sample is the downbeat of bar one.
                self.beat_phase = f64::MAX;
                self.beat = u64::MAX;
            }
            Msg::Bpm(b) => self.bpm = b,
            Msg::Backing(b) => {
                self.backing = b;
                self.backing_pos = 0.0;
            }
            Msg::BackingPlay(on) => self.backing_on = on,
            Msg::BackingVolume(v) => self.backing_vol = v,
            Msg::BackingInTake(on) => self.backing_in_take = on,
        }
    }

    pub fn process(&mut self, out: &mut [f32], ch: usize) {
        while let Ok(m) = self.rx.pop() {
            self.handle(m);
        }
        let rate = self.rec.rate;
        let dt = self.dt;
        let (lo, hi) = (self.rec.lo, self.rec.hi);
        let platter_target = if self.motor { self.rpm * rate } else { 0.0 };
        let fstep = (dt / FADER_RAMP) as f32;
        let rec_request = self.shared.rec_request.load(Relaxed);
        if !rec_request && self.recording {
            self.recording = false;
        }
        let beat_len = self.out_rate * 60.0 / self.bpm;
        let click_decay = (-dt / 0.025).exp() as f32;
        let mut peak = 0f32;

        for frame in out.chunks_mut(ch) {
            // Metronome, and starting a requested take on a downbeat.
            let mut downbeat = !self.click_on;
            if self.click_on {
                self.beat_phase += 1.0;
                if self.beat_phase >= beat_len {
                    self.beat_phase = if self.beat_phase == f64::MAX { 0.0 } else { self.beat_phase - beat_len };
                    self.beat = self.beat.wrapping_add(1);
                    downbeat = self.beat % 4 == 0;
                    self.click_hz = if downbeat { 2000.0 } else { 1400.0 };
                    self.click_env = if downbeat { 0.35 } else { 0.22 };
                    self.click_ph = 0.0;
                }
            }
            if rec_request && !self.recording && downbeat {
                self.recording = true;
                self.rec_frames = 0;
                self.shared.recording.store(true, Relaxed);
                // The backing track starts over with the take, so both begin at 0:00.
                self.backing_pos = 0.0;
            }

            let tau = if platter_target.abs() > self.platter.abs() { START_TAU } else { STOP_TAU };
            self.platter += (platter_target - self.platter) * (dt / tau);

            let p0 = self.p;
            if self.hand {
                if self.ramp_i < self.ramp_n {
                    self.ramp_i += 1;
                }
                // Critically damped spring: hand-to-vinyl coupling, and a last bit of smoothing.
                let h = self.hand_target();
                let w = self.omega;
                self.vel += (w * w * (h - self.p) - 2.0 * w * self.vel) * dt;
            } else {
                self.vel += (self.platter - self.vel) * (dt / SLIP_TAU);
            }
            self.p += self.vel * dt;
            let step;
            if self.rec.looped {
                step = self.p - p0;
                // Locked groove: wrap, and move the hand's frame with it so a held
                // record doesn't lurch a whole turn.
                let span = hi - lo;
                let shift = if self.p >= hi { -span } else if self.p < lo { span } else { 0.0 };
                if shift != 0.0 {
                    self.p += shift;
                    self.base += shift;
                    self.h_from += shift;
                    self.h_to += shift;
                }
            } else {
                if self.p < lo {
                    self.p = lo;
                    self.vel = self.vel.max(0.0);
                } else if self.p > hi {
                    self.p = hi;
                    self.vel = self.vel.min(0.0);
                }
                step = self.p - p0;
            }

            let stretch = step.abs().clamp(1.0, MAX_STRETCH);
            let (l, r) = self.sinc.read(&self.rec.l, &self.rec.r, self.p, stretch);
            let (mut yl, mut yr) = if self.rec.eq {
                // A magnetic cartridge outputs stylus velocity: groove signal x record speed
                // (signed), then the phono stage undoes RIAA at the *playback* frequency.
                let s = step / (rate * dt);
                (self.play[0].process(l as f64 * s), self.play[1].process(r as f64 * s))
            } else {
                (l as f64, r as f64)
            };
            yl = self.hpf[0].process(yl);
            yr = self.hpf[1].process(yr);

            if self.fader < self.fader_target {
                self.fader = (self.fader + fstep).min(self.fader_target);
            } else if self.fader > self.fader_target {
                self.fader = (self.fader - fstep).max(self.fader_target);
            }
            let g = self.fader * self.volume;
            let ol = soft_clip(yl as f32 * g);
            let or = soft_clip(yr as f32 * g);
            // The click is for your ears only; it never goes into the take.
            let mut click = 0.0;
            if self.click_env > 1e-4 {
                self.click_ph += self.click_hz * dt as f32;
                click = (self.click_ph * std::f32::consts::TAU).sin() * self.click_env;
                self.click_env *= click_decay;
            }
            // The backing track: under the record, not through its fader. It loops.
            let (mut bl, mut br) = (0.0, 0.0);
            if let (Some(b), true) = (&self.backing, self.backing_on) {
                let n = b.l.len();
                if n > 1 {
                    let i = self.backing_pos as usize % n;
                    let j = (i + 1) % n;
                    let f = self.backing_pos.fract() as f32;
                    bl = (b.l[i] + (b.l[j] - b.l[i]) * f) * self.backing_vol;
                    br = (b.r[i] + (b.r[j] - b.r[i]) * f) * self.backing_vol;
                    self.backing_pos += b.rate as f64 * dt;
                    if self.backing_pos >= n as f64 {
                        self.backing_pos -= n as f64;
                    }
                }
            }
            // Without a backing track the scratch passes untouched (it's already limited).
            let (ml, mr) = if self.backing_on && self.backing.is_some() { (soft_clip(ol + bl), soft_clip(or + br)) } else { (ol, or) };
            frame[0] = ml + click;
            if ch > 1 {
                frame[1] = mr + click;
            }
            for x in frame.iter_mut().skip(2) {
                *x = 0.0;
            }
            peak = peak.max(ml.abs()).max(mr.abs());
            if self.recording && self.take.slots() >= 2 {
                let (tl, tr) = if self.backing_in_take { (ml, mr) } else { (ol, or) };
                let _ = self.take.push(tl);
                let _ = self.take.push(tr);
                self.rec_frames += 1;
            }
        }

        self.shared.recording.store(self.recording, Relaxed);
        self.shared.rec_frames.store(self.rec_frames, Relaxed);
        self.shared.pos.store((self.p - self.rec.cue).to_bits(), Relaxed);
        self.shared.speed.store((self.vel / rate).to_bits(), Relaxed);
        let old = f32::from_bits(self.shared.level.load(Relaxed));
        self.shared.level.store(peak.max(old * 0.9).to_bits(), Relaxed);
    }
}

#[inline]
fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a < 0.8 {
        x
    } else {
        x.signum() * (0.8 + 0.2 * ((a - 0.8) / 0.2).tanh())
    }
}

/// Offline check: runs a scripted gesture (tablet-rate events) through the engine and
/// writes the result, reporting levels and how fast it renders versus real time.
pub fn render_test(input: Option<&str>, out_path: &str) -> anyhow::Result<()> {
    use crate::deck;
    let src = match input {
        Some("empty") => deck::Source::silent(),
        Some(p) => deck::load(std::path::Path::new(p))?,
        None => {
            // Synthetic "ahh": a few harmonics with a vowel-ish emphasis.
            let rate = 44100u32;
            let n = (rate as f32 * 0.6) as usize;
            let mut l = vec![0f32; n];
            for (i, v) in l.iter_mut().enumerate() {
                let t = i as f32 / rate as f32;
                let env = (t * 40.0).min(1.0) * (1.0 - t / 0.6).max(0.0);
                let mut s = 0.0;
                for h in 1..20 {
                    let f = 180.0 * h as f32;
                    let formant = (-(f - 750.0).powi(2) / 2.0e5).exp() + 0.5 * (-(f - 1200.0).powi(2) / 2.0e5).exp() + 0.1;
                    s += (2.0 * std::f32::consts::PI * f * t).sin() * formant / h as f32;
                }
                *v = s * env * 0.5;
            }
            deck::Source { r: l.clone(), l, rate, name: "synth".into() }
        }
    };
    let rec = Arc::new(deck::build(&src, 0, src.len(), true, true, false));
    let out_rate = 48000.0;
    let (mut tx, rx) = rtrb::RingBuffer::<Msg>::new(4096);
    let (trash_tx, _trash_rx) = rtrb::RingBuffer::new(16);
    let (take_tx, _take_rx) = rtrb::RingBuffer::new(16);
    let shared = Arc::new(Shared::default());
    let mut eng = Engine::new(rec.clone(), out_rate, rx, trash_tx, shared.clone(), take_tx);
    let _ = tx.push(Msg::Motor(true));

    let secs = 6.0;
    let total = (secs * out_rate) as usize;
    let block = 256;
    let mut out = vec![0f32; total * 2];
    let ev_dt = 1.0 / 200.0; // Intuos pen report rate
    let mut next_ev = 0.0;
    let mut hand = false;
    let src_rate = rec.rate;
    let started = std::time::Instant::now();
    let mut i = 0;
    while i < total {
        let t = i as f64 / out_rate;
        while next_ev <= t {
            let te = next_ev;
            let us = (te * 1e6) as u64;
            // 0-0.5 s: hand on record at cue. 0.5-3.5 s: baby scratches at ~3 Hz with
            // fader chirps from 3.5 s. 3.5-4 s: release (plays). 4.2-6 s: slow drags.
            let want_hand = (te < 3.5) || (4.2..6.0).contains(&te);
            if want_hand && !hand {
                let _ = tx.push(Msg::HandDown { t: us });
                hand = true;
            } else if !want_hand && hand {
                let _ = tx.push(Msg::HandUp);
                hand = false;
            }
            if hand {
                let u = if te < 0.5 {
                    0.0
                } else if te < 3.5 {
                    let ph = (te - 0.5) * 3.0 * std::f64::consts::TAU;
                    0.18 * src_rate * 0.5 * (1.0 - ph.cos())
                } else {
                    let ph = (te - 4.2) * 0.7 * std::f64::consts::TAU;
                    -0.25 * src_rate * ph.sin()
                };
                let _ = tx.push(Msg::HandMove { t: us, u });
            }
            if te >= 2.0 && te < 3.5 {
                let ph = ((te - 2.0) * 6.0).fract();
                let _ = tx.push(Msg::Fader(ph < 0.5));
            } else {
                let _ = tx.push(Msg::Fader(true));
            }
            next_ev += ev_dt;
        }
        let n = block.min(total - i);
        eng.process(&mut out[i * 2..(i + n) * 2], 2);
        i += n;
    }
    let took = started.elapsed().as_secs_f64();

    let nan = out.iter().any(|v| !v.is_finite());
    let peak = out.iter().fold(0f32, |a, v| a.max(v.abs()));
    let rms = (out.iter().map(|v| v * v).sum::<f32>() / out.len() as f32).sqrt();
    println!("rendered {secs}s in {:.3}s ({:.0}x realtime)  peak {:.3}  rms {:.4}  nan {}", took, secs / took, peak, rms, nan);

    let spec = hound::WavSpec { channels: 2, sample_rate: out_rate as u32, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(out_path, spec)?;
    for v in &out {
        w.write_sample(*v)?;
    }
    w.finalize()?;
    println!("wrote {out_path}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A silent record with the motor off, plus a backing track that's a constant 0.5.
    fn setup() -> (Engine, rtrb::Producer<Msg>, rtrb::Consumer<f32>, Arc<Shared>) {
        let src = Source::silent();
        let rec = Arc::new(crate::deck::build(&src, 0, src.len(), true, false, false));
        let (tx, rx) = rtrb::RingBuffer::new(64);
        let (trash_tx, _trash_rx) = rtrb::RingBuffer::new(8);
        let (take_tx, take_rx) = rtrb::RingBuffer::new(48000 * 2);
        let shared = Arc::new(Shared::default());
        let eng = Engine::new(rec, 48000.0, rx, trash_tx, shared.clone(), take_tx);
        (eng, tx, take_rx, shared)
    }

    fn backing() -> Arc<Source> {
        Arc::new(Source { l: vec![0.5; 48000], r: vec![0.5; 48000], rate: 48000, name: "beat".into() })
    }

    #[test]
    fn backing_is_heard_but_kept_out_of_takes_unless_asked() {
        let (mut eng, mut tx, mut take, shared) = setup();
        let _ = tx.push(Msg::Backing(Some(backing())));
        let _ = tx.push(Msg::BackingPlay(true));
        let _ = tx.push(Msg::BackingVolume(1.0));
        shared.rec_request.store(true, Relaxed);
        let mut out = vec![0.0f32; 512 * 2];
        eng.process(&mut out, 2);
        assert!(out.iter().all(|&x| (x - 0.5).abs() < 1e-3), "backing should be heard");
        assert!(shared.recording.load(Relaxed));
        let mut n = 0;
        while let Ok(x) = take.pop() {
            assert!(x.abs() < 1e-6, "take should be the (silent) scratch alone");
            n += 1;
        }
        assert_eq!(n, 512 * 2);

        let _ = tx.push(Msg::BackingInTake(true));
        eng.process(&mut out, 2);
        while let Ok(x) = take.pop() {
            assert!((x - 0.5).abs() < 1e-3, "with U on, the take is the full mix");
        }
    }

    #[test]
    fn backing_restarts_when_a_take_starts() {
        let (mut eng, mut tx, _take, shared) = setup();
        let _ = tx.push(Msg::Backing(Some(backing())));
        let _ = tx.push(Msg::BackingPlay(true));
        let mut out = vec![0.0f32; 1000 * 2];
        eng.process(&mut out, 2);
        assert!(eng.backing_pos > 900.0);
        shared.rec_request.store(true, Relaxed);
        let mut out = vec![0.0f32; 2];
        eng.process(&mut out, 2);
        assert!(eng.backing_pos < 2.0, "take start puts the backing track back to 0:00");
    }
}

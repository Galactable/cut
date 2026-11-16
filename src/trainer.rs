//! The trainer strip: your scratching drawn against the beat. Time runs right to left
//! past a fixed "now" line, height is where the record is in the sample, and the
//! upright lines are the beats of the backing track (or of the click).

use std::collections::VecDeque;

use rtrb::Consumer;
use winit::keyboard::KeyCode;

use crate::engine::{Trace, TRACE_HZ};
use crate::text::Fonts;
use crate::ui::{mix, Canvas, DIM, GREEN, LILAC, PANEL, WHITE};
use crate::{App, LABEL_TEXT};

/// How much history is kept, in seconds.
const KEEP_SECS: usize = 30;
/// Beats across the strip (when there's a beat to go by; otherwise it shows 5 seconds).
const BEATS_SHOWN: f64 = 8.0;
/// Where the now line sits, as a share of the width. Well to the right for now, so
/// most of the strip is what you just played.
const NOW_AT: f64 = 0.8;
/// The least the strip's height ever covers, in seconds of the sample.
const MIN_SPAN: f64 = 0.25;
/// How far the record has to come back before a change of direction counts.
const TURN: f32 = 0.006;
/// A jump this big between two samples is the record being moved (cue, loop), not played.
const JUMP: f32 = 0.25;

pub struct Trainer {
    pub on: bool,
    pub frozen: bool,
    rx: Consumer<Trace>,
    pts: VecDeque<Trace>,
    /// The part of the sample the strip's height covers, in seconds from the cue.
    range: (f64, f64),
}

impl Trainer {
    pub fn new(rx: Consumer<Trace>) -> Self {
        Trainer { on: false, frozen: false, rx, pts: VecDeque::new(), range: (-0.02, MIN_SPAN) }
    }

    /// Takes in what the engine logged. True when there's something new to draw.
    pub fn collect(&mut self) -> bool {
        let keep = self.on && !self.frozen;
        let mut any = false;
        while let Ok(p) = self.rx.pop() {
            if keep {
                self.pts.push_back(p);
                any = true;
            }
        }
        while self.pts.len() > KEEP_SECS * TRACE_HZ as usize {
            self.pts.pop_front();
        }
        any
    }
}

impl App {
    /// The trainer's keys. False when the key isn't one of them.
    pub fn trainer_command(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::F12 => {
                let t = &mut self.trainer;
                t.on = !t.on;
                t.frozen = false;
                t.pts.clear();
                self.say(if self.trainer.on { "trainer strip on: your scratching against the beat. \\ freezes it" } else { "trainer strip off" });
            }
            KeyCode::Backslash if self.trainer.on => {
                self.trainer.frozen = !self.trainer.frozen;
                self.say(if self.trainer.frozen { "strip frozen - \\ carries on" } else { "strip running" });
            }
            _ => return false,
        }
        true
    }

    pub fn draw_trainer(&mut self, c: &mut Canvas, f: &mut Fonts, (x, y, w, h): (i32, i32, i32, i32)) {
        let rate = self.out_rate as f64;
        // The beat to draw against: the backing track's while it plays, else the click's.
        let period = self.grid.filter(|_| self.backing_on).map(|g| g.period()).or(self.click.then(|| 60.0 / self.bpm));
        let pps = w as f64 / period.map_or(5.0, |p| p * BEATS_SHOWN);
        let now_x = x as f64 + w as f64 * NOW_AT;
        let (left, right) = (x as f64, (x + w) as f64);
        let t = &mut self.trainer;
        let Some(last) = t.pts.back().copied() else {
            f.draw(c, x + 8, y + 6, "waiting for the audio engine", LABEL_TEXT, 12.0, false);
            return;
        };
        let now = last.frame as f64 / rate;
        let to_x = |p: &Trace| now_x - (now - p.frame as f64 / rate) * pps;
        let oldest = now - (now_x - left) / pps;
        let start = t.pts.partition_point(|p| (p.frame as f64 / rate) < oldest);

        // Height: follows where you're working in the sample, gliding so it doesn't jump.
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for p in t.pts.iter().skip(start) {
            lo = lo.min(p.pos as f64);
            hi = hi.max(p.pos as f64);
        }
        let grow = (MIN_SPAN - (hi - lo)).max(0.0) / 2.0 + (hi - lo) * 0.1;
        let want = (lo - grow, hi + grow);
        let ease = if t.frozen { 0.0 } else { 0.12 };
        t.range = (t.range.0 + (want.0 - t.range.0) * ease, t.range.1 + (want.1 - t.range.1) * ease);
        let (lo, hi) = t.range;
        let (top, bottom) = ((y + 4) as f64, (y + h - 5) as f64);
        let to_y = |pos: f32| (bottom - (pos as f64 - lo) / (hi - lo) * (bottom - top)).clamp(top, bottom);

        // Beats: bars brightest, then beats, then the "and" between them.
        let shades = [mix(PANEL, DIM, 45), mix(PANEL, DIM, 130), mix(PANEL, WHITE, 150)];
        let beat_line = |c: &mut Canvas, at: f64, half: f64| {
            if at < left || at >= right {
                return;
            }
            let kind = if half as i64 % 2 != 0 { 0 } else if ((half / 2.0) as i64).rem_euclid(4) == 0 { 2 } else { 1 };
            c.rect(at as i32, y, if kind == 2 { 2 } else { 1 }, h, shades[kind]);
        };
        let mut prev: Option<&Trace> = None;
        for p in t.pts.iter().skip(start) {
            if let Some(a) = prev {
                let step = p.beat - a.beat;
                let (ha, hb) = ((a.beat * 2.0).floor(), (p.beat * 2.0).floor());
                if step > 0.0 && step < 0.5 && hb > ha {
                    let k = (hb / 2.0 - a.beat) / step;
                    beat_line(c, to_x(a) + (to_x(p) - to_x(a)) * k, hb);
                }
            }
            prev = Some(p);
        }
        if let (Some(per), true) = (period, last.beat.is_finite()) {
            // Ahead of now: where the coming beats will be.
            let mut half = (last.beat * 2.0).floor() + 1.0;
            loop {
                let at = now_x + (half / 2.0 - last.beat) * per * pps;
                if at >= right {
                    break;
                }
                beat_line(c, at, half);
                half += 1.0;
            }
        }
        if lo < 0.0 && hi > 0.0 {
            let cy = to_y(0.0) as i32;
            c.rect(x, cy, w, 1, mix(PANEL, GREEN, 60));
            f.draw(c, x + 6, cy - 15, "cue", mix(PANEL, GREEN, 150), 11.0, false);
        }

        // Your hand: green while you hold the record, lilac while it plays by itself,
        // grey while the fader is closed.
        let mut prev: Option<&Trace> = None;
        for p in t.pts.iter().skip(start) {
            if let Some(a) = prev.filter(|a| (p.pos - a.pos).abs() < JUMP) {
                let col = if !p.open() { DIM } else if p.hand() { GREEN } else { LILAC };
                c.line(to_x(a), to_y(a.pos), to_x(p), to_y(p.pos), 2, col);
            }
            prev = Some(p);
        }
        // A dot wherever you changed direction: these are what should land on the beat.
        let mut turn: Option<(&Trace, f32)> = None;
        for p in t.pts.iter().skip(start) {
            if !p.hand() {
                turn = None;
                continue;
            }
            let Some((e, dir)) = turn else {
                turn = Some((p, 0.0));
                continue;
            };
            let d = p.pos - e.pos;
            if d.abs() >= JUMP {
                turn = Some((p, 0.0));
            } else if dir == 0.0 {
                if d.abs() > TURN {
                    turn = Some((p, d.signum()));
                }
            } else if d * dir > 0.0 {
                turn = Some((p, dir));
            } else if -d * dir > TURN {
                c.dot(to_x(e), to_y(e.pos), 3.5, WHITE);
                turn = Some((p, -dir));
            }
        }

        c.rect(now_x as i32 - 1, y, 2, h, GREEN);
        c.dot(now_x, to_y(last.pos), 4.5, if last.hand() { GREEN } else { LILAC });
        let label = match (t.frozen, period) {
            (true, _) => "FROZEN - \\ carries on",
            (false, None) => "no beat to draw against: play the backing track (B) or turn on the click (T)",
            _ => "",
        };
        if !label.is_empty() {
            f.draw(c, x + 8, y + 6, label, if t.frozen { WHITE } else { LABEL_TEXT }, 12.0, t.frozen);
        }
    }
}

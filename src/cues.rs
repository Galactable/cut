//! Hot cues: three spots in the sample you can jump straight back to. Hold a hot cue
//! button for a second to put it where the needle is; tap it to jump there.

use std::time::{Duration, Instant};

use crate::{engine::Msg, App};

/// How many hot cues there are.
pub const N: usize = 3;
/// How long a button has to be held to set its cue.
pub const HOLD: Duration = Duration::from_millis(1000);
/// Marker colours, one per cue.
pub const COLORS: [u32; N] = [0xff9f1c, 0x2ec4ff, 0xff4fd8];

#[derive(Default)]
pub struct HotCues {
    /// Where each cue is, in samples of the loaded file.
    pub at: [Option<f64>; N],
    /// When each button went down, until it's released or its cue gets set.
    pub down: [Option<Instant>; N],
}

impl HotCues {
    /// How far through the hold each held button is (0-1), for drawing.
    pub fn progress(&self, n: usize) -> Option<f32> {
        self.down[n].map(|t| (t.elapsed().as_secs_f32() / HOLD.as_secs_f32()).min(1.0))
    }
}

impl App {
    /// A hot cue button went down: start timing the hold.
    pub fn cue_down(&mut self, n: usize) {
        if self.cues.down[n].is_none() {
            self.cues.down[n] = Some(Instant::now());
            self.dirty = true;
        }
    }

    /// A hot cue button came up: if it wasn't held long enough to set the cue, it was a
    /// tap, so jump. `act` false just forgets the press (released with a menu open).
    pub fn cue_up(&mut self, n: usize, act: bool) {
        if self.cues.down[n].take().is_none() {
            return;
        }
        self.dirty = true;
        if act {
            self.cue_jump(n);
        }
    }

    /// Let go of every held hot cue button without doing anything.
    pub fn cues_release_all(&mut self) {
        self.cues.down = [None; N];
        self.dirty = true;
    }

    /// Called every pass of the event loop: sets the cues held long enough.
    pub fn cue_tick(&mut self) {
        for n in 0..N {
            if self.cues.down[n].is_some_and(|t| t.elapsed() >= HOLD) {
                self.cues.down[n] = None;
                self.cue_set(n);
            }
        }
        if self.cues.down.iter().any(Option::is_some) {
            self.dirty = true;
        }
    }

    /// The needle's place in the loaded file, in its samples.
    fn needle_in_file(&self) -> f64 {
        self.region.0 as f64 + self.shared.pos() * self.src.rate as f64 / self.rec.rate
    }

    fn cue_set(&mut self, n: usize) {
        self.dirty = true;
        if self.src.name.is_empty() {
            self.say("load a sample first");
            return;
        }
        let s = self.needle_in_file();
        let (a, b) = (self.region.0 as f64, self.region.1 as f64);
        if s < a || s > b {
            self.say("the needle is outside the section - hot cues go inside it");
            return;
        }
        self.cues.at[n] = Some(s);
        self.say(format!("hot cue {} set at {:.3}s", n + 1, s / self.src.rate as f64));
    }

    pub fn cue_jump(&mut self, n: usize) {
        match self.cues.at[n] {
            Some(s) => {
                let p = self.rec.cue + (s - self.region.0 as f64) * self.rec.rate / self.src.rate as f64;
                self.send(Msg::Seek(p));
            }
            None => self.say(format!("hot cue {} isn't set - hold its button for a second to set it", n + 1)),
        }
    }

    /// The section changed: keep only the cues still inside it.
    pub fn cues_keep_in_section(&mut self) {
        let (a, b) = (self.region.0 as f64, self.region.1 as f64);
        for c in &mut self.cues.at {
            if c.is_some_and(|s| s < a || s > b) {
                *c = None;
            }
        }
    }

    /// A new file: no cues.
    pub fn cues_clear(&mut self) {
        self.cues = HotCues::default();
    }
}

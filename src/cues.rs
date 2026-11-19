//! Hot cues: three spots in the sample you can jump straight back to. Hold a hot cue
//! button for a second to put it where the needle is, keep holding to three seconds to
//! remove it; tap it to jump there.

use std::time::{Duration, Instant};

use crate::{engine::Msg, App};

/// How many hot cues there are.
pub const N: usize = 3;
/// How long a button has to be held to set its cue.
pub const HOLD: Duration = Duration::from_millis(1000);
/// How long a button has to be held to remove its cue.
pub const REMOVE: Duration = Duration::from_millis(3000);
/// Marker colours, one per cue.
pub const COLORS: [u32; N] = [0xff9f1c, 0x2ec4ff, 0xff4fd8];

#[derive(Default)]
pub struct HotCues {
    /// Where each cue is, in samples of the loaded file.
    pub at: [Option<f64>; N],
    /// When each button went down, until it's released or its cue gets removed.
    pub down: [Option<Instant>; N],
    /// The held button has already set its cue (past `HOLD`).
    pub set: [bool; N],
}

/// Where a held hot cue button is in its hold, for drawing.
pub enum Hold {
    /// Filling up towards setting the cue (0-1).
    Setting(f32),
    /// The cue is set; emptying towards removing it (1 = just set, 0 = removed).
    Removing(f32),
}

impl HotCues {
    pub fn progress(&self, n: usize) -> Option<Hold> {
        let t = self.down[n]?.elapsed();
        Some(if t < HOLD {
            Hold::Setting(t.as_secs_f32() / HOLD.as_secs_f32())
        } else {
            Hold::Removing(1.0 - ((t - HOLD).as_secs_f32() / (REMOVE - HOLD).as_secs_f32()).min(1.0))
        })
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
        // Past a second the hold set the cue, so letting go doesn't jump.
        let was_set = std::mem::take(&mut self.cues.set[n]);
        if act && !was_set {
            self.cue_jump(n);
        }
    }

    /// Let go of every held hot cue button without doing anything.
    pub fn cues_release_all(&mut self) {
        self.cues.down = [None; N];
        self.cues.set = [false; N];
        self.dirty = true;
    }

    /// Called every pass of the event loop: sets the cues held long enough, and removes
    /// the ones held longer still.
    pub fn cue_tick(&mut self) {
        for n in 0..N {
            let Some(t) = self.cues.down[n].map(|t| t.elapsed()) else { continue };
            if t >= REMOVE {
                // Let go of the button without a jump; the release is ignored.
                self.cues.down[n] = None;
                self.cues.set[n] = false;
                self.dirty = true;
                if self.cues.at[n].take().is_some() {
                    self.say(format!("hot cue {} removed", n + 1));
                }
            } else if t >= HOLD && !self.cues.set[n] {
                self.cues.set[n] = true;
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
        self.say(format!("hot cue {} set at {:.3}s - keep holding to remove it", n + 1, s / self.src.rate as f64));
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

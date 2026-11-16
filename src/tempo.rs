//! The backing track's beat grid in the app: found when the track loads, corrected by
//! hand when the detection is off, and remembered per track.

use std::time::Instant;

use winit::keyboard::KeyCode;

use crate::beat::{self, Grid};
use crate::engine::Msg;
use crate::App;

/// Taps further apart than this start a new count.
const TAP_GAP_SECS: f64 = 2.5;
const NUDGE_SECS: f64 = 0.005;

impl App {
    /// Where the backing track is, in seconds.
    fn backing_secs(&self) -> Option<f64> {
        let b = self.backing.as_ref()?;
        Some(self.shared.backing_pos() / b.rate as f64)
    }

    fn send_grid(&mut self) {
        let rate = self.backing.as_ref().map_or(0.0, |b| b.rate as f64);
        let g = self.grid.map(|g| (g.period() * rate, g.first * rate));
        self.send(Msg::Grid(g));
    }

    /// A newly loaded backing track: your own correction if there is one, otherwise
    /// what the detection found.
    pub fn grid_for_new_backing(&mut self, found: beat::Analysis) -> Option<Grid> {
        let saved = match (&self.backing_path, &self.backing) {
            (Some(p), Some(b)) => beat::saved::get(p, b.len()),
            _ => None,
        };
        self.grid = saved.or(found.grid);
        self.beat = Some(found);
        self.taps.clear();
        self.send_grid();
        self.grid
    }

    /// A correction made by hand: used from now on and remembered for this track.
    fn correct_grid(&mut self, g: Grid) {
        let g = Grid { bpm: g.bpm.clamp(30.0, 300.0), first: g.first }.tidy();
        self.grid = Some(g);
        self.send_grid();
        if let (Some(p), Some(b)) = (&self.backing_path, &self.backing) {
            beat::saved::set(p, b.len(), Some(g));
        }
    }

    /// The beat keys. False when the key isn't one of them.
    pub fn beat_command(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Slash => self.tap(),
            KeyCode::Digit1 => {
                let found = self.beat.as_ref().and_then(|a| a.grid);
                if let (Some(p), Some(b)) = (&self.backing_path, &self.backing) {
                    beat::saved::set(p, b.len(), None);
                }
                self.grid = found;
                self.send_grid();
                match (found, self.backing.is_some()) {
                    (Some(g), _) => self.say(format!("beat detected again: {:.1} BPM", g.bpm)),
                    (None, true) => self.say("no steady beat found - play the track and tap / on the beat, starting on beat one"),
                    (None, false) => self.say("load a backing track first (Y)"),
                }
            }
            KeyCode::Digit7 | KeyCode::Digit8 | KeyCode::Semicolon | KeyCode::Quote => {
                let Some(g) = self.grid else {
                    self.say("no beat grid yet - play the backing track and tap / on the beat");
                    return true;
                };
                let new = match code {
                    KeyCode::Digit7 => Grid { bpm: g.bpm / 2.0, ..g },
                    KeyCode::Digit8 => Grid { bpm: g.bpm * 2.0, ..g },
                    _ => {
                        let dir = if code == KeyCode::Quote { 1.0 } else { -1.0 };
                        // With Ctrl: a whole beat, which changes which beat is one.
                        Grid { first: g.first + dir * if self.ctrl { g.period() } else { NUDGE_SECS }, ..g }
                    }
                };
                self.correct_grid(new);
                match code {
                    KeyCode::Digit7 | KeyCode::Digit8 => self.say(format!("beat: {:.1} BPM", new.bpm.clamp(30.0, 300.0))),
                    _ if self.ctrl => self.say("beat one moved by a beat"),
                    KeyCode::Quote => self.say("beats 5 ms later"),
                    _ => self.say("beats 5 ms earlier"),
                }
            }
            // With a grid, the BPM keys fine-tune the track's tempo instead of the free click.
            KeyCode::Digit9 | KeyCode::Digit0 if self.grid.is_some() => {
                let g = self.grid.unwrap();
                let d = if code == KeyCode::Digit0 { 1.0 } else { -1.0 } * if self.ctrl { 1.0 } else { 0.1 };
                let bpm = ((g.bpm + d) * 10.0).round() / 10.0;
                let here = self.backing_secs().unwrap_or(g.first);
                self.correct_grid(g.retimed(bpm, here));
            }
            _ => return false,
        }
        true
    }

    /// Tap along with the track, starting on beat one. From the fourth tap on, the
    /// grid follows the taps.
    fn tap(&mut self) {
        let Some(t) = self.backing_secs().filter(|_| self.backing_on) else {
            self.say("play the backing track (B), then tap / on the beat, starting on beat one");
            return;
        };
        let now = Instant::now();
        let fresh = self.taps.last().is_none_or(|(last, at)| t < *last || now.duration_since(*at).as_secs_f64() > TAP_GAP_SECS);
        if fresh {
            self.taps.clear();
        }
        self.taps.push((t, now));
        let times: Vec<f64> = self.taps.iter().map(|(t, _)| *t).collect();
        match self.beat.as_ref().and_then(|a| a.from_taps(&times)) {
            Some(g) => {
                self.correct_grid(g);
                self.say(format!("tapped: {:.1} BPM", g.bpm));
            }
            None if times.len() < 4 => self.say(format!("tap {} of 4 - the first tap is beat one", times.len())),
            None => self.say("those taps were too uneven - start again on beat one"),
        }
    }
}

//! The Devices menu (X) and everything a set-up device does while you play.
//!
//! Any device CUT can read shows up here as it's plugged in. Picking one lets you learn
//! its controls: move what should scratch, then press what should cut the fader, run the
//! motor and so on. Those controls then run exactly what the keys do. Devices CUT can't
//! read yet can be given access (one device, by vendor, product and name) from here too.

use std::{
    collections::HashMap,
    sync::{atomic::Ordering::Relaxed, Arc},
    time::{Duration, Instant},
};

use winit::{
    event::KeyEvent,
    keyboard::{KeyCode, PhysicalKey},
};

use crate::{
    devices::{self, Action, Bound, Class, Ctl, DevInfo, DevProfile, Enc, Surface, ACTIONS, GUIDED},
    draw_panel,
    input::Kind,
    text,
    ui::*,
    App, Row, LABEL_TEXT, SCROLL_HOLD, SEC_PER_REV,
};

pub struct DevMenu {
    pub stage: Stage,
    pub sel: usize,
    pub rows: Vec<(i32, i32)>,
    pub panel: (i32, i32, i32, i32),
    /// List keyboards, mice, pens and system buttons too.
    pub show_all: bool,
    /// The device you just touched, picked out in the list.
    pub flash: Option<(Arc<str>, Instant)>,
}

pub enum Stage {
    List,
    Device(Arc<str>),
    Access(Arc<str>),
    Learn(Learn),
}

pub struct Learn {
    inst: Arc<str>,
    steps: Vec<Step>,
    at: usize,
    stats: HashMap<Ctl, Stats>,
    got: Vec<Bound>,
    turned: f64,
    /// Walking through every step (vs. one row).
    guided: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Scratch,
    Calibrate,
    Action(usize),
}

#[derive(Default)]
struct Stats {
    n: u32,
    min: i32,
    max: i32,
    last: Option<i32>,
    /// Same value as the one before: relative encoders do this, absolute ones don't.
    repeats: u32,
    /// Jumps of over half the range: a ring wrapping round.
    jumps: u32,
    /// Every value matches a relative MIDI encoding.
    rel_like: bool,
    near64: u32,
    /// Only ever 0 or 127: a button sending CCs.
    only_ends: bool,
}

/// A row on a device's page.
#[derive(Clone, Copy, PartialEq)]
enum DevRow {
    Guided,
    Scratch,
    Turn,
    Sens,
    Reverse,
    Act(usize),
    Grab,
    Surface,
    Reset,
    Forget,
}

/// How far an axis must travel before it counts as "moved" while learning.
const MOVED: f64 = 0.15;

fn norm(v: i32, (lo, hi): (i32, i32)) -> f64 {
    ((v - lo) as f64 / (hi - lo).max(1) as f64).clamp(0.0, 1.0)
}

/// Units of a control's movement since its last value.
fn delta(enc: Enc, v: i32, prev: Option<i32>, (lo, hi): (i32, i32)) -> f64 {
    match enc {
        Enc::Button => 0.0,
        Enc::Rel => v as f64,
        Enc::Rel64 => (v - 64) as f64,
        Enc::Rel2c => (if v < 64 { v } else { v - 128 }) as f64,
        Enc::Axis => prev.map_or(0.0, |p| (v - p) as f64),
        Enc::Wrap => {
            let Some(p) = prev else { return 0.0 };
            let span = hi - lo + 1;
            let mut d = v - p;
            if d > span / 2 {
                d -= span;
            } else if d < -span / 2 {
                d += span;
            }
            // A big jump is a finger lifted and put down elsewhere, not a turn.
            if d.abs() > span / 4 {
                0.0
            } else {
                d as f64
            }
        }
    }
}

/// Units a bound control moved. A tablet's touch ring reports its lowest value when the
/// finger lifts, so going to or from it is never a turn (positions are 1-71, 0 = lifted).
fn turned(b: Bound, v: i32, prev: Option<i32>, range: (i32, i32)) -> f64 {
    if b.enc == Enc::Wrap && matches!(b.ctl, Ctl::Abs(_)) {
        if v == range.0 || prev == Some(range.0) {
            return 0.0;
        }
        // The positions themselves start one above it.
        return delta(b.enc, v, prev, (range.0 + 1, range.1));
    }
    delta(b.enc, v, prev, range)
}

/// Control units in one turn of the record, when not calibrated.
fn default_per_turn(enc: Enc, (lo, hi): (i32, i32)) -> f64 {
    match enc {
        Enc::Rel => 24.0,
        Enc::Rel64 | Enc::Rel2c => 128.0,
        Enc::Wrap | Enc::Axis => (hi - lo + 1).max(2) as f64,
        Enc::Button => 1.0,
    }
}

/// Whether a control reads as held. `cut_in` is how far an axis has to move.
fn pressed(b: Bound, v: i32, range: (i32, i32), cut_in: f64) -> bool {
    match (b.enc, b.ctl) {
        (Enc::Button, Ctl::Cc(..)) => v >= 64,
        (Enc::Button, _) => v > 0,
        (Enc::Axis, _) => norm(v, range) >= cut_in,
        _ => false,
    }
}

impl Stats {
    fn add(&mut self, v: i32, (lo, hi): (i32, i32), midi: bool) {
        if self.n == 0 {
            self.min = v;
            self.max = v;
            self.rel_like = true;
            self.only_ends = true;
        }
        if v != 0 && v != 127 {
            self.only_ends = false;
        }
        self.n += 1;
        self.min = self.min.min(v);
        self.max = self.max.max(v);
        if let Some(p) = self.last {
            if p == v {
                self.repeats += 1;
            }
            if (v - p).abs() > (hi - lo) / 2 {
                self.jumps += 1;
            }
        }
        self.last = Some(v);
        let near_edges = (1..=24).contains(&v) || (104..=127).contains(&v);
        let near_mid = (40..=88).contains(&v);
        if !(midi && (near_edges || near_mid)) {
            self.rel_like = false;
        }
        if near_mid {
            self.near64 += 1;
        }
    }

    fn relative(&self) -> bool {
        self.rel_like && self.n >= 6 && self.repeats * 10 >= self.n * 3
    }

    fn span(&self, range: (i32, i32)) -> f64 {
        (self.max - self.min) as f64 / (range.1 - range.0).max(1) as f64
    }

    /// How a control was moving.
    fn enc(&self, ctl: Ctl, range: (i32, i32)) -> Enc {
        match ctl {
            Ctl::Key(_) | Ctl::Note(..) => Enc::Button,
            Ctl::Rel(_) => Enc::Rel,
            Ctl::Cc(..) if self.relative() => {
                if self.near64 * 2 >= self.n {
                    Enc::Rel64
                } else {
                    Enc::Rel2c
                }
            }
            Ctl::Cc(..) if self.only_ends => Enc::Button,
            _ if self.jumps > 0 && self.span(range) > 0.5 => Enc::Wrap,
            _ => Enc::Axis,
        }
    }

    /// Moved enough to be what you meant (for a wheel, an axis or a knob).
    fn moved(&self, ctl: Ctl, range: (i32, i32)) -> bool {
        match ctl {
            Ctl::Key(_) | Ctl::Note(..) => false,
            Ctl::Rel(_) => self.n >= 3,
            _ => self.relative() || self.span(range) >= MOVED,
        }
    }
}

impl Learn {
    fn step(&self) -> Option<Step> {
        self.steps.get(self.at).copied()
    }

    /// The control that moved most, for the scratch step.
    fn best(&self, info: &DevInfo) -> Option<Bound> {
        self.stats
            .iter()
            .filter(|(c, s)| s.moved(**c, info.range(**c)))
            .max_by_key(|(_, s)| s.n)
            .map(|(c, s)| Bound { ctl: *c, enc: s.enc(*c, info.range(*c)) })
            .filter(|b| b.enc != Enc::Button)
    }
}

impl App {
    pub fn overlay_open(&self) -> bool {
        self.menu.is_some() || self.folders.is_some() || self.dev_menu.is_some() || self.whats_new.is_some()
    }

    fn dev_profile(&self, inst: &str) -> Option<&DevProfile> {
        self.devs.get(inst).and_then(|i| self.dev_profiles.get(&i.id))
    }

    fn save_dev(&mut self, p: DevProfile) {
        let _ = std::fs::create_dir_all(devices::dir());
        let _ = std::fs::write(p.path(), p.text());
        self.dev_profiles.insert(p.id.clone(), p);
        self.sync_devices();
    }

    fn edit_dev(&mut self, inst: &str, f: impl FnOnce(&mut DevProfile)) {
        let Some(info) = self.devs.get(inst) else { return };
        let mut p = self.dev_profiles.get(&info.id).cloned().unwrap_or_else(|| DevProfile::new(&info.id, &info.name));
        f(&mut p);
        self.save_dev(p);
    }

    /// How a touch surface behaves: its setup, or what it looks like.
    pub fn touch_kind(&self, inst: &str, kind: Kind) -> Kind {
        self.dev_profile(inst).and_then(|p| p.surface).map_or(kind, Surface::kind)
    }

    /// Tells each device's reader whether to take it from the desktop and whether to
    /// pass its controls on.
    pub fn sync_devices(&mut self) {
        let learning = match &self.dev_menu {
            Some(DevMenu { stage: Stage::Learn(l), .. }) => Some(l.inst.clone()),
            _ => None,
        };
        let menu = self.dev_menu.is_some();
        for info in self.devs.values() {
            let p = self.dev_profiles.get(&info.id);
            let used = p.is_some_and(|p| p.scratch.is_some() || !p.binds.is_empty());
            let learn = learning.as_deref() == Some(&*info.inst);
            let grab = match info.class {
                Class::Touch(k) => self.focused && p.and_then(|p| p.surface).map_or(k, Surface::kind) == Kind::TabletTouch,
                Class::Midi => false,
                _ => self.focused && (learn || (used && p.is_some_and(|p| p.grab))),
            };
            info.grab.store(grab, Relaxed);
            // With the menu open everything is listened to (to find devices), except
            // what the list doesn't show: a hovering pen would redraw it nonstop.
            let shown = info.class.usual() || self.dev_menu.as_ref().is_some_and(|m| m.show_all);
            info.forward.store(used || (menu && shown), Relaxed);
        }
    }

    pub fn device_added(&mut self, info: DevInfo) {
        let id = info.id.clone();
        // Setups learned before not_a_control existed may hold such a signal: drop it.
        if let Some(p) = self.dev_profiles.get(&id) {
            if p.binds.iter().any(|(_, b)| not_a_control(b.ctl, &info)) || p.scratch.is_some_and(|b| not_a_control(b.ctl, &info)) {
                let mut p = p.clone();
                p.binds.retain(|(_, b)| !not_a_control(b.ctl, &info));
                if p.scratch.is_some_and(|b| not_a_control(b.ctl, &info)) {
                    p.scratch = None;
                }
                self.save_dev(p);
            }
        }
        let fresh = !self.devs.contains_key(&info.inst);
        if !self.dev_profiles.contains_key(&id) {
            if let Some(p) = devices::builtin(&id, &info.name) {
                self.save_dev(p);
            }
        }
        let nice = info.readable && info.class.usual() && !self.dev_profiles.contains_key(&id) && !matches!(info.class, Class::Touch(_));
        let name = info.name.clone();
        let readable = info.readable;
        self.devs.insert(info.inst.clone(), info);
        self.sync_devices();
        // Startup finds everything at once; only point out what's plugged in later.
        if fresh && self.started.elapsed() > Duration::from_secs(3) {
            if nice {
                self.say(format!("new device: {name} - press X to set it up"));
            } else if readable && self.dev_profiles.contains_key(&id) {
                self.say(format!("{name} ready"));
            }
        }
    }

    pub fn device_gone(&mut self, inst: Arc<str>) {
        self.devs.remove(&inst);
        self.dev_last.retain(|(d, _), _| *d != inst);
        self.dev_pressed.retain(|(d, _)| *d != inst);
        self.dev_fader.retain(|(d, _)| *d != inst);
        self.dev_hold.retain(|(d, _)| *d != inst);
        self.update_fader();
        self.update_hand();
        if let Some(m) = self.dev_menu.as_mut() {
            let on_it = match &m.stage {
                Stage::Device(i) | Stage::Access(i) => *i == inst,
                Stage::Learn(l) => l.inst == inst,
                Stage::List => false,
            };
            if on_it {
                m.stage = Stage::List;
                m.sel = 0;
                self.sync_devices();
            }
        }
    }

    /// A control moved on a device: learn it, or do what it's set up to do.
    pub fn device_control(&mut self, inst: Arc<str>, ctl: Ctl, value: i32, t: u64) -> bool {
        let Some(info) = self.devs.get(&inst) else { return false };
        let range = info.range(ctl);
        let key = (inst.clone(), ctl);
        let prev = self.dev_last.insert(key.clone(), value);

        if let Some(m) = self.dev_menu.as_mut() {
            match &mut m.stage {
                Stage::Learn(l) if l.inst == inst => {
                    let info = self.devs.get(&inst).unwrap();
                    learn_event(l, info, self.dev_profiles.get(&info.id), ctl, value, prev, range);
                    return true;
                }
                Stage::List => {
                    // Touch something to find it in the list.
                    let real = ctl.is_button() && value > 0 || !ctl.is_button() && prev.is_some_and(|p| p != value);
                    return real && self.dev_flash(&inst);
                }
                _ => {}
            }
        }

        let Some(p) = self.dev_profile(&inst) else { return false };
        let scratch = p.scratch.filter(|b| b.ctl == ctl);
        let (per_turn, sens, rev) = (p.per_turn, p.sens, p.reverse);
        let hits: Vec<(usize, Bound)> = p.binds.iter().filter(|(_, b)| b.ctl == ctl).copied().collect();
        // Releases always go through, so nothing stays held; the rest only while focused
        // and no menu is open.
        let live = self.focused && !self.overlay_open() && self.bar.open.is_none();
        let mut did = false;

        if let Some(b) = scratch {
            let d = turned(b, value, prev, range);
            if d != 0.0 && live {
                let turns = d / per_turn.unwrap_or(default_per_turn(b.enc, range)) * sens * if rev { -1.0 } else { 1.0 };
                self.sens_kind = Kind::Controller;
                self.jog_last = Some(Instant::now());
                self.update_hand_at(t);
                let du = turns * SEC_PER_REV * self.rec.rate * self.sens[Kind::Controller as usize];
                self.move_hand_at(du, t);
                did = true;
            }
        }

        let was = self.dev_pressed.contains(&key);
        for (ai, b) in hits {
            match ACTIONS[ai].2 {
                Action::Fader => {
                    let on = pressed(b, value, range, 0.12) && (live || self.dev_fader.contains(&key));
                    if on {
                        self.dev_fader.insert(key.clone());
                    } else {
                        self.dev_fader.remove(&key);
                    }
                    self.update_fader();
                }
                Action::Hold => {
                    let on = pressed(b, value, range, 0.5) && (live || self.dev_hold.contains(&key));
                    if on {
                        self.dev_hold.insert(key.clone());
                    } else {
                        self.dev_hold.remove(&key);
                    }
                    if on {
                        self.sens_kind = Kind::Controller;
                    }
                    self.update_hand_at(t);
                }
                Action::Volume if live => match b.enc {
                    Enc::Axis => {
                        // The same 0-150% as the keys.
                        self.volume = (norm(value, range) * 1.5) as f32;
                        self.send(crate::engine::Msg::Volume(self.volume));
                        self.save_settings();
                    }
                    Enc::Button => {}
                    enc => {
                        // 20 steps of 5% to a full turn.
                        let step = default_per_turn(enc, range) / 20.0;
                        let acc = self.vol_acc.entry(key.clone()).or_insert(0.0);
                        *acc += turned(b, value, prev, range);
                        let n = (*acc / step).trunc();
                        *acc -= n * step;
                        let code = if n > 0.0 { KeyCode::ArrowUp } else { KeyCode::ArrowDown };
                        for _ in 0..(n.abs() as usize).min(20) {
                            self.command(code);
                        }
                    }
                },
                Action::Cmd(code) => {
                    if pressed(b, value, range, 0.5) && !was && live {
                        self.command(code);
                    }
                }
                _ => {}
            }
            did = true;
        }
        if pressed(Bound { ctl, enc: Enc::default_for(ctl) }, value, range, 0.5) {
            self.dev_pressed.insert(key);
        } else {
            self.dev_pressed.remove(&key);
        }
        did
    }

    /// Something on this device was touched while the list is open: pick it out.
    pub fn dev_flash(&mut self, inst: &Arc<str>) -> bool {
        if !matches!(self.dev_menu, Some(DevMenu { stage: Stage::List, .. })) {
            return false;
        }
        let list = self.dev_list();
        if let Some(m) = self.dev_menu.as_mut() {
            m.flash = Some((inst.clone(), Instant::now()));
            if let Some(i) = list.iter().position(|d| d == inst) {
                m.sel = i;
            }
        }
        true
    }

    pub fn jog_active(&self) -> bool {
        self.jog_last.is_some_and(|t| t.elapsed() < SCROLL_HOLD)
    }

    /// Devices on the list, in order.
    fn dev_list(&self) -> Vec<Arc<str>> {
        let show_all = self.dev_menu.as_ref().is_some_and(|m| m.show_all);
        let order = |c: Class| match c {
            Class::Midi => 0,
            Class::Controls => 1,
            Class::Touch(_) => 2,
            Class::Pen => 3,
            Class::Keyboard => 4,
            Class::Mouse => 5,
            Class::Other => 6,
        };
        let mut v: Vec<&DevInfo> = self.devs.values().filter(|d| show_all || d.class.usual() || self.dev_profiles.contains_key(&d.id)).collect();
        v.sort_by_key(|d| (order(d.class), !d.readable, d.name.to_lowercase(), d.inst.to_string()));
        v.into_iter().map(|d| d.inst.clone()).collect()
    }

    fn device_rows(&self, inst: &str) -> Vec<DevRow> {
        let Some(info) = self.devs.get(inst) else { return Vec::new() };
        let mut rows = Vec::new();
        if let Class::Touch(_) = info.class {
            rows.push(DevRow::Surface);
        } else {
            rows.extend([DevRow::Guided, DevRow::Scratch, DevRow::Turn, DevRow::Sens, DevRow::Reverse]);
            rows.extend((0..ACTIONS.len()).map(DevRow::Act));
            if !info.is_midi() {
                rows.push(DevRow::Grab);
            }
        }
        if devices::builtin(&info.id, &info.name).is_some() {
            rows.push(DevRow::Reset);
        }
        if self.dev_profiles.contains_key(&info.id) {
            rows.push(DevRow::Forget);
        }
        rows
    }

    pub fn open_dev_menu(&mut self) {
        self.dev_menu = Some(DevMenu { stage: Stage::List, sel: 0, rows: Vec::new(), panel: (0, 0, 0, 0), show_all: false, flash: None });
        self.sync_devices();
    }

    fn close_dev_menu(&mut self) {
        self.dev_menu = None;
        self.sync_devices();
    }

    fn start_learn(&mut self, inst: Arc<str>, steps: Vec<Step>, guided: bool) {
        if let Some(m) = self.dev_menu.as_mut() {
            m.stage = Stage::Learn(Learn { inst, steps, at: 0, stats: HashMap::new(), got: Vec::new(), turned: 0.0, guided });
            m.sel = 0;
        }
        self.sync_devices();
    }

    /// Enter while learning: keep what was heard for this step and go on.
    fn learn_next(&mut self) {
        let Some(DevMenu { stage: Stage::Learn(l), .. }) = self.dev_menu.as_mut() else { return };
        let inst = l.inst.clone();
        let step = l.step();
        let info = self.devs.get(&inst).cloned();
        let best = info.as_ref().and_then(|i| l.best(i));
        let (got, turned) = (std::mem::take(&mut l.got), l.turned);
        l.at += 1;
        l.stats.clear();
        l.turned = 0.0;
        let done = l.at >= l.steps.len();
        let had_scratch = self.dev_profile(&inst).is_some_and(|p| p.scratch.is_some());
        match step {
            Some(Step::Scratch) => {
                if let Some(b) = best {
                    self.edit_dev(&inst, |p| {
                        p.scratch = Some(b);
                        p.per_turn = None;
                    });
                }
            }
            Some(Step::Calibrate) => {
                if turned >= 4.0 {
                    self.edit_dev(&inst, |p| p.per_turn = Some(turned.round()));
                }
            }
            Some(Step::Action(a)) => {
                if !got.is_empty() {
                    self.edit_dev(&inst, |p| {
                        p.binds.retain(|(i, _)| *i != a);
                        // A control does one thing: take it off anything else it was on.
                        p.binds.retain(|(_, b)| !got.iter().any(|g| g.ctl == b.ctl));
                        if p.scratch.is_some_and(|s| got.iter().any(|g| g.ctl == s.ctl)) {
                            p.scratch = None;
                        }
                        p.binds.extend(got.iter().map(|b| (a, *b)));
                    });
                }
            }
            None => {}
        }
        // Calibrating only makes sense for something that turns.
        let turning = self.dev_profile(&inst).and_then(|p| p.scratch).is_some_and(|s| s.enc.is_turning());
        if let Some(DevMenu { stage: Stage::Learn(l), .. }) = self.dev_menu.as_mut() {
            if l.step() == Some(Step::Calibrate) && !turning {
                l.at += 1;
            }
            if l.at >= l.steps.len() || done {
                let guided = l.guided;
                self.dev_back_to_device(inst.clone());
                if guided {
                    let name = self.devs.get(&inst).map(|i| i.name.clone()).unwrap_or_default();
                    self.say(format!("{name} is set up"));
                } else if !had_scratch && turning {
                    self.say("scratch control set - turn calibration is on the next row");
                }
            }
        }
    }

    fn dev_back_to_device(&mut self, inst: Arc<str>) {
        if let Some(m) = self.dev_menu.as_mut() {
            m.stage = Stage::Device(inst);
            m.sel = 0;
        }
        self.sync_devices();
    }

    fn dev_activate(&mut self) {
        let Some(m) = self.dev_menu.as_mut() else { return };
        match &m.stage {
            Stage::List => {
                let list = self.dev_list();
                let Some(m) = self.dev_menu.as_mut() else { return };
                if m.sel == list.len() {
                    m.show_all = !m.show_all;
                    self.sync_devices();
                    return;
                }
                let Some(inst) = list.get(m.sel).cloned() else { return };
                let readable = self.devs.get(&inst).is_some_and(|i| i.readable);
                m.stage = if readable { Stage::Device(inst) } else { Stage::Access(inst) };
                m.sel = 0;
            }
            Stage::Access(inst) => {
                let inst = inst.clone();
                if m.sel == 0 {
                    if let Some(info) = self.devs.get(&inst) {
                        self.grant(info.clone());
                    }
                }
                if let Some(m) = self.dev_menu.as_mut() {
                    m.stage = Stage::List;
                    m.sel = 0;
                }
            }
            Stage::Device(inst) => {
                let inst = inst.clone();
                let sel = m.sel;
                let Some(&row) = self.device_rows(&inst).get(sel) else { return };
                match row {
                    DevRow::Guided => {
                        let mut steps = vec![Step::Scratch, Step::Calibrate];
                        steps.extend(GUIDED.iter().map(|&a| Step::Action(devices::action(a))));
                        self.start_learn(inst, steps, true);
                    }
                    DevRow::Scratch => self.start_learn(inst, vec![Step::Scratch, Step::Calibrate], false),
                    DevRow::Turn => {
                        if self.dev_profile(&inst).and_then(|p| p.scratch).is_some_and(|s| s.enc.is_turning()) {
                            self.start_learn(inst, vec![Step::Calibrate], false);
                        } else {
                            self.say("set a scratch control that turns first (a jog, dial or ring)");
                        }
                    }
                    DevRow::Sens => self.dev_sens(&inst, 1),
                    DevRow::Reverse => self.edit_dev(&inst, |p| p.reverse = !p.reverse),
                    DevRow::Act(a) => self.start_learn(inst, vec![Step::Action(a)], false),
                    DevRow::Grab => self.edit_dev(&inst, |p| p.grab = !p.grab),
                    DevRow::Surface => {
                        let k = self.devs.get(&inst).map(|i| match i.class {
                            Class::Touch(k) => k,
                            _ => Kind::Touchpad,
                        });
                        let now = self.touch_kind(&inst, k.unwrap_or(Kind::Touchpad));
                        let next = if now == Kind::TabletTouch { Surface::Touchpad } else { Surface::Tablet };
                        self.edit_dev(&inst, |p| p.surface = Some(next));
                    }
                    DevRow::Reset => {
                        if let Some(info) = self.devs.get(&inst) {
                            if let Some(p) = devices::builtin(&info.id, &info.name) {
                                self.save_dev(p);
                                self.say("back to the built-in setup");
                            }
                        }
                    }
                    DevRow::Forget => {
                        if let Some(info) = self.devs.get(&inst) {
                            let id = info.id.clone();
                            if let Some(p) = self.dev_profiles.remove(&id) {
                                let _ = std::fs::remove_file(p.path());
                            }
                            self.sync_devices();
                            self.say("setup forgotten");
                        }
                        if let Some(m) = self.dev_menu.as_mut() {
                            m.sel = 0;
                        }
                    }
                }
            }
            Stage::Learn(_) => self.learn_next(),
        }
    }

    fn dev_sens(&mut self, inst: &str, dir: i32) {
        self.edit_dev(inst, |p| {
            p.sens = if dir > 0 { (p.sens * 1.15).min(20.0) } else { (p.sens / 1.15).max(0.05) };
        });
    }

    /// Asks (through polkit) for a udev rule that lets you read this one device.
    fn grant(&mut self, info: DevInfo) {
        let Ok(exe) = std::env::current_exe() else { return };
        let status = self.take_status.clone();
        let (v, p) = (format!("{:04x}", info.vendor), format!("{:04x}", info.product));
        let name = info.name.clone();
        self.say("asking for your password...");
        std::thread::spawn(move || {
            let ok = std::process::Command::new("pkexec").arg(&exe).args(["--grant-access", &v, &p, &name]).status().is_ok_and(|s| s.success());
            *status.lock().unwrap() = Some(if ok {
                format!("{name}: access given - it will show up in a moment")
            } else {
                format!("no access given. In a terminal: sudo {} --grant-access {v} {p} \"{name}\"", exe.display())
            });
        });
    }

    pub fn dev_key(&mut self, ev: &KeyEvent) {
        let PhysicalKey::Code(code) = ev.physical_key else { return };
        let Some(m) = self.dev_menu.as_ref() else { return };
        let sel = m.sel;
        // The row under the selection on a device's page, and that device.
        let (rows, page) = match &m.stage {
            Stage::List => (self.dev_list().len() + 1, None),
            Stage::Access(_) => (2, None),
            Stage::Device(i) => {
                let r = self.device_rows(i);
                (r.len(), Some((i.clone(), r.get(sel).copied())))
            }
            Stage::Learn(_) => (1, None),
        };
        let rows = rows.max(1);
        let Some(m) = self.dev_menu.as_mut() else { return };
        match (&m.stage, code) {
            (Stage::Learn(l), KeyCode::Escape) => {
                let inst = l.inst.clone();
                self.dev_back_to_device(inst);
            }
            (Stage::Learn(l), KeyCode::Backspace | KeyCode::Delete) => {
                let (inst, step) = (l.inst.clone(), l.step());
                if let Stage::Learn(l) = &mut m.stage {
                    l.got.clear();
                    l.stats.clear();
                }
                match step {
                    Some(Step::Action(a)) => self.edit_dev(&inst, |p| p.binds.retain(|(i, _)| *i != a)),
                    Some(Step::Scratch) => self.edit_dev(&inst, |p| p.scratch = None),
                    Some(Step::Calibrate) => self.edit_dev(&inst, |p| p.per_turn = None),
                    None => {}
                }
            }
            (Stage::Learn(_), KeyCode::Enter | KeyCode::NumpadEnter) => self.learn_next(),
            (Stage::Learn(l), KeyCode::Tab) => {
                // Skip: forget what was heard for this step, then move on.
                let _ = l;
                if let Stage::Learn(l) = &mut m.stage {
                    l.got.clear();
                    l.stats.clear();
                    l.turned = 0.0;
                }
                self.learn_next();
            }
            (Stage::Learn(_), _) => {}
            (Stage::List, KeyCode::Escape | KeyCode::KeyX) => self.close_dev_menu(),
            (Stage::Device(_) | Stage::Access(_), KeyCode::Escape | KeyCode::Backspace) => {
                m.stage = Stage::List;
                m.sel = 0;
            }
            (Stage::Device(_), KeyCode::ArrowLeft | KeyCode::ArrowRight) => {
                if let Some((i, Some(DevRow::Sens))) = page {
                    self.dev_sens(&i, if code == KeyCode::ArrowRight { 1 } else { -1 });
                }
            }
            (Stage::Device(_), KeyCode::Delete) => {
                if let Some((i, Some(row))) = page {
                    match row {
                        DevRow::Act(a) => self.edit_dev(&i, |p| p.binds.retain(|(x, _)| *x != a)),
                        DevRow::Scratch => self.edit_dev(&i, |p| p.scratch = None),
                        DevRow::Turn => self.edit_dev(&i, |p| p.per_turn = None),
                        DevRow::Sens => self.edit_dev(&i, |p| p.sens = 1.0),
                        _ => {}
                    }
                }
            }
            (_, KeyCode::ArrowUp) => m.sel = (m.sel + rows - 1) % rows,
            (_, KeyCode::ArrowDown) => m.sel = (m.sel + 1) % rows,
            (_, KeyCode::Enter | KeyCode::NumpadEnter) => self.dev_activate(),
            _ => {}
        }
    }

    pub fn dev_click(&mut self) {
        let Some(m) = &self.dev_menu else { return };
        let (px, py, pw, ph) = m.panel;
        let (x, y) = self.cursor;
        let inside = x >= px as f64 && x < (px + pw) as f64 && y >= py as f64 && y < (py + ph) as f64;
        if !inside {
            if let Stage::Learn(_) = m.stage {
                return;
            }
            self.close_dev_menu();
            return;
        }
        if let Stage::Learn(_) = m.stage {
            return;
        }
        if let Some(r) = m.rows.iter().position(|&(a, b)| y >= a as f64 && y < b as f64) {
            if let Some(m) = self.dev_menu.as_mut() {
                m.sel = r;
            }
            self.dev_activate();
        }
    }

    pub fn draw_devices(&self, c: &mut Canvas, f: &mut text::Fonts, m: &mut DevMenu) {
        let mut sel = m.sel;
        let (title, sub, rows, hints): (String, Option<(String, u32)>, Vec<Row>, &[(&str, &str)]) = match &m.stage {
            Stage::List => {
                let list = self.dev_list();
                let mut rows: Vec<Row> = list
                    .iter()
                    .filter_map(|i| self.devs.get(i))
                    .map(|d| {
                        let p = self.dev_profiles.get(&d.id).filter(|p| !p.is_empty());
                        let kind = match d.class {
                            Class::Midi => "\u{f001}",
                            Class::Touch(_) => "\u{f0a6}",
                            Class::Keyboard => "\u{f11c}",
                            Class::Mouse => "\u{f245}",
                            Class::Pen => "\u{f040}",
                            _ => "\u{f11b}",
                        };
                        let flashing = m.flash.as_ref().is_some_and(|(f, t)| *f == d.inst && t.elapsed() < Duration::from_millis(900));
                        let row = Row::new(&format!("{kind}  {}", d.name), if d.readable { TEXT } else { DIM }, flashing);
                        if flashing {
                            row.tag("\u{f0a7} that's this one", GREEN)
                        } else if !d.readable {
                            row.tag("needs access", RED)
                        } else if let Class::Touch(k) = d.class {
                            row.tag(if self.touch_kind(&d.inst, k) == Kind::TabletTouch { "touch: tablet" } else { "touch: touchpad" }, LILAC)
                        } else if let Some(p) = p {
                            let n = p.binds.len();
                            let s = match (p.scratch.is_some(), n) {
                                (true, 0) => "scratch".to_string(),
                                (true, n) => format!("scratch + {n} control{}", if n == 1 { "" } else { "s" }),
                                (false, n) => format!("{n} control{}", if n == 1 { "" } else { "s" }),
                            };
                            row.tag(&format!("\u{f00c} {s}"), GREEN)
                        } else {
                            row.tag("not set up", LABEL_TEXT)
                        }
                    })
                    .collect();
                if rows.is_empty() {
                    rows.push(Row::new("no devices found", DIM, false));
                }
                rows.push(Row::new(if m.show_all { "Show only likely devices" } else { "Show all devices (keyboards, mice, pens...)" }, LABEL_TEXT, false));
                (
                    "\u{f11b}  DEVICES".to_string(),
                    Some(("touch a control to find its device".to_string(), DIM)),
                    rows,
                    &[("\u{2191}\u{2193}", "choose"), ("Enter", "set up"), ("Esc", "close")],
                )
            }
            Stage::Access(i) => {
                let d = self.devs.get(i);
                let name = d.map(|d| d.name.clone()).unwrap_or_default();
                let warn = d.is_some_and(|d| matches!(d.class, Class::Keyboard | Class::Mouse));
                let mut rows = vec![Row::new("Give CUT access to this device (asks for your password)", GREEN, true), Row::new("Cancel", TEXT, false)];
                if warn {
                    rows.push(Row::new("\u{f071} It's a keyboard or mouse: any app you run could read it too.", RED, false));
                }
                (format!("\u{f023}  {name}"), Some(("only this device, by its id and name".to_string(), DIM)), rows, &[("Enter", "choose"), ("Esc", "back")])
            }
            Stage::Device(i) => {
                let d = self.devs.get(i);
                let name = d.map(|d| d.name.clone()).unwrap_or_default();
                let p = d.and_then(|d| self.dev_profiles.get(&d.id));
                let range = |b: &Bound| d.map_or((0, 127), |d| d.range(b.ctl));
                let rows = self
                    .device_rows(i)
                    .into_iter()
                    .map(|r| match r {
                        DevRow::Guided => Row::new("\u{f0e7}  Guided setup (scratch, fader, motor, cue...)", GREEN, true),
                        DevRow::Scratch => {
                            let row = Row::new("Scratch with", TEXT, false);
                            match p.and_then(|p| p.scratch) {
                                Some(b) => row.tag(&b.label(), GREEN),
                                None => row.tag("not set", DIM),
                            }
                        }
                        DevRow::Turn => {
                            let row = Row::new("One record turn", TEXT, false);
                            match p.and_then(|p| p.scratch.map(|s| (s, p.per_turn))) {
                                Some((s, Some(n))) if s.enc.is_turning() => row.tag(&format!("{n:.0} steps (calibrated)"), GREEN),
                                Some((s, None)) if s.enc.is_turning() => row.tag(&format!("{:.0} steps (a guess - Enter to calibrate)", default_per_turn(s.enc, range(&s))), WHITE),
                                Some(_) => row.tag("full travel", LABEL_TEXT),
                                None => row.tag("-", DIM),
                            }
                        }
                        DevRow::Sens => Row::new("Sensitivity", TEXT, false).tag(&format!("\u{2190} x{:.2} \u{2192}", p.map_or(1.0, |p| p.sens)), WHITE),
                        DevRow::Reverse => Row::new("Scratch direction", TEXT, false).tag(if p.is_some_and(|p| p.reverse) { "reversed" } else { "normal" }, WHITE),
                        DevRow::Act(a) => {
                            let row = Row::new(ACTIONS[a].1, TEXT, false);
                            let list: Vec<String> = p.map(|p| p.binds_for(a).map(Bound::label).collect()).unwrap_or_default();
                            if list.is_empty() {
                                row.tag("-", DIM)
                            } else {
                                row.tag(&list.join(", "), GREEN)
                            }
                        }
                        DevRow::Grab => Row::new("Take it from the desktop while CUT is focused", TEXT, false).tag(if p.is_none_or(|p| p.grab) { "on" } else { "off" }, if p.is_none_or(|p| p.grab) { GREEN } else { DIM }),
                        DevRow::Surface => {
                            let k = match d.map(|d| d.class) {
                                Some(Class::Touch(k)) => self.touch_kind(i, k),
                                _ => Kind::Touchpad,
                            };
                            Row::new("Works as", TEXT, false).tag(if k == Kind::TabletTouch { "tablet: 1 finger scratches" } else { "touchpad: 2 fingers scratch" }, LILAC)
                        }
                        DevRow::Reset => Row::new("Reset to the built-in setup", TEXT, false),
                        DevRow::Forget => Row::new("Forget this setup", RED, false),
                    })
                    .collect();
                (format!("\u{f11b}  {name}"), p.map(|p| (devices::slug(&p.id) + ".txt", DIM)), rows, &[("Enter", "learn / change"), ("\u{2190}\u{2192}", "sensitivity"), ("Del", "clear"), ("Esc", "back")])
            }
            Stage::Learn(l) => {
                sel = 0;
                let d = self.devs.get(&l.inst);
                let p = d.and_then(|d| self.dev_profiles.get(&d.id));
                let name = d.map(|d| d.name.clone()).unwrap_or_default();
                let mut rows = Vec::new();
                let progress = if l.guided { format!("step {} of {}", l.at + 1, l.steps.len()) } else { String::new() };
                match l.step() {
                    Some(Step::Scratch) => {
                        rows.push(Row::new("Move or turn what you want to scratch with", WHITE, true));
                        rows.push(Row::new("a jog wheel, dial, ring, knob or stick - keep going for a second", LABEL_TEXT, false));
                        match d.and_then(|d| l.best(d)) {
                            Some(b) => rows.push(Row::new(&format!("heard: {}", b.label()), GREEN, true).tag("Enter to keep", GREEN)),
                            None => rows.push(Row::new("listening...", DIM, false)),
                        }
                    }
                    Some(Step::Calibrate) => {
                        rows.push(Row::new("Turn it exactly one full turn, either way", WHITE, true));
                        rows.push(Row::new("then Enter. That's one turn of the record. Enter now to skip", LABEL_TEXT, false));
                        rows.push(Row::new(&format!("{:.0} steps", l.turned), if l.turned > 0.0 { GREEN } else { DIM }, true));
                    }
                    Some(Step::Action(a)) => {
                        let (_, label, act) = ACTIONS[a];
                        let what = match act {
                            Action::Volume => "Turn or slide what should set the volume",
                            Action::Fader => "Press the buttons (or move the fader) that should cut the sound",
                            Action::Hold => "Press what holds the record: a button, or touch the jog's top plate",
                            _ => "Press the control for this",
                        };
                        rows.push(Row::new(what, WHITE, true).tag(label, LILAC));
                        let now: Vec<String> = if l.got.is_empty() {
                            p.map(|p| p.binds_for(a).map(Bound::label).collect()).unwrap_or_default()
                        } else {
                            l.got.iter().map(Bound::label).collect()
                        };
                        if now.is_empty() {
                            rows.push(Row::new("listening...", DIM, false));
                        } else {
                            rows.push(Row::new(&now.join(", "), GREEN, true).tag(if l.got.is_empty() { "now" } else { "heard" }, GREEN));
                        }
                        if matches!(act, Action::Fader) {
                            rows.push(Row::new("more than one is fine. Enter when done", LABEL_TEXT, false));
                        }
                    }
                    None => {}
                }
                (format!("\u{f130}  {name}"), Some((progress, LILAC)), rows, &[("Enter", "keep / next"), ("Tab", "skip"), ("Del", "clear"), ("Esc", "stop")])
            }
        };
        draw_panel(c, f, &title, sub, &rows, sel, hints, &mut m.rows, &mut m.panel);
    }
}

/// One control event while learning.
/// Signals that aren't controls: ABS_MISC, which a Wacom pad sends with every button
/// press, and axes whose range is a single value.
fn not_a_control(ctl: Ctl, info: &DevInfo) -> bool {
    match ctl {
        Ctl::Abs(c) => c == 0x28 || info.ranges.get(&c).is_some_and(|(lo, hi)| lo >= hi),
        _ => false,
    }
}

fn learn_event(l: &mut Learn, info: &DevInfo, profile: Option<&DevProfile>, ctl: Ctl, value: i32, prev: Option<i32>, range: (i32, i32)) {
    if not_a_control(ctl, info) {
        return;
    }
    l.stats.entry(ctl).or_default().add(value, range, info.is_midi());
    match l.step() {
        Some(Step::Calibrate) => {
            if let Some(s) = profile.and_then(|p| p.scratch).filter(|s| s.ctl == ctl) {
                l.turned += turned(s, value, prev, range).abs();
            }
        }
        Some(Step::Action(a)) => {
            let st = &l.stats[&ctl];
            let b = match ctl {
                Ctl::Key(_) | Ctl::Note(..) if value > 0 => Some(Bound { ctl, enc: Enc::Button }),
                Ctl::Rel(_) if matches!(ACTIONS[a].2, Action::Volume) || st.n >= 3 => Some(Bound { ctl, enc: Enc::Rel }),
                Ctl::Cc(..) if st.enc(ctl, range) == Enc::Button && value >= 64 => Some(Bound { ctl, enc: Enc::Button }),
                Ctl::Abs(_) | Ctl::Cc(..) | Ctl::Bend(_) if st.moved(ctl, range) => Some(Bound { ctl, enc: st.enc(ctl, range) }),
                _ => None,
            };
            if let Some(b) = b {
                if let Some(old) = l.got.iter_mut().find(|g| g.ctl == b.ctl) {
                    *old = b;
                } else {
                    // One knob/fader/wheel for volume; any number of buttons elsewhere.
                    if matches!(ACTIONS[a].2, Action::Volume) {
                        l.got.clear();
                    }
                    l.got.push(b);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(vals: &[i32], midi: bool, range: (i32, i32)) -> Stats {
        let mut s = Stats::default();
        for &v in vals {
            s.add(v, range, midi);
        }
        s
    }

    #[test]
    fn detects_midi_jog_encodings() {
        let cc = Ctl::Cc(0, 34);
        // Pioneer-style: 64 = still, 65 forward, 63 back.
        assert_eq!(feed(&[65, 65, 66, 65, 65, 65, 64, 65], true, (0, 127)).enc(cc, (0, 127)), Enc::Rel64);
        // Two's complement: 1 forward, 127 back.
        assert_eq!(feed(&[1, 1, 2, 1, 1, 127, 127, 1], true, (0, 127)).enc(cc, (0, 127)), Enc::Rel2c);
        // A knob turned through its range.
        assert_eq!(feed(&[10, 20, 30, 40, 50, 60, 70], true, (0, 127)).enc(cc, (0, 127)), Enc::Axis);
        // A button sending CC 127 / 0.
        assert_eq!(feed(&[127, 0, 127], true, (0, 127)).enc(cc, (0, 127)), Enc::Button);
    }

    #[test]
    fn detects_rings() {
        let ring = Ctl::Abs(8);
        let s = feed(&[60, 65, 70, 71, 2, 6, 10, 20, 30, 40], false, (0, 71));
        assert_eq!(s.enc(ring, (0, 71)), Enc::Wrap);
    }

    #[test]
    fn deltas() {
        assert_eq!(delta(Enc::Rel64, 66, None, (0, 127)), 2.0);
        assert_eq!(delta(Enc::Rel2c, 126, None, (0, 127)), -2.0);
        // Wrapping forward past the end of a ring.
        assert_eq!(delta(Enc::Wrap, 1, Some(70), (0, 71)), 3.0);
        // A finger put down somewhere else isn't a turn.
        assert_eq!(delta(Enc::Wrap, 30, Some(5), (0, 71)), 0.0);
        // Nor is lifting off a tablet ring (0) and touching it again.
        let ring = Bound { ctl: Ctl::Abs(8), enc: Enc::Wrap };
        assert_eq!(turned(ring, 12, Some(0), (0, 71)), 0.0);
        assert_eq!(turned(ring, 0, Some(33), (0, 71)), 0.0);
        assert_eq!(turned(ring, 1, Some(71), (0, 71)), 1.0);
    }

    #[test]
    fn wacom_pad_activity_signal_is_not_learned() {
        let info = DevInfo {
            inst: "/dev/input/event10".into(),
            id: "056a:0315 Wacom Intuos Pro M Pad".into(),
            name: "Wacom Intuos Pro M Pad".into(),
            class: Class::Controls,
            readable: true,
            vendor: 0x056a,
            product: 0x0315,
            ranges: [(8, (0, 71)), (40, (0, 0))].into_iter().collect(),
            grab: Default::default(),
            forward: Default::default(),
        };
        let mut l = Learn { inst: info.inst.clone(), steps: vec![Step::Action(devices::action("click"))], at: 0, stats: HashMap::new(), got: Vec::new(), turned: 0.0, guided: false };
        // A button press arrives with ABS_MISC jumping 0 -> 15 -> 0 around it.
        for (ctl, v) in [(Ctl::Abs(40), 15), (Ctl::Key(263), 1), (Ctl::Abs(40), 0), (Ctl::Key(263), 0)] {
            learn_event(&mut l, &info, None, ctl, v, None, info.range(ctl));
        }
        assert_eq!(l.got, vec![Bound { ctl: Ctl::Key(263), enc: Enc::Button }]);
    }

    #[test]
    fn guided_steps_exist() {
        for a in GUIDED {
            devices::action(a);
        }
    }

    #[test]
    fn profile_round_trip() {
        let mut p = devices::builtin("056a:0357 Wacom Intuos Pro M Pad", "Wacom Intuos Pro M Pad").unwrap();
        p.scratch = Some(Bound { ctl: Ctl::Cc(0, 34), enc: Enc::Rel64 });
        p.per_turn = Some(512.0);
        p.sens = 1.5;
        p.reverse = true;
        assert_eq!(DevProfile::parse(&p.text()), Some(p));
    }
}

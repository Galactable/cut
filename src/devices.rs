//! What CUT knows about input devices beyond the keyboard and mouse: the controls a
//! device has, and the per-device setup files that turn those controls into the
//! scratch wheel and the same commands the keys run.
//!
//! A setup lives in `~/.config/cut/devices/<device>.txt`, in the same `key=value` style
//! as the settings and profiles:
//!
//! ```text
//! id=056a:0357 Wacom Intuos Pro M Pad
//! name=Wacom Intuos Pro M Pad
//! grab=true
//! scratch=cc:0:34/rel64
//! per_turn=128
//! fader=key:256 key:257 key:258 key:259
//! motor=key:260
//! ```

use std::{
    collections::HashMap,
    fmt,
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};

use winit::keyboard::KeyCode;

use crate::input::Kind;

/// One control on a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ctl {
    /// evdev key or button (EV_KEY code).
    Key(u16),
    /// evdev absolute axis (EV_ABS code).
    Abs(u16),
    /// evdev relative axis (EV_REL code).
    Rel(u16),
    /// MIDI note (channel 0-15, note).
    Note(u8, u8),
    /// MIDI control change (channel, controller).
    Cc(u8, u8),
    /// MIDI pitch bend (channel).
    Bend(u8),
}

impl fmt::Display for Ctl {
    /// The form used in device files.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Ctl::Key(c) => write!(f, "key:{c}"),
            Ctl::Abs(c) => write!(f, "abs:{c}"),
            Ctl::Rel(c) => write!(f, "rel:{c}"),
            Ctl::Note(ch, n) => write!(f, "note:{ch}:{n}"),
            Ctl::Cc(ch, n) => write!(f, "cc:{ch}:{n}"),
            Ctl::Bend(ch) => write!(f, "bend:{ch}"),
        }
    }
}

impl Ctl {
    pub fn parse(s: &str) -> Option<Ctl> {
        let mut it = s.split(':');
        let kind = it.next()?;
        let mut num = || it.next().and_then(|v| v.parse::<u16>().ok());
        Some(match kind {
            "key" => Ctl::Key(num()?),
            "abs" => Ctl::Abs(num()?),
            "rel" => Ctl::Rel(num()?),
            "note" => Ctl::Note(num()?.min(15) as u8, num()?.min(127) as u8),
            "cc" => Ctl::Cc(num()?.min(15) as u8, num()?.min(127) as u8),
            "bend" => Ctl::Bend(num()?.min(15) as u8),
            _ => return None,
        })
    }

    /// A name for people: the kernel's name for evdev codes, MIDI terms otherwise.
    pub fn label(&self) -> String {
        let tidy = |s: String| s.trim_start_matches("BTN_").trim_start_matches("KEY_").trim_start_matches("ABS_").trim_start_matches("REL_").to_string();
        match *self {
            Ctl::Key(c) => format!("button {}", tidy(format!("{:?}", evdev::Key::new(c)))),
            Ctl::Abs(c) => format!("axis {}", tidy(format!("{:?}", evdev::AbsoluteAxisType(c)))),
            Ctl::Rel(c) => format!("wheel {}", tidy(format!("{:?}", evdev::RelativeAxisType(c)))),
            Ctl::Note(ch, n) => format!("note {n} ch{}", ch + 1),
            Ctl::Cc(ch, n) => format!("CC {n} ch{}", ch + 1),
            Ctl::Bend(ch) => format!("pitch bend ch{}", ch + 1),
        }
    }

    /// Plain on/off controls.
    pub fn is_button(&self) -> bool {
        matches!(self, Ctl::Key(_) | Ctl::Note(..))
    }
}

/// How a control's values are read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Enc {
    /// On/off: pressed while the value is non-zero (or >= 64 for a MIDI CC).
    Button,
    /// Each event is a step (evdev wheels and dials).
    Rel,
    /// MIDI relative, 64 = no movement (most DJ jog wheels).
    Rel64,
    /// MIDI relative, two's complement: 1.. forward, 127.. back.
    Rel2c,
    /// A position along a range (sticks, sliders, knobs, faders).
    Axis,
    /// A position round a ring that wraps from the end back to the start.
    Wrap,
}

impl Enc {
    fn name(self) -> &'static str {
        match self {
            Enc::Button => "button",
            Enc::Rel => "rel",
            Enc::Rel64 => "rel64",
            Enc::Rel2c => "rel2c",
            Enc::Axis => "axis",
            Enc::Wrap => "wrap",
        }
    }

    fn parse(s: &str) -> Option<Enc> {
        Some(match s {
            "button" => Enc::Button,
            "rel" => Enc::Rel,
            "rel64" => Enc::Rel64,
            "rel2c" => Enc::Rel2c,
            "axis" => Enc::Axis,
            "wrap" => Enc::Wrap,
            _ => return None,
        })
    }

    /// What a control reads as when nothing more is known.
    pub fn default_for(ctl: Ctl) -> Enc {
        match ctl {
            Ctl::Key(_) | Ctl::Note(..) => Enc::Button,
            Ctl::Rel(_) => Enc::Rel,
            Ctl::Abs(_) | Ctl::Cc(..) | Ctl::Bend(_) => Enc::Axis,
        }
    }

    pub fn is_turning(self) -> bool {
        matches!(self, Enc::Rel | Enc::Rel64 | Enc::Rel2c | Enc::Wrap)
    }
}

/// A control and how to read it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bound {
    pub ctl: Ctl,
    pub enc: Enc,
}

impl Bound {
    fn parse(s: &str) -> Option<Bound> {
        let (c, e) = match s.split_once('/') {
            Some((c, e)) => (c, Some(e)),
            None => (s, None),
        };
        let ctl = Ctl::parse(c)?;
        let enc = e.and_then(Enc::parse).unwrap_or(Enc::default_for(ctl));
        Some(Bound { ctl, enc })
    }

    fn text(&self) -> String {
        if self.enc == Enc::default_for(self.ctl) {
            self.ctl.to_string()
        } else {
            format!("{}/{}", self.ctl, self.enc.name())
        }
    }

    pub fn label(&self) -> String {
        match self.enc {
            Enc::Rel64 | Enc::Rel2c => format!("{} (relative)", self.ctl.label()),
            Enc::Wrap => format!("{} (ring)", self.ctl.label()),
            _ => self.ctl.label(),
        }
    }
}

/// What a control does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// Cuts the sound while held (or hamster: opens it). Like Space/J/K/L.
    Fader,
    /// Hand on the record while held, like Shift. Also a jog wheel's touch plate.
    Hold,
    /// Turning sets the volume; a slider or knob sets it directly.
    Volume,
    /// Runs exactly what this key does.
    Cmd(KeyCode),
}

/// Every action a control can take: (file key, label for people, action).
pub const ACTIONS: &[(&str, &str, Action)] = &[
    ("fader", "Crossfader (cuts while held)", Action::Fader),
    ("hold", "Hold record / jog touch plate", Action::Hold),
    ("motor", "Motor on/off", Action::Cmd(KeyCode::KeyM)),
    ("cue", "Back to cue", Action::Cmd(KeyCode::KeyC)),
    ("record", "Record take", Action::Cmd(KeyCode::KeyR)),
    ("click", "Click on/off", Action::Cmd(KeyCode::KeyT)),
    ("volume", "Volume (ring, knob or slider)", Action::Volume),
    ("vol_up", "Volume up", Action::Cmd(KeyCode::ArrowUp)),
    ("vol_down", "Volume down", Action::Cmd(KeyCode::ArrowDown)),
    ("hamster", "Hamster fader on/off", Action::Cmd(KeyCode::KeyI)),
    ("rpm33", "33 rpm", Action::Cmd(KeyCode::Digit3)),
    ("rpm45", "45 rpm", Action::Cmd(KeyCode::Digit4)),
    ("prev", "Previous file", Action::Cmd(KeyCode::Comma)),
    ("next", "Next file", Action::Cmd(KeyCode::Period)),
    ("loop", "Loop section", Action::Cmd(KeyCode::KeyG)),
    ("flip", "Flip direction", Action::Cmd(KeyCode::KeyD)),
    ("sens_down", "Less sensitive", Action::Cmd(KeyCode::BracketLeft)),
    ("sens_up", "More sensitive", Action::Cmd(KeyCode::BracketRight)),
    ("bpm_down", "BPM down", Action::Cmd(KeyCode::Digit9)),
    ("bpm_up", "BPM up", Action::Cmd(KeyCode::Digit0)),
    ("eq", "Vinyl EQ on/off", Action::Cmd(KeyCode::KeyV)),
    ("noise", "Surface noise on/off", Action::Cmd(KeyCode::KeyN)),
    ("fullscreen", "Fullscreen", Action::Cmd(KeyCode::F11)),
];

/// The actions the guided setup asks for, in order (indexes into ACTIONS).
pub const GUIDED: &[usize] = &[0, 1, 2, 3, 4, 6];

/// How a touch surface behaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Surface {
    /// Two fingers scratch, one is the cursor.
    Touchpad,
    /// One finger scratches; CUT takes it from the desktop while focused.
    Tablet,
}

impl Surface {
    pub fn kind(self) -> Kind {
        match self {
            Surface::Touchpad => Kind::Touchpad,
            Surface::Tablet => Kind::TabletTouch,
        }
    }
}

/// One device's setup.
#[derive(Clone, Debug, PartialEq)]
pub struct DevProfile {
    pub id: String,
    pub name: String,
    /// Take the device from the desktop while CUT is focused.
    pub grab: bool,
    pub surface: Option<Surface>,
    pub scratch: Option<Bound>,
    /// Control units in one turn of the record. None: a guess from the encoding.
    pub per_turn: Option<f64>,
    /// This device's own sensitivity, on top of the controller [ ] setting.
    pub sens: f64,
    /// Turn the scratch control the other way round.
    pub reverse: bool,
    /// (index into ACTIONS, control).
    pub binds: Vec<(usize, Bound)>,
}

impl DevProfile {
    pub fn new(id: &str, name: &str) -> Self {
        DevProfile { id: id.to_string(), name: name.to_string(), grab: true, surface: None, scratch: None, per_turn: None, sens: 1.0, reverse: false, binds: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.scratch.is_none() && self.binds.is_empty() && self.surface.is_none()
    }

    pub fn binds_for(&self, action: usize) -> impl Iterator<Item = &Bound> {
        self.binds.iter().filter(move |(a, _)| *a == action).map(|(_, b)| b)
    }

    pub fn text(&self) -> String {
        let mut s = format!("# CUT device setup. Change it from the Devices menu (X) in CUT.\nid={}\nname={}\ngrab={}\n", self.id, self.name, self.grab);
        if let Some(sf) = self.surface {
            s.push_str(&format!("surface={}\n", if sf == Surface::Tablet { "tablet" } else { "touchpad" }));
        }
        if let Some(b) = &self.scratch {
            s.push_str(&format!("scratch={}\n", b.text()));
        }
        if let Some(p) = self.per_turn {
            s.push_str(&format!("per_turn={p}\n"));
        }
        s.push_str(&format!("sens={}\nreverse={}\n", self.sens, self.reverse));
        for (i, (key, _, _)) in ACTIONS.iter().enumerate() {
            let list: Vec<String> = self.binds_for(i).map(Bound::text).collect();
            if !list.is_empty() {
                s.push_str(&format!("{key}={}\n", list.join(" ")));
            }
        }
        s
    }

    pub fn parse(text: &str) -> Option<DevProfile> {
        let mut p = DevProfile::new("", "");
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "id" => p.id = v.to_string(),
                "name" => p.name = v.to_string(),
                "grab" => p.grab = v == "true",
                "surface" => p.surface = Some(if v == "tablet" { Surface::Tablet } else { Surface::Touchpad }),
                "scratch" => p.scratch = Bound::parse(v),
                "per_turn" => p.per_turn = v.parse().ok().filter(|n: &f64| *n > 0.0),
                "sens" => p.sens = v.parse().unwrap_or(1.0f64).clamp(0.05, 20.0),
                "reverse" => p.reverse = v == "true",
                _ => {
                    if let Some(i) = ACTIONS.iter().position(|(key, _, _)| *key == k) {
                        p.binds.extend(v.split_whitespace().filter_map(Bound::parse).map(|b| (i, b)));
                    }
                }
            }
        }
        (!p.id.is_empty()).then_some(p)
    }

    pub fn path(&self) -> PathBuf {
        dir().join(format!("{}.txt", slug(&self.id)))
    }
}

pub fn dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| ".".into());
    home.join(".config").join("cut").join("devices")
}

pub fn load_all() -> HashMap<String, DevProfile> {
    let mut out = HashMap::new();
    if let Ok(d) = std::fs::read_dir(dir()) {
        for e in d.flatten() {
            if let Some(p) = std::fs::read_to_string(e.path()).ok().and_then(|t| DevProfile::parse(&t)) {
                out.insert(p.id.clone(), p);
            }
        }
    }
    out
}

/// A file-name-safe version of a device id.
pub fn slug(id: &str) -> String {
    let mut s = String::new();
    for ch in id.chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch.to_ascii_lowercase());
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    s.trim_matches('-').to_string()
}

/// The setup a known device starts with, before anyone has changed it.
pub fn builtin(id: &str, name: &str) -> Option<DevProfile> {
    let n = name.to_lowercase();
    // Wacom ExpressKeys and touch ring, laid out like tablet-setup.sh:
    // top four = crossfader, then motor, back to cue, record, hold record, click.
    if n.contains("wacom") && n.ends_with("pad") {
        let mut p = DevProfile::new(id, name);
        let key = |c: u16| Bound { ctl: Ctl::Key(c), enc: Enc::Button };
        let act = |k: &str| ACTIONS.iter().position(|(a, _, _)| *a == k).unwrap();
        for c in 0x100..=0x103 {
            p.binds.push((act("fader"), key(c)));
        }
        p.binds.push((act("motor"), key(0x104)));
        p.binds.push((act("cue"), key(0x105)));
        p.binds.push((act("record"), key(0x106)));
        p.binds.push((act("hold"), key(0x107)));
        p.binds.push((act("click"), key(0x108)));
        p.binds.push((act("volume"), Bound { ctl: Ctl::Abs(0x08), enc: Enc::Wrap }));
        p.binds.sort_by_key(|(a, _)| *a);
        return Some(p);
    }
    None
}

/// What kind of thing a device is, as far as CUT cares.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Class {
    /// A multi-touch surface, read by the touch path.
    Touch(Kind),
    /// Buttons, wheels, axes: set up from the Devices menu.
    Controls,
    Midi,
    /// Shown only with "show all devices".
    Pen,
    Keyboard,
    Mouse,
    Other,
}

impl Class {
    pub fn usual(self) -> bool {
        matches!(self, Class::Touch(_) | Class::Controls | Class::Midi)
    }
}

/// A device that's plugged in.
#[derive(Clone, Debug)]
pub struct DevInfo {
    /// This plug-in of it (the /dev/input node, or the MIDI port).
    pub inst: Arc<str>,
    /// What its setup is saved under: the same for every one of the same model.
    pub id: String,
    pub name: String,
    pub class: Class,
    /// We can read it. If not, it needs a udev rule (see grant_access).
    pub readable: bool,
    pub vendor: u16,
    pub product: u16,
    /// evdev absolute axes: (min, max).
    pub ranges: HashMap<u16, (i32, i32)>,
    /// Set by the app: take the device from the desktop now.
    pub grab: Arc<AtomicBool>,
    /// Set by the app: send its controls (only when something uses them).
    pub forward: Arc<AtomicBool>,
}

impl DevInfo {
    pub fn range(&self, ctl: Ctl) -> (i32, i32) {
        match ctl {
            Ctl::Abs(c) => self.ranges.get(&c).copied().unwrap_or((0, 255)),
            Ctl::Bend(_) => (-8192, 8191),
            Ctl::Key(_) => (0, 1),
            _ => (0, 127),
        }
    }

    pub fn is_midi(&self) -> bool {
        self.class == Class::Midi
    }
}

/// `cut-deck --grant-access VENDOR PRODUCT NAME`, run as root through pkexec: lets the
/// logged-in user read that one device (and nothing else) from now on.
pub fn grant_access(vendor: &str, product: &str, name: &str) -> anyhow::Result<()> {
    let hex = |s: &str| s.len() == 4 && s.chars().all(|c| c.is_ascii_hexdigit());
    anyhow::ensure!(hex(vendor) && hex(product), "vendor and product must be 4 hex digits");
    // udev patterns: anything unusual in the name becomes a one-character wildcard.
    let name: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || " -_.:()".contains(c) { c } else { '?' }).collect();
    let line = format!(
        "SUBSYSTEM==\"input\", KERNEL==\"event*\", ATTRS{{id/vendor}}==\"{}\", ATTRS{{id/product}}==\"{}\", ATTRS{{name}}==\"{}\", TAG+=\"uaccess\"",
        vendor.to_lowercase(),
        product.to_lowercase(),
        name
    );
    let path = "/etc/udev/rules.d/70-cut-devices.rules";
    let mut text = std::fs::read_to_string(path).unwrap_or_else(|_| "# CUT: devices you gave CUT access to from its Devices menu.\n".to_string());
    if !text.lines().any(|l| l == line) {
        text.push_str(&line);
        text.push('\n');
        std::fs::write(path, text)?;
    }
    std::process::Command::new("udevadm").args(["control", "--reload"]).status()?;
    std::process::Command::new("udevadm").args(["trigger", "--subsystem-match=input", "--action=change"]).status()?;
    Ok(())
}

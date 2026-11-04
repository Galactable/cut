mod deck;
mod desk;
mod devices;
mod devmenu;
mod dsp;
mod engine;
mod input;
mod library;
mod midi;
mod recorder;
mod text;
mod ui;

use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU32,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{atomic::Ordering::Relaxed, mpsc, Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use winit::{
    application::ApplicationHandler,
    platform::x11::WindowAttributesExtX11,
    event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Fullscreen, Window, WindowId},
};

use deck::{Record, Source};
use engine::{Engine, Msg, Shared};
use input::{InEvent, Kind};
use ui::*;

/// The menu bar: (title, items). Each item runs exactly what its key does.
const MENUS: &[(&str, &[(&str, KeyCode, &str)])] = &[
    ("FILE", &[
        ("Open sample...", KeyCode::KeyO, "O"),
        ("Previous file", KeyCode::Comma, ","),
        ("Next file", KeyCode::Period, "."),
        ("Save section to library...", KeyCode::KeyS, "S"),
        ("Save format", KeyCode::KeyE, "E"),
        ("Profiles...", KeyCode::KeyP, "P"),
        ("Folders...", KeyCode::KeyF, "F"),
        ("Devices...", KeyCode::KeyX, "X"),
        ("Quit", KeyCode::KeyQ, "Ctrl+Q"),
    ]),
    ("SECTION", &[
        ("Whole sample", KeyCode::KeyA, "A"),
        ("Zoom to section", KeyCode::KeyZ, "Z"),
        ("Loop section", KeyCode::KeyG, "G"),
        ("Back to cue", KeyCode::KeyC, "C"),
    ]),
    ("DECK", &[
        ("Motor", KeyCode::KeyM, "M"),
        ("33 rpm", KeyCode::Digit3, "3"),
        ("45 rpm", KeyCode::Digit4, "4"),
        ("Hamster fader", KeyCode::KeyI, "I"),
        ("Volume up", KeyCode::ArrowUp, "Up"),
        ("Volume down", KeyCode::ArrowDown, "Down"),
    ]),
    ("SCRATCH", &[
        ("Scratch motion", KeyCode::Tab, "Tab"),
        ("Flip direction", KeyCode::KeyD, "D"),
        ("Less sensitive", KeyCode::BracketLeft, "["),
        ("More sensitive", KeyCode::BracketRight, "]"),
        ("Smoother", KeyCode::Minus, "-"),
        ("Snappier", KeyCode::Equal, "="),
    ]),
    ("SOUND", &[("Vinyl EQ", KeyCode::KeyV, "V"), ("Surface noise", KeyCode::KeyN, "N")]),
    ("TAKES", &[
        ("Record take", KeyCode::KeyR, "R"),
        ("Click", KeyCode::KeyT, "T"),
        ("BPM down", KeyCode::Digit9, "9"),
        ("BPM up", KeyCode::Digit0, "0"),
    ]),
    ("BACKING", &[
        ("Load backing track...", KeyCode::KeyY, "Y"),
        ("Play / pause", KeyCode::KeyB, "B"),
        ("Volume down", KeyCode::Digit5, "5"),
        ("Volume up", KeyCode::Digit6, "6"),
        ("Record it into takes", KeyCode::KeyU, "U"),
    ]),
    ("VIEW", &[("Key guide", KeyCode::KeyH, "H"), ("Fullscreen", KeyCode::F11, "F11"), ("What's new", KeyCode::F1, "F1")]),
];

const BAR_H: i32 = 34;
const ICON_PX: usize = 22;
const STATUS_H: i32 = 30;
const BAR_BG: u32 = 0x131319;
const EDGE: u32 = 0x2c2c3a;

/// Menu bar state, plus where things were drawn for the pointer.
#[derive(Default)]
struct MenuBar {
    open: Option<usize>,
    sel: usize,
    titles: Vec<(i32, i32)>,
    drop: (i32, i32, i32, i32),
    rows: Vec<(i32, i32)>,
}

/// Chip labels: quieter than values, but readable.
const LABEL_TEXT: u32 = 0x9898b4;

/// One platter revolution at 33 1/3 rpm, in seconds of audio.
const SEC_PER_REV: f64 = 1.8;
/// Sensitivity at x1.00: how far you move for one second of audio. A 0.25 s baby scratch
/// is ~1.6 cm on a touchpad and ~5 cm on a tablet's touch surface.
const PX_PER_SEC: f64 = 1300.0;
const TOUCH_MM_PER_SEC: f64 = 65.0;
const TABLET_TOUCH_MM_PER_SEC: f64 = 200.0;
/// Scroll units (a "line") to screen pixels, for two-finger trackpad/touch scrolling.
const SCROLL_PX: f64 = 40.0;
/// Scrolling counts as a hand on the record until it goes quiet for this long.
const SCROLL_HOLD: Duration = Duration::from_millis(140);

/// Which guide groups go in which column (indexes into GUIDE), left to right.
const GUIDE_COLUMNS: &[&[usize]] = &[&[0, 4, 6], &[2, 3], &[1, 5, 7]];

/// The key guide, by group. Headings carry a Nerd Font icon.
const GUIDE: &[(&str, &str, &[(&str, &str)])] = &[
    ("\u{f001}", "SCRATCH", &[
        ("mouse drag", "hand on record"),
        ("2 fingers", "grab (touchpad)"),
        ("1 finger", "grab (tablet)"),
        ("Shift", "hold record"),
        ("Tab", "scratch motion"),
        ("D", "flip direction"),
        ("[ ]", "sensitivity"),
        ("- =", "smoothing"),
    ]),
    ("\u{f1de}", "FADER", &[("Space J K L", "cut"), ("I", "hamster mode")]),
    ("\u{f04b}", "DECK", &[("M", "motor on/off"), ("C", "back to cue"), ("3 4", "33 / 45 rpm"), ("Up Down", "volume")]),
    ("\u{f0c4}", "SAMPLE", &[
        ("drag wave", "pick section"),
        ("drag edge", "trim section"),
        ("A", "whole sample"),
        ("Z / wheel", "zoom waveform"),
        ("G", "loop section"),
        ("S", "save to library"),
        (", .", "prev / next file"),
        ("O", "open file"),
    ]),
    ("\u{f130}", "TAKES", &[("R", "record take"), ("E", "WAV / MP3"), ("T", "click on/off"), ("9 0", "BPM (Ctrl = 10)")]),
    ("\u{f028}", "SOUND", &[("V", "vinyl EQ"), ("N", "surface noise")]),
    ("\u{f001}", "BACKING", &[("Y", "load track"), ("B", "play / pause"), ("5 6", "its volume"), ("U", "into takes")]),
    ("\u{f108}", "APP", &[("Alt / F10", "menu bar"), ("H", "hide keys"), ("P", "profiles"), ("F", "folders"), ("X", "devices"), ("F11", "fullscreen"), ("Ctrl+Q", "quit")]),
];

#[derive(Clone, Copy, PartialEq)]
enum Which {
    Samples,
    Takes,
}

enum FolderStage {
    /// Samples or takes?
    Pick,
    /// Walking the disk to choose a folder.
    Browse { which: Which, dir: PathBuf, subdirs: Vec<String>, naming: Option<String> },
    /// The old folder has files: move them, or just switch?
    Move { which: Which, to: PathBuf },
}

struct FolderMenu {
    stage: FolderStage,
    sel: usize,
    rows: Vec<(i32, i32)>,
    panel: (i32, i32, i32, i32),
}

impl FolderMenu {
    fn row_count(&self) -> usize {
        match &self.stage {
            FolderStage::Pick | FolderStage::Move { .. } => 2,
            FolderStage::Browse { subdirs, .. } => 3 + subdirs.len(),
        }
    }
}

fn browse(which: Which, dir: PathBuf) -> FolderStage {
    let mut subdirs: Vec<String> = std::fs::read_dir(&dir)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.'))
                .collect()
        })
        .unwrap_or_default();
    subdirs.sort_by_key(|n| n.to_lowercase());
    FolderStage::Browse { which, dir, subdirs, naming: None }
}

struct ProfileMenu {
    names: Vec<String>,
    /// 0 is "save current settings", then one row per profile.
    sel: usize,
    /// The name being typed when saving.
    naming: Option<String>,
    /// The row waiting for a second Delete press.
    confirm_delete: Option<usize>,
    /// Reloading the active profile would throw away changes: needs a second Enter.
    confirm_revert: bool,
    /// Row extents (y0, y1) and the panel, from the last paint, for the pointer.
    rows: Vec<(i32, i32)>,
    panel: (i32, i32, i32, i32),
}

/// This build's version, and its release notes (newest first, `## x.y.z` per release).
const VERSION: &str = env!("CARGO_PKG_VERSION");
const CHANGELOG: &str = include_str!("../CHANGELOG.md");

/// The "what's new" panel: the notes, wrapped to the panel, and how far it's scrolled.
struct WhatsNew {
    lines: Vec<(String, u32, bool)>,
    top: usize,
    rows: Vec<(i32, i32)>,
    panel: (i32, i32, i32, i32),
}

/// Compares "0.10.2" > "0.9.0" the way people mean it.
fn version_key(v: &str) -> Vec<u64> {
    v.trim().trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

enum ProfileAction {
    Load(String),
    Save(String),
    Delete(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Follows the line you're scratching along, whatever its angle.
    Auto,
    LinearX,
    LinearY,
    Platter,
}

impl Mode {
    fn short(self) -> &'static str {
        match self {
            Mode::Auto => "AUTO",
            Mode::LinearX => "LEFT/RIGHT",
            Mode::LinearY => "UP/DOWN",
            Mode::Platter => "PLATTER",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Mode::Auto => "AUTO (follows your stroke)",
            Mode::LinearX => "LINEAR LEFT/RIGHT",
            Mode::LinearY => "LINEAR UP/DOWN",
            Mode::Platter => "PLATTER (circle)",
        }
    }
}

#[derive(Clone, Copy)]
struct Layout {
    overview: (i32, i32, i32, i32),
    platter: (f64, f64, f64),
}

/// Starting layout until the first frame is painted (paint() owns the real one).
fn layout(w: i32, h: i32) -> Layout {
    let overview = (12, 58, w - 24, 70);
    let top = 298.0;
    let bottom = h as f64 - 62.0;
    let r = ((bottom - top) / 2.0 - 8.0).min(w as f64 / 2.0 - 20.0).max(20.0);
    Layout { overview, platter: (w as f64 / 2.0, (top + bottom) / 2.0, r) }
}

struct App {
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    tx: rtrb::Producer<Msg>,
    trash: rtrb::Consumer<Arc<Record>>,
    shared: Arc<Shared>,
    take_status: Arc<Mutex<Option<String>>>,
    _stream: cpal::Stream,
    out_rate: u32,

    src: Source,
    path: Option<PathBuf>,
    region: (usize, usize),
    rec: Arc<Record>,
    peaks: Vec<(f32, f32)>,
    /// What the peaks were computed for: (view start, view end, columns).
    peaks_key: (u64, u64, usize),
    /// The part of the file the top waveform shows, in samples (zoomed with the wheel or Z).
    view: (f64, f64),
    /// Play the section as a seamless loop instead of a record with lead-in and run-out.
    loop_section: bool,

    /// Scratch motion per input (mouse, touchpad, tablet touch, controller).
    mode: [Mode; 4],
    dir: f64,
    /// Sensitivity multiplier per input (mouse, touchpad, tablet touch, controller).
    sens: [f64; 4],
    /// The input [ and ] adjust: whichever you used last.
    sens_kind: Kind,
    smooth_hz: f64,
    eq: bool,
    noise: bool,
    invert: bool,
    motor: bool,
    rpm45: bool,
    volume: f32,
    click: bool,
    bpm: f64,

    fader_keys: HashSet<KeyCode>,
    fader_open: bool,

    cursor: (f64, f64),
    last_angle: f64,
    mouse_scratch: bool,
    shift: bool,
    ctrl: bool,
    scroll_last: Option<Instant>,
    hand: bool,
    hand_u: f64,
    region_drag: Option<f64>,
    in_status: Arc<input::Status>,
    /// Where things were drawn last frame, for hit-testing clicks and the platter centre.
    lay: Layout,
    fonts: text::Fonts,
    desk: desk::Desk,
    focused: bool,
    /// Pen or touch currently holding the record through direct input.
    direct: Option<Kind>,
    direct_prev: (f64, f64),
    direct_angle: f64,
    /// Recent motion covariance (xx, yy, xy) for AUTO mode.
    stroke_cov: (f64, f64, f64),
    stroke_axis: (f64, f64),

    note: Option<(String, Instant)>,
    open_rx: Option<mpsc::Receiver<Option<PathBuf>>>,
    save_rx: Option<mpsc::Receiver<Option<(String, String, String)>>>,
    menu: Option<ProfileMenu>,
    bar: MenuBar,
    quit: bool,
    /// The torn-record app icon, for the menu bar.
    icon: Vec<u32>,
    /// The record, pre-drawn (see ui::PlatterImage).
    platter_img: Option<ui::PlatterImage>,
    /// Something changed that needs a redraw (input, a message...). Engine-driven
    /// changes are caught by comparing `frame_sig` with what was last drawn.
    dirty: bool,
    /// Touch moved the record: a partial redraw is enough.
    moved: bool,
    last_sig: (u64, u32, u64, bool),
    folders: Option<FolderMenu>,
    /// Where saved sections go, and where takes are recorded (shared with the recorder).
    samples_dir: PathBuf,
    takes_dir: Arc<Mutex<PathBuf>>,
    /// Save sections and takes as MP3 instead of WAV (shared with the recorder).
    save_mp3: Arc<std::sync::atomic::AtomicBool>,
    /// The key guide; hidden at start for a full view of the record (H).
    show_guide: bool,
    /// The settings profile last loaded or saved, and whether settings changed since.
    profile: Option<String>,
    profile_dirty: bool,
    last_draw: Instant,

    /// Devices plugged in now, by `inst`, and every device setup, by id.
    devs: HashMap<Arc<str>, devices::DevInfo>,
    dev_profiles: HashMap<String, devices::DevProfile>,
    dev_menu: Option<devmenu::DevMenu>,
    /// Last value of each device control, and which are held down.
    dev_last: HashMap<(Arc<str>, devices::Ctl), i32>,
    dev_pressed: HashSet<(Arc<str>, devices::Ctl)>,
    /// Device controls holding the fader cut / the record.
    dev_fader: HashSet<(Arc<str>, devices::Ctl)>,
    dev_hold: HashSet<(Arc<str>, devices::Ctl)>,
    /// Turning left over towards the next volume step, per control.
    vol_acc: HashMap<(Arc<str>, devices::Ctl), f64>,
    /// A jog without a touch plate holds the record while it's turning.
    jog_last: Option<Instant>,
    started: Instant,
    /// The newest version whose notes you've seen; the badge says NEW until you look.
    version_seen: String,
    whats_new: Option<WhatsNew>,
    /// Where the version badge was drawn, for clicks.
    version_box: (i32, i32, i32, i32),

    /// A track that plays under the record. The previous one is kept a while so the
    /// audio thread never frees it.
    backing: Option<Arc<Source>>,
    backing_prev: Option<Arc<Source>>,
    backing_path: Option<PathBuf>,
    backing_on: bool,
    backing_vol: f32,
    backing_in_take: bool,
    backing_rx: Option<mpsc::Receiver<Option<(PathBuf, Result<Source, String>)>>>,
}

impl App {
    fn send(&mut self, m: Msg) {
        let _ = self.tx.push(m);
    }

    /// Same clock as the kernel's input timestamps.
    fn now_us(&self) -> u64 {
        input::micros(SystemTime::now())
    }

    fn settings_path() -> PathBuf {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| ".".into());
        home.join(".config").join("cut").join("settings.txt")
    }

    /// Feel and mixer settings as `key=value` lines (also the profile file format).
    fn settings_text(&self) -> String {
        let m = |m: Mode| match m {
            Mode::Auto => "auto",
            Mode::LinearX => "x",
            Mode::LinearY => "y",
            Mode::Platter => "platter",
        };
        format!(
            "sens_mouse={}\nsens_touchpad={}\nsens_tablet_touch={}\nsens_controller={}\nmode_mouse={}\nmode_touchpad={}\nmode_tablet_touch={}\nsmooth={}\ndir={}\neq={}\nnoise={}\ninvert={}\nvolume={}\nbpm={}\nrpm45={}\nloop={}\n",
            self.sens[0], self.sens[1], self.sens[2], self.sens[3], m(self.mode[0]), m(self.mode[1]), m(self.mode[2]), self.smooth_hz, self.dir, self.eq, self.noise, self.invert, self.volume, self.bpm, self.rpm45, self.loop_section
        )
    }

    /// Saves the settings so the next run starts where you left off, including which
    /// profile they came from.
    fn save_settings(&self) {
        let mut text = self.settings_text();
        text.push_str(&format!(
            "samples_dir={}\ntakes_dir={}\nformat={}\nversion_seen={}\nbacking={}\nbacking_volume={}\nbacking_in_takes={}\n",
            self.samples_dir.display(),
            self.takes_dir.lock().unwrap().display(),
            if self.save_mp3.load(Relaxed) { "mp3" } else { "wav" },
            self.version_seen,
            self.backing_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            self.backing_vol,
            self.backing_in_take
        ));
        if let Some(p) = &self.profile {
            text.push_str(&format!("profile={p}\nprofile_changed={}\n", self.profile_dirty));
        }
        let path = Self::settings_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }

    fn load_settings(&mut self) {
        let Ok(text) = std::fs::read_to_string(Self::settings_path()) else {
            // A first run has nothing new to show.
            self.version_seen = VERSION.to_string();
            return;
        };
        for line in text.lines() {
            match line.split_once('=') {
                Some(("profile", v)) => self.profile = Some(v.trim().to_string()).filter(|v| !v.is_empty()),
                Some(("profile_changed", v)) => self.profile_dirty = v.trim() == "true",
                Some(("samples_dir", v)) if !v.trim().is_empty() => self.samples_dir = PathBuf::from(v.trim()),
                Some(("format", v)) => self.save_mp3.store(v.trim() == "mp3", Relaxed),
                Some(("version_seen", v)) => self.version_seen = v.trim().to_string(),
                Some(("backing", v)) if !v.trim().is_empty() => self.backing_path = Some(PathBuf::from(v.trim())),
                Some(("backing_volume", v)) => self.backing_vol = v.trim().parse::<f32>().unwrap_or(0.7).clamp(0.0, 1.5),
                Some(("backing_in_takes", v)) => self.backing_in_take = v.trim() == "true",
                Some(("takes_dir", v)) if !v.trim().is_empty() => *self.takes_dir.lock().unwrap() = PathBuf::from(v.trim()),
                _ => {}
            }
        }
        self.apply_settings(&text);
    }

    fn apply_settings(&mut self, text: &str) {
        let (old_eq, old_noise, old_loop) = (self.rec.eq, self.noise, self.rec.looped && !self.src.name.is_empty());
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let num = v.trim().parse::<f64>().ok();
            let flag = v.trim() == "true";
            match k.trim() {
                // "sens" is from before per-input sensitivity; it was tuned on the touchpad.
                "sens" | "sens_touchpad" => self.sens[1] = num.unwrap_or(1.0).clamp(0.1, 10.0),
                "sens_mouse" | "sens_pen" => self.sens[0] = num.unwrap_or(1.0).clamp(0.1, 10.0),
                "sens_tablet_touch" => self.sens[2] = num.unwrap_or(1.0).clamp(0.1, 10.0),
                "sens_controller" => self.sens[3] = num.unwrap_or(1.0).clamp(0.1, 10.0),
                "smooth" => self.smooth_hz = num.unwrap_or(self.smooth_hz).clamp(20.0, 400.0),
                "dir" => self.dir = if num.unwrap_or(1.0) < 0.0 { -1.0 } else { 1.0 },
                "eq" => self.eq = flag,
                "noise" => self.noise = flag,
                "invert" => self.invert = flag,
                "volume" => self.volume = num.unwrap_or(self.volume as f64).clamp(0.0, 1.5) as f32,
                "bpm" => self.bpm = num.unwrap_or(self.bpm).clamp(40.0, 220.0),
                "rpm45" => self.rpm45 = flag,
                "loop" => self.loop_section = flag,
                "mode" | "mode_touchpad" | "mode_mouse" | "mode_pen" | "mode_tablet_touch" => {
                    // The single "mode" from before per-input modes was last set on the tablet.
                    let i = match k.trim() {
                        "mode_mouse" | "mode_pen" => 0,
                        "mode_touchpad" => 1,
                        _ => 2,
                    };
                    self.mode[i] = match v.trim() {
                        "x" => Mode::LinearX,
                        "y" => Mode::LinearY,
                        "platter" => Mode::Platter,
                        _ => Mode::Auto,
                    }
                }
                _ => {}
            }
        }
        // Push the loaded values to the engine and re-press the record if EQ/noise differ.
        self.send(Msg::Smooth(self.smooth_hz));
        self.send(Msg::Volume(self.volume));
        self.send(Msg::Bpm(self.bpm));
        self.send(Msg::Rpm(if self.rpm45 { 45.0 / (100.0 / 3.0) } else { 1.0 }));
        if self.eq != old_eq || self.noise != old_noise || self.loop_section != old_loop {
            self.rebuild(true);
        }
        self.fader_open = !self.invert;
        self.send(Msg::Fader(self.fader_open));
    }

    fn profiles_dir() -> PathBuf {
        Self::settings_path().with_file_name("profiles")
    }

    fn profile_names() -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(Self::profiles_dir())
            .map(|d| d.filter_map(|e| e.ok()).filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned())).collect())
            .unwrap_or_default();
        names.sort_by_key(|n| n.to_lowercase());
        names
    }

    /// P: the profile menu, drawn over the deck.
    fn open_profile_menu(&mut self) {
        let names = Self::profile_names();
        let sel = self.profile.as_ref().and_then(|p| names.iter().position(|n| n == p)).map_or(0, |i| i + 1);
        self.menu = Some(ProfileMenu { names, sel, naming: None, confirm_delete: None, confirm_revert: false, rows: Vec::new(), panel: (0, 0, 0, 0) });
    }

    /// Enter (or a click) on the selected row.
    fn menu_activate(&mut self) {
        let Some(m) = self.menu.as_mut() else { return };
        if m.sel == 0 {
            m.naming = Some(self.profile.clone().unwrap_or_default());
        } else if let Some(name) = m.names.get(m.sel - 1).cloned() {
            let reverting = self.profile_dirty && self.profile.as_deref() == Some(name.as_str());
            if reverting && !m.confirm_revert {
                m.confirm_revert = true;
                return;
            }
            self.menu = None;
            self.profile_action(ProfileAction::Load(name));
        }
    }

    fn menu_key(&mut self, ev: &KeyEvent) {
        let Some(m) = self.menu.as_mut() else { return };
        let PhysicalKey::Code(code) = ev.physical_key else { return };
        if let Some(name) = m.naming.as_mut() {
            match code {
                KeyCode::Enter | KeyCode::NumpadEnter => {
                    let name = library::clean(name);
                    if !name.is_empty() {
                        self.menu = None;
                        self.profile_action(ProfileAction::Save(name));
                    }
                }
                KeyCode::Escape => m.naming = None,
                KeyCode::Backspace => {
                    name.pop();
                }
                _ => {
                    if let Some(t) = &ev.text {
                        for ch in t.chars().filter(|c| !c.is_control()) {
                            if name.chars().count() < 40 {
                                name.push(ch);
                            }
                        }
                    }
                }
            }
            return;
        }
        let rows = m.names.len() + 1;
        match code {
            KeyCode::Escape | KeyCode::KeyP => self.menu = None,
            KeyCode::ArrowUp => {
                m.sel = (m.sel + rows - 1) % rows;
                m.confirm_delete = None;
                m.confirm_revert = false;
            }
            KeyCode::ArrowDown => {
                m.sel = (m.sel + 1) % rows;
                m.confirm_delete = None;
                m.confirm_revert = false;
            }
            KeyCode::Enter | KeyCode::NumpadEnter => self.menu_activate(),
            KeyCode::Delete | KeyCode::Backspace if m.sel > 0 => {
                if m.confirm_delete == Some(m.sel) {
                    let name = m.names[m.sel - 1].clone();
                    self.profile_action(ProfileAction::Delete(name));
                    if let Some(m) = self.menu.as_mut() {
                        m.names = Self::profile_names();
                        m.sel = m.sel.min(m.names.len());
                        m.confirm_delete = None;
                    }
                } else {
                    m.confirm_delete = Some(m.sel);
                }
            }
            _ => {}
        }
    }

    /// Which menu row is under the pointer, if any.
    fn menu_row_at(&self, y: f64) -> Option<usize> {
        let m = self.menu.as_ref()?;
        m.rows.iter().position(|&(a, b)| y >= a as f64 && y < b as f64)
    }

    fn profile_action(&mut self, action: ProfileAction) {
        let dir = Self::profiles_dir();
        match action {
            ProfileAction::Load(name) => match std::fs::read_to_string(dir.join(format!("{name}.txt"))) {
                Ok(text) => {
                    self.apply_settings(&text);
                    self.profile = Some(name.clone());
                    self.profile_dirty = false;
                    self.say(format!("loaded profile {name}"));
                }
                Err(e) => self.say(format!("couldn't load profile: {e}")),
            },
            ProfileAction::Save(name) => {
                let _ = std::fs::create_dir_all(&dir);
                match std::fs::write(dir.join(format!("{name}.txt")), self.settings_text()) {
                    Ok(()) => {
                        self.profile = Some(name.clone());
                        self.profile_dirty = false;
                        self.say(format!("saved profile {name}"));
                    }
                    Err(e) => self.say(format!("couldn't save profile: {e}")),
                }
            }
            ProfileAction::Delete(name) => {
                let _ = std::fs::remove_file(dir.join(format!("{name}.txt")));
                if self.profile.as_deref() == Some(name.as_str()) {
                    self.profile = None;
                }
                self.say(format!("deleted profile {name}"));
            }
        }
        self.save_settings();
    }

    fn folder(&self, which: Which) -> PathBuf {
        match which {
            Which::Samples => self.samples_dir.clone(),
            Which::Takes => self.takes_dir.lock().unwrap().clone(),
        }
    }

    fn open_folder_menu(&mut self) {
        self.folders = Some(FolderMenu { stage: FolderStage::Pick, sel: 0, rows: Vec::new(), panel: (0, 0, 0, 0) });
    }

    fn set_folder(&mut self, which: Which, to: PathBuf) {
        match which {
            Which::Samples => self.samples_dir = to.clone(),
            Which::Takes => *self.takes_dir.lock().unwrap() = to.clone(),
        }
        self.save_settings();
        let name = if which == Which::Samples { "samples" } else { "takes" };
        self.say(format!("{name} folder: {}", library::pretty(&to)));
    }

    fn folder_activate(&mut self) {
        let Some(m) = self.folders.as_mut() else { return };
        let sel = m.sel;
        match &mut m.stage {
            FolderStage::Pick => {
                let which = if sel == 0 { Which::Samples } else { Which::Takes };
                // Start in the current folder, or the nearest one that exists.
                let mut dir = self.folder(which);
                while !dir.is_dir() {
                    match dir.parent() {
                        Some(p) => dir = p.to_path_buf(),
                        None => break,
                    }
                }
                let m = self.folders.as_mut().unwrap();
                m.stage = browse(which, dir);
                m.sel = 0;
            }
            FolderStage::Browse { which, dir, subdirs, naming } => {
                let (which, dir) = (*which, dir.clone());
                match sel {
                    0 => {
                        let current = self.folder(which);
                        let has_files = std::fs::read_dir(&current).map(|mut d| d.next().is_some()).unwrap_or(false);
                        if dir == current {
                            self.folders = None;
                        } else if which == Which::Takes && self.shared.recording.load(Relaxed) {
                            self.say("stop recording before changing the takes folder");
                        } else if has_files {
                            let m = self.folders.as_mut().unwrap();
                            m.stage = FolderStage::Move { which, to: dir };
                            m.sel = 0;
                        } else {
                            self.folders = None;
                            self.set_folder(which, dir);
                        }
                    }
                    1 => *naming = Some(String::new()),
                    2 => {
                        let up = dir.parent().map(Path::to_path_buf).unwrap_or(dir);
                        let m = self.folders.as_mut().unwrap();
                        m.stage = browse(which, up);
                        m.sel = 0;
                    }
                    n => {
                        let next = dir.join(&subdirs[n - 3]);
                        let m = self.folders.as_mut().unwrap();
                        m.stage = browse(which, next);
                        m.sel = 0;
                    }
                }
            }
            FolderStage::Move { which, to } => {
                let (which, to) = (*which, to.clone());
                self.folders = None;
                if sel == 1 {
                    self.set_folder(which, to);
                    return;
                }
                let from = self.folder(which);
                match library::move_contents(&from, &to) {
                    Ok((moved, skipped)) => {
                        // Takes kept inside the samples folder travel with it.
                        if which == Which::Samples {
                            let mut t = self.takes_dir.lock().unwrap();
                            if let Ok(rest) = t.strip_prefix(&from) {
                                *t = to.join(rest);
                            }
                        }
                        self.set_folder(which, to);
                        let extra = if skipped > 0 { format!(", {skipped} left behind (same name already there)") } else { String::new() };
                        self.say(format!("moved {moved} item{}{extra}", if moved == 1 { "" } else { "s" }));
                    }
                    Err(e) => self.say(format!("couldn't move files: {e:#}")),
                }
            }
        }
    }

    fn folder_key(&mut self, ev: &KeyEvent) {
        let Some(m) = self.folders.as_mut() else { return };
        let PhysicalKey::Code(code) = ev.physical_key else { return };
        if let FolderStage::Browse { which, dir, naming: Some(name), .. } = &mut m.stage {
            match code {
                KeyCode::Enter | KeyCode::NumpadEnter => {
                    let clean = library::clean(name);
                    if !clean.is_empty() {
                        let new = dir.join(&clean);
                        match std::fs::create_dir_all(&new) {
                            Ok(()) => {
                                m.stage = browse(*which, new);
                                m.sel = 0;
                            }
                            Err(e) => {
                                let msg = format!("couldn't create folder: {e}");
                                self.say(msg);
                            }
                        }
                    }
                }
                KeyCode::Escape => {
                    if let FolderStage::Browse { naming, .. } = &mut m.stage {
                        *naming = None;
                    }
                }
                KeyCode::Backspace => {
                    name.pop();
                }
                _ => {
                    if let Some(t) = &ev.text {
                        name.extend(t.chars().filter(|c| !c.is_control()));
                    }
                }
            }
            return;
        }
        let rows = m.row_count();
        match code {
            KeyCode::Escape => match m.stage {
                FolderStage::Pick => self.folders = None,
                _ => {
                    m.stage = FolderStage::Pick;
                    m.sel = 0;
                }
            },
            KeyCode::KeyF => self.folders = None,
            KeyCode::ArrowUp => m.sel = (m.sel + rows - 1) % rows,
            KeyCode::ArrowDown => m.sel = (m.sel + 1) % rows,
            KeyCode::Backspace => {
                if let FolderStage::Browse { .. } = m.stage {
                    m.sel = 2;
                    self.folder_activate();
                }
            }
            KeyCode::Enter | KeyCode::NumpadEnter => self.folder_activate(),
            _ => {}
        }
    }

    /// What a menu item currently is: a value or on/off, shown beside it.
    fn item_state(&self, code: KeyCode) -> Option<(String, u32)> {
        let on = |b: bool| Some(if b { ("on".to_string(), GREEN) } else { ("off".to_string(), DIM) });
        let kind = self.sens_kind as usize;
        match code {
            KeyCode::KeyM => on(self.motor),
            KeyCode::KeyE => Some(if self.save_mp3.load(Relaxed) { ("MP3".into(), LILAC) } else { ("WAV".into(), LILAC) }),
            KeyCode::KeyG => on(self.loop_section),
            KeyCode::KeyI => on(self.invert),
            KeyCode::KeyV => on(self.eq),
            KeyCode::KeyN => on(self.noise),
            KeyCode::KeyH => on(self.show_guide),
            KeyCode::KeyT => on(self.click),
            KeyCode::KeyB => Some(match &self.backing {
                Some(_) if self.backing_on => ("playing".into(), GREEN),
                Some(_) => ("paused".into(), WHITE),
                None => ("none loaded".into(), DIM),
            }),
            KeyCode::KeyU => on(self.backing_in_take),
            KeyCode::Digit5 | KeyCode::Digit6 => Some((format!("{:.0}%", self.backing_vol * 100.0), WHITE)),
            KeyCode::F11 => on(self.window.as_ref().is_some_and(|w| w.fullscreen().is_some())),
            KeyCode::Digit3 if !self.rpm45 => Some(("\u{f00c}".into(), GREEN)),
            KeyCode::Digit4 if self.rpm45 => Some(("\u{f00c}".into(), GREEN)),
            KeyCode::Tab => Some((format!("{} {}", kind_name(self.sens_kind), self.motion_name()), LILAC)),
            KeyCode::KeyD => Some((if self.dir > 0.0 { "+".into() } else { "-".into() }, WHITE)),
            KeyCode::BracketLeft | KeyCode::BracketRight => Some((format!("x{:.2}", self.sens[kind]), WHITE)),
            KeyCode::Minus | KeyCode::Equal => Some((format!("{:.0} Hz", self.smooth_hz), WHITE)),
            KeyCode::ArrowUp | KeyCode::ArrowDown => Some((format!("{:.0}%", self.volume * 100.0), WHITE)),
            KeyCode::Digit9 | KeyCode::Digit0 => Some((format!("{:.0} BPM", self.bpm), WHITE)),
            KeyCode::KeyR => Some(if self.shared.recording.load(Relaxed) {
                ("recording".into(), RED)
            } else if self.shared.rec_request.load(Relaxed) {
                ("armed".into(), WHITE)
            } else {
                ("off".into(), DIM)
            }),
            KeyCode::KeyP => Some(match &self.profile {
                Some(p) => (format!("{p}{}", if self.profile_dirty { "*" } else { "" }), GREEN),
                None => ("none".into(), DIM),
            }),
            _ => None,
        }
    }

    fn open_bar(&mut self, i: usize) {
        self.bar.open = Some(i);
        self.bar.sel = 0;
    }

    /// Runs a menu item: the same as pressing its key.
    fn run_item(&mut self, code: KeyCode) {
        self.bar.open = None;
        if code == KeyCode::KeyQ {
            self.quit = true;
        } else {
            self.command(code);
        }
    }

    /// Keys while a menu is open. Returns false for keys the menu doesn't use, which then
    /// close it and work as normal.
    fn bar_key(&mut self, code: KeyCode) -> bool {
        let Some(open) = self.bar.open else { return false };
        let n = MENUS.len();
        let items = MENUS[open].1;
        match code {
            KeyCode::ArrowLeft => self.open_bar((open + n - 1) % n),
            KeyCode::ArrowRight => self.open_bar((open + 1) % n),
            KeyCode::ArrowUp => self.bar.sel = (self.bar.sel + items.len() - 1) % items.len(),
            KeyCode::ArrowDown => self.bar.sel = (self.bar.sel + 1) % items.len(),
            KeyCode::Enter | KeyCode::NumpadEnter => {
                let code = items[self.bar.sel.min(items.len() - 1)].1;
                self.run_item(code);
            }
            KeyCode::Escape | KeyCode::F10 | KeyCode::AltLeft => self.bar.open = None,
            _ => {
                self.bar.open = None;
                return false;
            }
        }
        true
    }

    /// Pointer press with a menu open or on the bar. Returns true if the bar took it.
    fn bar_click(&mut self) -> bool {
        let (x, y) = self.cursor;
        if y < BAR_H as f64 {
            if let Some(i) = self.bar.titles.iter().position(|&(a, b)| x >= a as f64 && x < b as f64) {
                if self.bar.open == Some(i) {
                    self.bar.open = None;
                } else {
                    self.open_bar(i);
                }
            } else {
                self.bar.open = None;
            }
            return true;
        }
        let Some(open) = self.bar.open else { return false };
        let (dx, dy, dw, dh) = self.bar.drop;
        if x >= dx as f64 && x < (dx + dw) as f64 && y >= dy as f64 && y < (dy + dh) as f64 {
            if let Some(r) = self.bar.rows.iter().position(|&(a, b)| y >= a as f64 && y < b as f64) {
                self.run_item(MENUS[open].1[r].1);
            }
        } else {
            self.bar.open = None;
        }
        true
    }

    /// Pointer movement with a menu open: hover switches menus and highlights items.
    fn bar_hover(&mut self) {
        let (x, y) = self.cursor;
        if y < BAR_H as f64 {
            if let Some(i) = self.bar.titles.iter().position(|&(a, b)| x >= a as f64 && x < b as f64) {
                if self.bar.open != Some(i) {
                    self.open_bar(i);
                }
            }
        } else if let Some(r) = self.bar.rows.iter().position(|&(a, b)| y >= a as f64 && y < b as f64) {
            self.bar.sel = r;
        }
    }

    /// What's going into the take, shown next to REC while a backing track plays.
    fn take_contents(&self, val: impl Fn(&str, u32) -> (String, u32, bool), lab: impl Fn(&str) -> (String, u32, bool)) -> Vec<(String, u32, bool)> {
        if self.backing.is_none() || !self.backing_on {
            return Vec::new();
        }
        if self.backing_in_take {
            vec![val("scratch + backing", WHITE)]
        } else {
            vec![lab("scratch only")]
        }
    }

    /// What the motion chip says for the input used last.
    fn motion_name(&self) -> &'static str {
        if self.sens_kind == Kind::Controller {
            "JOG"
        } else {
            self.mode[self.sens_kind as usize].short()
        }
    }

    /// There are release notes newer than the ones you've seen.
    fn has_news(&self) -> bool {
        version_key(VERSION) > version_key(&self.version_seen)
    }

    /// F1 or a click on the version: the release notes, newest first, with the ones
    /// you haven't seen marked.
    fn open_whats_new(&mut self) {
        let seen = version_key(&self.version_seen);
        let mut lines = Vec::new();
        for raw in CHANGELOG.lines() {
            let line = raw.trim_end();
            if let Some(v) = line.strip_prefix("## ") {
                if !lines.is_empty() {
                    lines.push((String::new(), TEXT, false));
                }
                let ver = v.split_whitespace().next().unwrap_or("");
                let new = version_key(ver) > seen;
                let mark = if new { "  \u{f005} new" } else { "" };
                lines.push((format!("{v}{mark}"), if new { GREEN } else { LILAC }, true));
            } else if line.starts_with('#') || line.is_empty() {
                continue;
            } else {
                // Wrap to the panel; bullets keep a hanging indent.
                let (first, rest) = match line.strip_prefix("- ") {
                    Some(t) => ("\u{2022} ", t),
                    None => ("", line),
                };
                let mut cur = first.to_string();
                for word in rest.split_whitespace() {
                    if cur.chars().count() + word.chars().count() + 1 > 66 && cur.trim() != "\u{2022}" {
                        lines.push((cur, TEXT, false));
                        cur = "  ".to_string();
                    }
                    if !cur.ends_with(' ') && !cur.is_empty() {
                        cur.push(' ');
                    }
                    cur.push_str(word);
                }
                lines.push((cur, TEXT, false));
            }
        }
        self.whats_new = Some(WhatsNew { lines, top: 0, rows: Vec::new(), panel: (0, 0, 0, 0) });
        if self.has_news() {
            self.version_seen = VERSION.to_string();
            self.save_settings();
        }
    }

    fn whats_new_key(&mut self, ev: &KeyEvent) {
        let Some(m) = self.whats_new.as_mut() else { return };
        let PhysicalKey::Code(code) = ev.physical_key else { return };
        let last = m.lines.len().saturating_sub(1);
        match code {
            KeyCode::Escape | KeyCode::F1 | KeyCode::Enter | KeyCode::NumpadEnter => self.whats_new = None,
            KeyCode::ArrowUp => m.top = m.top.saturating_sub(1),
            KeyCode::ArrowDown => m.top = (m.top + 1).min(last),
            KeyCode::PageUp => m.top = m.top.saturating_sub(10),
            KeyCode::PageDown => m.top = (m.top + 10).min(last),
            _ => {}
        }
    }

    fn draw_whats_new(&self, c: &mut Canvas, f: &mut text::Fonts, m: &mut WhatsNew) {
        // Same page size as draw_panel; hand it just the page so nothing is highlighted.
        let shown = ((c.h as i32 - 200) / 36).max(3) as usize;
        m.top = m.top.min(m.lines.len().saturating_sub(shown));
        let rows: Vec<Row> = m.lines.iter().skip(m.top).take(shown).map(|(t, col, b)| Row::new(t, *col, *b)).collect();
        let more = if m.top + shown < m.lines.len() { "\u{2193} more" } else { "" };
        draw_panel(
            c,
            f,
            "\u{f005}  WHAT'S NEW",
            Some((format!("CUT {VERSION}  {more}"), GREEN)),
            &rows,
            usize::MAX,
            &[("\u{2191}\u{2193}", "scroll"), ("Esc", "close")],
            &mut m.rows,
            &mut m.panel,
        );
    }

    fn say(&mut self, s: impl Into<String>) {
        self.note = Some((s.into(), Instant::now()));
        self.dirty = true;
    }

    fn rebuild(&mut self, keep_pos: bool) {
        let rec = Arc::new(deck::build(&self.src, self.region.0, self.region.1, self.eq, self.noise, self.loop_section));
        self.rec = rec.clone();
        self.send(Msg::SetRecord(rec, keep_pos));
    }

    fn load_path(&mut self, p: &Path) {
        match deck::load(p) {
            Ok(src) => {
                self.region = (0, src.len());
                self.src = src;
                self.path = Some(p.to_path_buf());
                self.peaks.clear();
                self.view = (0.0, self.src.len() as f64);
                self.rebuild(false);
                if let Some(w) = &self.window {
                    w.set_title(&format!("CUT - {}", self.src.name));
                }
                self.say(format!("loaded {}", self.src.name));
            }
            Err(e) => self.say(format!("couldn't load: {e:#}")),
        }
    }

    fn scroll_active(&self) -> bool {
        self.scroll_last.is_some_and(|t| t.elapsed() < SCROLL_HOLD)
    }

    fn update_hand(&mut self) {
        let t = self.now_us();
        self.update_hand_at(t);
    }

    fn update_hand_at(&mut self, t: u64) {
        let want = self.mouse_scratch || self.shift || self.scroll_active() || self.direct.is_some() || !self.dev_hold.is_empty() || self.jog_active();
        if want && !self.hand {
            self.hand = true;
            self.hand_u = 0.0;
            self.reset_stroke_axis();
            self.send(Msg::HandDown { t });
        } else if !want && self.hand {
            self.hand = false;
            self.send(Msg::HandUp);
        }
    }

    fn move_hand(&mut self, du: f64) {
        let t = self.now_us();
        self.move_hand_at(du, t);
    }

    fn move_hand_at(&mut self, du: f64, t: u64) {
        if !self.hand || du == 0.0 {
            return;
        }
        self.hand_u += du * self.dir;
        let u = self.hand_u;
        self.send(Msg::HandMove { t, u });
    }

    /// Motion along the main axis of your recent strokes (~150 ms of real movement).
    /// `mm` is millimetres per input unit. Right/up is always forward, like the linear
    /// modes; only for strokes near the upper-left/lower-right diagonal, where that rule
    /// can't decide, does the axis keep whichever sign it already had.
    fn along_stroke(&mut self, dx: f64, dy: f64, mm: f64) -> f64 {
        let dist = dx.hypot(dy) * mm;
        // Finger jitter while holding still says nothing about stroke direction.
        if dist > 0.04 {
            let (xx, yy, xy) = self.stroke_cov;
            let k = 0.93;
            self.stroke_cov = (xx * k + dx * dx, yy * k + dy * dy, xy * k + dx * dy);
            let (xx, yy, xy) = self.stroke_cov;
            let th = 0.5 * (2.0 * xy).atan2(xx - yy);
            let (mut ax, mut ay) = (th.cos(), th.sin());
            let toward_up_right = (ax - ay) * std::f64::consts::FRAC_1_SQRT_2;
            let flip = if toward_up_right.abs() > 0.35 {
                toward_up_right < 0.0
            } else {
                ax * self.stroke_axis.0 + ay * self.stroke_axis.1 < 0.0
            };
            if flip {
                ax = -ax;
                ay = -ay;
            }
            self.stroke_axis = (ax, ay);
        }
        dx * self.stroke_axis.0 + dy * self.stroke_axis.1
    }

    fn reset_stroke_axis(&mut self) {
        self.stroke_cov = (0.0, 0.0, 0.0);
        self.stroke_axis = (1.0, 0.0);
    }

    fn over_overview(&self) -> bool {
        let (ox, oy, ow, oh) = self.lay.overview;
        let (x, y) = self.cursor;
        x >= ox as f64 && x < (ox + ow) as f64 && y >= oy as f64 && y < (oy + oh) as f64
    }

    fn direct_event(&mut self, ev: InEvent) {
        match ev {
            InEvent::Down { kind, t, x, y, fingers, .. } => {
                // Touches while you're in another window aren't for us.
                if !self.focused || self.overlay_open() || self.bar.open.is_some() {
                    return;
                }
                // Two fingers over the top waveform zoom it (as scrolling) rather than scratch.
                if kind == Kind::Touchpad && self.over_overview() {
                    return;
                }
                if self.direct.is_some() {
                    return;
                }
                self.direct = Some(kind);
                // One finger on the touchpad still moves the cursor; two pause it for scratching.
                if kind == Kind::Touchpad {
                    self.desk.pause_touchpad(fingers >= 2);
                }
                self.sens_kind = kind;
                self.direct_prev = (x, y);
                self.direct_angle = y.atan2(x);
                self.update_hand_at(t);
            }
            InEvent::Move { kind, t, x, y, fingers, .. } => {
                if self.direct != Some(kind) {
                    return;
                }
                // One finger on the touchpad only holds the record (a tap is a quick stop);
                // its movement is the cursor's.
                if kind == Kind::Touchpad && fingers < 2 {
                    self.direct_prev = (x, y);
                    self.direct_angle = y.atan2(x);
                    return;
                }
                let (dx, dy) = (x - self.direct_prev.0, y - self.direct_prev.1);
                self.direct_prev = (x, y);
                let ang = y.atan2(x);
                let da = wrap_angle(ang - self.direct_angle);
                self.direct_angle = ang;
                let mm_per_sec = match kind {
                    Kind::Mouse | Kind::Touchpad | Kind::Controller => TOUCH_MM_PER_SEC,
                    Kind::TabletTouch => TABLET_TOUCH_MM_PER_SEC,
                };
                let sens = self.sens[kind as usize];
                let k = self.rec.rate * sens / mm_per_sec;
                let du = match self.mode[kind as usize] {
                    Mode::Auto => self.along_stroke(dx, dy, 1.0) * k,
                    Mode::LinearX => dx * k,
                    Mode::LinearY => -dy * k,
                    // Near the centre (the spindle) the angle swings wildly for tiny moves.
                    Mode::Platter if x.hypot(y) < 8.0 => 0.0,
                    Mode::Platter => da / std::f64::consts::TAU * SEC_PER_REV * self.rec.rate * sens,
                };
                self.move_hand_at(du, t);
            }
            InEvent::Anchor { kind, x, y, fingers, .. } => {
                if self.direct == Some(kind) {
                    if kind == Kind::Touchpad {
                        self.desk.pause_touchpad(fingers >= 2);
                    }
                    self.direct_prev = (x, y);
                    self.direct_angle = y.atan2(x);
                }
            }
            InEvent::Up { kind, .. } => {
                if kind == Kind::Touchpad {
                    self.desk.pause_touchpad(false);
                }
                if self.direct == Some(kind) {
                    self.direct = None;
                    self.update_hand();
                }
            }
            _ => {}
        }
    }

    fn update_fader(&mut self) {
        let any = !self.fader_keys.is_empty() || !self.dev_fader.is_empty();
        let open = if self.invert { any } else { !any };
        if open != self.fader_open {
            self.fader_open = open;
            self.send(Msg::Fader(open));
        }
    }

    fn px_to_samples(&self) -> f64 {
        self.rec.rate * self.sens[Kind::Mouse as usize] / PX_PER_SEC
    }

    fn cursor_moved(&mut self, x: f64, y: f64) {
        if self.bar.open.is_some() {
            self.cursor = (x, y);
            self.bar_hover();
            return;
        }
        if self.whats_new.is_some() {
            self.cursor = (x, y);
            return;
        }
        if let Some(m) = self.dev_menu.as_mut() {
            self.cursor = (x, y);
            if !matches!(m.stage, devmenu::Stage::Learn(_)) {
                if let Some(r) = m.rows.iter().position(|&(a, b)| y >= a as f64 && y < b as f64) {
                    m.sel = r;
                }
            }
            return;
        }
        if let Some(m) = self.folders.as_mut() {
            self.cursor = (x, y);
            let naming = matches!(m.stage, FolderStage::Browse { naming: Some(_), .. });
            if let Some(r) = m.rows.iter().position(|&(a, b)| y >= a as f64 && y < b as f64) {
                if !naming {
                    m.sel = r;
                }
            }
            return;
        }
        if self.menu.is_some() {
            self.cursor = (x, y);
            if let Some(r) = self.menu_row_at(y) {
                if let Some(m) = self.menu.as_mut().filter(|m| m.naming.is_none() && m.sel != r) {
                    m.sel = r;
                    m.confirm_delete = None;
                    m.confirm_revert = false;
                }
            }
            return;
        }
        let (dx, dy) = (x - self.cursor.0, y - self.cursor.1);
        self.cursor = (x, y);
        let lay = self.lay;
        let (cx, cy, _) = lay.platter;
        let ang = (y - cy).atan2(x - cx);
        let da = wrap_angle(ang - self.last_angle);
        self.last_angle = ang;
        // Direct touch input drives the record itself; don't double it with the pointer.
        if self.region_drag.is_some() || self.direct.is_some() {
            return;
        }
        let du = match self.mode[Kind::Mouse as usize] {
            Mode::Auto => self.along_stroke(dx, dy, 0.12) * self.px_to_samples(),
            Mode::LinearX => dx * self.px_to_samples(),
            Mode::LinearY => -dy * self.px_to_samples(),
            Mode::Platter => da / std::f64::consts::TAU * SEC_PER_REV * self.rec.rate * self.sens[Kind::Mouse as usize],
        };
        self.move_hand(du);
    }

    fn scrolled(&mut self, dx: f64, dy: f64) {
        // With the touchpad read directly, its two-finger scrolls are already handled.
        if self.in_status.touch.load(Relaxed) {
            return;
        }
        self.scroll_last = Some(Instant::now());
        self.update_hand();
        let d = if self.mode[Kind::Touchpad as usize] == Mode::LinearX { dx } else { dy };
        let du = d * self.px_to_samples();
        self.move_hand(du);
    }

    fn overview_x_to_sample(&self, x: f64) -> usize {
        let (ox, _, ow, _) = self.lay.overview;
        let t = ((x - ox as f64) / ow as f64).clamp(0.0, 1.0);
        (self.view.0 + t * (self.view.1 - self.view.0)).round().clamp(0.0, self.src.len() as f64) as usize
    }

    fn overview_sample_to_x(&self, s: f64) -> f64 {
        let (ox, _, ow, _) = self.lay.overview;
        ox as f64 + (s - self.view.0) / (self.view.1 - self.view.0) * ow as f64
    }

    /// Zoom the top waveform by `factor` (<1 = in), keeping the sample under `x` in place.
    fn zoom_overview(&mut self, factor: f64, x: f64) {
        let len = self.src.len() as f64;
        let (v0, v1) = self.view;
        let at = self.overview_x_to_sample(x) as f64;
        // Down to 1/20 s across the whole width, which is a few samples per pixel.
        let width = ((v1 - v0) * factor).clamp((self.src.rate as f64 / 20.0).min(len), len);
        let t = (at - v0) / (v1 - v0);
        let start = (at - t * width).clamp(0.0, len - width);
        self.view = (start, start + width);
    }

    fn pan_overview(&mut self, px: f64) {
        let len = self.src.len() as f64;
        let (v0, v1) = self.view;
        let ow = self.lay.overview.2.max(1) as f64;
        let shift = px / ow * (v1 - v0);
        let start = (v0 + shift).clamp(0.0, len - (v1 - v0));
        self.view = (start, start + (v1 - v0));
    }

    /// Z: zoom in on the section, or back out to the whole file.
    fn toggle_zoom(&mut self) {
        let len = self.src.len() as f64;
        if self.view.1 - self.view.0 < len - 1.0 {
            self.view = (0.0, len);
        } else {
            let (a, b) = (self.region.0 as f64, self.region.1 as f64);
            let pad = ((b - a) * 0.15).max(self.src.rate as f64 * 0.02);
            let width = (b - a + 2.0 * pad).clamp((self.src.rate as f64 / 20.0).min(len), len);
            let start = ((a + b) / 2.0 - width / 2.0).clamp(0.0, len - width);
            self.view = (start, start + width);
        }
    }

    fn mouse_button(&mut self, button: MouseButton, pressed: bool) {
        if self.whats_new.is_some() {
            if pressed && button == MouseButton::Left {
                self.whats_new = None;
            }
            return;
        }
        if self.dev_menu.is_some() {
            if pressed && button == MouseButton::Left {
                self.dev_click();
            }
            return;
        }
        if pressed && button == MouseButton::Left && self.bar.open.is_none() && self.menu.is_none() && self.folders.is_none() {
            let (bx, by, bw, bh) = self.version_box;
            let (x, y) = self.cursor;
            if bw > 0 && x >= bx as f64 && x < (bx + bw) as f64 && y >= by as f64 && y < (by + bh) as f64 {
                self.open_whats_new();
                return;
            }
        }
        if self.menu.is_none() && self.folders.is_none() {
            if pressed && button == MouseButton::Left && self.bar_click() {
                return;
            }
            if self.bar.open.is_some() {
                return;
            }
        }
        if let Some(m) = &self.folders {
            if pressed && button == MouseButton::Left {
                let (px, py, pw, ph) = m.panel;
                let (x, y) = self.cursor;
                let inside = x >= px as f64 && x < (px + pw) as f64 && y >= py as f64 && y < (py + ph) as f64;
                let row = m.rows.iter().position(|&(a, b)| y >= a as f64 && y < b as f64);
                match row {
                    Some(r) if inside => {
                        if let Some(m) = self.folders.as_mut() {
                            m.sel = r;
                        }
                        self.folder_activate();
                    }
                    _ if !inside => self.folders = None,
                    _ => {}
                }
            }
            return;
        }
        if let Some(m) = &self.menu {
            if pressed && button == MouseButton::Left {
                let (px, py, pw, ph) = m.panel;
                let (x, y) = self.cursor;
                let inside = x >= px as f64 && x < (px + pw) as f64 && y >= py as f64 && y < (py + ph) as f64;
                match self.menu_row_at(y) {
                    Some(r) if inside => {
                        if let Some(m) = self.menu.as_mut() {
                            m.sel = r;
                        }
                        self.menu_activate();
                    }
                    _ if !inside => self.menu = None,
                    _ => {}
                }
            }
            return;
        }
        match button {
            MouseButton::Left => {
                if pressed {
                    let (ox, oy, ow, oh) = self.lay.overview;
                    let (x, y) = self.cursor;
                    if x >= ox as f64 && x < (ox + ow) as f64 && y >= oy as f64 && y < (oy + oh) as f64 {
                        // Grabbing near an edge of the current section moves that edge;
                        // anywhere else starts a new selection.
                        let ex0 = self.overview_sample_to_x(self.region.0 as f64);
                        let ex1 = self.overview_sample_to_x(self.region.1 as f64);
                        let anchor = if (x - ex0).abs() <= 6.0 {
                            ex1
                        } else if (x - ex1).abs() <= 6.0 {
                            ex0
                        } else {
                            x
                        };
                        self.region_drag = Some(anchor);
                        return;
                    }
                    self.mouse_scratch = true;
                    self.sens_kind = Kind::Mouse;
                } else if let Some(x0) = self.region_drag.take() {
                    let (a, b) = (self.overview_x_to_sample(x0), self.overview_x_to_sample(self.cursor.0));
                    let (a, b) = (a.min(b), a.max(b));
                    if b - a > (self.src.rate as usize) / 100 {
                        self.region = (a, b);
                        self.rebuild(false);
                        self.say(format!("section {:.3}s - {:.3}s", a as f64 / self.src.rate as f64, b as f64 / self.src.rate as f64));
                    }
                    return;
                } else {
                    self.mouse_scratch = false;
                }
                self.update_hand();
            }
            _ => {}
        }
    }

    fn key(&mut self, code: KeyCode, pressed: bool, repeat: bool) {
        match code {
            KeyCode::Space | KeyCode::KeyJ | KeyCode::KeyK | KeyCode::KeyL => {
                if pressed {
                    self.fader_keys.insert(code);
                } else {
                    self.fader_keys.remove(&code);
                }
                self.update_fader();
                return;
            }
            KeyCode::ShiftLeft | KeyCode::ShiftRight => {
                self.shift = pressed;
                self.update_hand();
                return;
            }
            _ => {}
        }
        if !pressed || repeat {
            return;
        }
        self.command(code);
    }

    /// Runs a key's command (from the keyboard or a menu), then saves the settings and
    /// marks the profile changed if they moved.
    fn command(&mut self, code: KeyCode) {
        let before = self.settings_text();
        self.handle_command(code);
        if self.settings_text() != before {
            self.profile_dirty = true;
        }
        self.save_settings();
    }

    fn handle_command(&mut self, code: KeyCode) {
        match code {
            KeyCode::KeyM | KeyCode::Enter => {
                self.motor = !self.motor;
                self.send(Msg::Motor(self.motor));
            }
            KeyCode::KeyC | KeyCode::Backspace => {
                let cue = self.rec.cue;
                self.send(Msg::Seek(cue));
            }
            KeyCode::Tab if self.sens_kind == Kind::Controller => {
                self.say("controllers turn the record directly - Tab is for touch surfaces and the mouse");
            }
            KeyCode::Tab => {
                // Changes the motion for whichever surface you used last.
                let m = &mut self.mode[self.sens_kind as usize];
                *m = match *m {
                    Mode::Auto => Mode::LinearX,
                    Mode::LinearX => Mode::LinearY,
                    Mode::LinearY => Mode::Platter,
                    Mode::Platter => Mode::Auto,
                };
                let m = *m;
                self.say(format!("{}: {}", kind_name(self.sens_kind), m.name()));
            }
            KeyCode::KeyD => self.dir = -self.dir,
            KeyCode::BracketLeft | KeyCode::BracketRight => {
                let s = &mut self.sens[self.sens_kind as usize];
                *s = if code == KeyCode::BracketLeft { (*s / 1.15).max(0.1) } else { (*s * 1.15).min(10.0) };
            }
            KeyCode::Minus => {
                self.smooth_hz = (self.smooth_hz / 1.25).max(20.0);
                self.send(Msg::Smooth(self.smooth_hz));
            }
            KeyCode::Equal => {
                self.smooth_hz = (self.smooth_hz * 1.25).min(400.0);
                self.send(Msg::Smooth(self.smooth_hz));
            }
            KeyCode::KeyV => {
                self.eq = !self.eq;
                self.rebuild(true);
            }
            KeyCode::KeyN => {
                self.noise = !self.noise;
                self.rebuild(true);
            }
            KeyCode::KeyI => {
                self.invert = !self.invert;
                self.update_fader();
            }
            KeyCode::KeyR => {
                let on = !self.shared.rec_request.load(Relaxed);
                self.shared.rec_request.store(on, Relaxed);
                let what = match (self.backing.is_some() && self.backing_on, self.backing_in_take) {
                    (true, true) => " - recording scratch + backing track",
                    (true, false) => " - recording the scratch only (U adds the backing track)",
                    _ => "",
                };
                if on && self.click {
                    self.say(format!("take armed - starts on the next bar{what}"));
                } else if on && !what.is_empty() {
                    self.say(format!("take started{what}"));
                }
            }
            KeyCode::KeyT => {
                self.click = !self.click;
                self.send(Msg::Metronome(self.click));
            }
            KeyCode::Digit9 | KeyCode::Digit0 => {
                let d = if code == KeyCode::Digit0 { 1.0 } else { -1.0 };
                let step = if self.ctrl { 10.0 } else { 1.0 };
                self.bpm = (self.bpm + d * step).clamp(40.0, 220.0);
                self.send(Msg::Bpm(self.bpm));
            }
            KeyCode::KeyO => self.open_dialog(),
            KeyCode::KeyS => self.save_dialog(),
            KeyCode::KeyP => self.open_profile_menu(),
            KeyCode::KeyH => self.show_guide = !self.show_guide,
            KeyCode::KeyF => self.open_folder_menu(),
            KeyCode::KeyX => self.open_dev_menu(),
            KeyCode::KeyY => self.backing_dialog(),
            KeyCode::KeyB => {
                if self.backing.is_none() {
                    self.backing_dialog();
                } else {
                    self.backing_on = !self.backing_on;
                    self.send(Msg::BackingPlay(self.backing_on));
                }
            }
            KeyCode::Digit5 | KeyCode::Digit6 => {
                let d = if code == KeyCode::Digit6 { 0.05 } else { -0.05 };
                self.backing_vol = (self.backing_vol + d).clamp(0.0, 1.5);
                self.send(Msg::BackingVolume(self.backing_vol));
            }
            KeyCode::KeyU => {
                self.backing_in_take = !self.backing_in_take;
                self.send(Msg::BackingInTake(self.backing_in_take));
                self.say(if self.backing_in_take { "takes record the full mix: scratch + backing track" } else { "takes record the scratch alone" });
            }
            KeyCode::F1 => self.open_whats_new(),
            KeyCode::KeyZ => self.toggle_zoom(),
            KeyCode::KeyE => {
                let mp3 = !self.save_mp3.load(Relaxed);
                self.save_mp3.store(mp3, Relaxed);
                self.say(if mp3 { "save format: MP3 (320 kbps) - used next time you save or record" } else { "save format: WAV - used next time you save or record" });
            }
            KeyCode::KeyG => {
                self.loop_section = !self.loop_section;
                self.rebuild(true);
                self.say(if self.loop_section { "section loops (no lead-in or run-out)" } else { "section plays once, with lead-in and run-out" });
            }
            KeyCode::Comma => self.step_file(-1),
            KeyCode::Period => self.step_file(1),
            KeyCode::KeyA => {
                self.region = (0, self.src.len());
                self.rebuild(false);
                self.say("whole sample");
            }
            KeyCode::Digit3 => {
                self.rpm45 = false;
                self.send(Msg::Rpm(1.0));
            }
            KeyCode::Digit4 => {
                self.rpm45 = true;
                self.send(Msg::Rpm(45.0 / (100.0 / 3.0)));
            }
            KeyCode::ArrowUp => {
                self.volume = (self.volume + 0.05).min(1.5);
                self.send(Msg::Volume(self.volume));
            }
            KeyCode::ArrowDown => {
                self.volume = (self.volume - 0.05).max(0.0);
                self.send(Msg::Volume(self.volume));
            }
            KeyCode::F11 => {
                if let Some(w) = &self.window {
                    w.set_fullscreen(if w.fullscreen().is_some() { None } else { Some(Fullscreen::Borderless(None)) });
                }
            }
            KeyCode::KeyQ if self.ctrl => self.quit = true,
            _ => {}
        }
    }

    /// Asks for a name and folder, then writes the selected section (dry, as heard at
    /// normal speed) into the sample library.
    fn save_dialog(&mut self) {
        if self.src.name.is_empty() {
            self.say("load a sample first");
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.save_rx = Some(rx);
        std::thread::spawn(move || {
            let out = std::process::Command::new("zenity")
                .args([
                    "--forms",
                    "--title=Save section to library",
                    "--text=Blank name = automatic. Type a new folder name to create one.",
                    "--add-entry=Name",
                    "--add-combo=Folder",
                    &format!("--combo-values={}", library::FOLDERS),
                    "--add-entry=...or new folder",
                ])
                .output();
            let res = out.ok().filter(|o| o.status.success()).map(|o| {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                let mut it = s.split('|').map(String::from);
                (it.next().unwrap_or_default(), it.next().unwrap_or_default(), it.next().unwrap_or_default())
            });
            let _ = tx.send(res);
        });
    }

    fn save_section(&mut self, name: &str, folder: &str, new_folder: &str) {
        let rate = self.src.rate as f64;
        let (a, b) = self.region;
        let stem = Path::new(&self.src.name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let name = match library::clean(name) {
            n if n.is_empty() => library::clean(&format!("{stem} {:.2}-{:.2}s", a as f64 / rate, b as f64 / rate)),
            n => n,
        };
        let folder = [new_folder, folder].iter().map(|f| library::clean(f)).find(|f| !f.is_empty()).unwrap_or_else(|| "Other".into());
        let dir = self.samples_dir.join(&folder);
        let _ = std::fs::create_dir_all(&dir);
        let mp3 = self.save_mp3.load(Relaxed);
        let path = library::unique(&dir, &name, if mp3 { "mp3" } else { "wav" });
        let fade = ((0.002 * rate) as usize).min((b - a) / 2).max(1);
        let ramp = |i: usize| (i.min(b - a - 1 - i) as f32 / fade as f32).min(1.0);
        let l: Vec<f32> = (a..b).map(|i| self.src.l[i] * ramp(i - a)).collect();
        let r: Vec<f32> = (a..b).map(|i| self.src.r[i] * ramp(i - a)).collect();
        let saved = if mp3 { library::save_mp3(&path, &l, &r, self.src.rate) } else { library::save_wav(&path, &l, &r, self.src.rate) };
        match saved {
            Ok(()) => self.say(format!("saved {}", path.display())),
            Err(e) => self.say(format!("save failed: {e:#}")),
        }
    }

    fn step_file(&mut self, step: isize) {
        match self.path.as_deref().and_then(|p| library::neighbour(p, step)) {
            Some(p) => self.load_path(&p),
            None => self.say("no other samples in this folder"),
        }
    }

    fn open_dialog(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.open_rx = Some(rx);
        let start = match self.path.as_deref().and_then(Path::parent) {
            Some(d) => d.to_path_buf(),
            None => self.samples_dir.clone(),
        };
        std::thread::spawn(move || {
            let out = std::process::Command::new("zenity")
                .args([
                    "--file-selection",
                    "--title=Load a sample",
                    &format!("--filename={}/", start.display()),
                    "--file-filter=Audio | *.wav *.WAV *.mp3 *.flac *.ogg *.aif *.aiff *.m4a",
                    "--file-filter=All files | *",
                ])
                .output();
            let path = out.ok().filter(|o| o.status.success()).map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));
            let _ = tx.send(path);
        });
    }

    /// Y: choose a backing track. It loads in the background (long tracks take a moment).
    fn backing_dialog(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.backing_rx = Some(rx);
        let start = match self.backing_path.as_deref().and_then(Path::parent) {
            Some(d) => d.to_path_buf(),
            None => self.samples_dir.clone(),
        };
        std::thread::spawn(move || {
            let out = std::process::Command::new("zenity")
                .args([
                    "--file-selection",
                    "--title=Load a backing track",
                    &format!("--filename={}/", start.display()),
                    "--file-filter=Audio | *.wav *.WAV *.mp3 *.flac *.ogg *.aif *.aiff *.m4a",
                    "--file-filter=All files | *",
                ])
                .output();
            let path = out.ok().filter(|o| o.status.success()).map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));
            let _ = tx.send(path.map(|p| {
                let src = deck::load(&p).map_err(|e| format!("{e:#}"));
                (p, src)
            }));
        });
    }

    /// Loads the last backing track at start-up, paused.
    fn reload_backing(&mut self) {
        let Some(p) = self.backing_path.clone().filter(|p| p.is_file()) else { return };
        let (tx, rx) = mpsc::channel();
        self.backing_rx = Some(rx);
        std::thread::spawn(move || {
            let src = deck::load(&p).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Some((p, src)));
        });
    }

    fn backing_loaded(&mut self, path: PathBuf, src: Result<Source, String>) {
        match src {
            Ok(src) => {
                let name = src.name.clone();
                let arc = Arc::new(src);
                self.send(Msg::Backing(Some(arc.clone())));
                self.send(Msg::BackingVolume(self.backing_vol));
                self.send(Msg::BackingInTake(self.backing_in_take));
                self.backing_prev = self.backing.replace(arc);
                self.backing_path = Some(path);
                self.say(format!("backing track: {name} - B to play. It restarts with each take"));
                self.save_settings();
            }
            Err(e) => self.say(format!("couldn't load backing track: {e}")),
        }
        self.dirty = true;
    }

    /// Everything on screen that changes without input: needle position, level meter,
    /// take timer, whether a message is showing.
    fn frame_sig(&self) -> (u64, u32, u64, bool) {
        let level = f32::from_bits(self.shared.level.load(Relaxed));
        let rec = if self.shared.recording.load(Relaxed) {
            1 + self.shared.rec_frames.load(Relaxed) / self.out_rate as u64
        } else {
            self.shared.rec_request.load(Relaxed) as u64
        };
        let note = self.note.as_ref().is_some_and(|(_, t)| t.elapsed() < Duration::from_secs(4));
        (self.shared.pos().to_bits(), (level * 70.0) as u32, rec, note)
    }

    fn draw(&mut self) {
        self.last_sig = self.frame_sig();
        // Clicks, keys, resizes and open menus need everything redrawn; touch and the
        // engine only move the record, needle, waveforms and status.
        let overlay = self.overlay_open() || self.bar.open.is_some();
        let full = self.dirty || overlay;
        self.dirty = false;
        self.moved = false;
        let Some(win) = self.window.clone() else { return };
        let Some(mut surface) = self.surface.take() else { return };
        let size = win.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        if surface.resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap()).is_ok() {
            if let Ok(mut buf) = surface.buffer_mut() {
                // Age 1 = the buffer still holds the last frame, so a partial repaint is safe.
                let partial = !full && buf.age() == 1;
                let mut c = Canvas { px: &mut buf, w: w as usize, h: h as usize };
                let t0 = Instant::now();
                self.paint(&mut c, partial);
                let t1 = Instant::now();
                let _ = buf.present();
                if std::env::var_os("CUT_PROFILE").is_some() {
                    eprintln!("paint {:.2} ms  present {:.2} ms", (t1 - t0).as_secs_f64() * 1e3, t1.elapsed().as_secs_f64() * 1e3);
                }
            }
        }
        self.surface = Some(surface);
    }

    /// `partial`: only the moving parts (waveforms, record, status bar) are repainted over
    /// the previous frame; the menu bar, key guide and background are left as they were.
    fn paint(&mut self, c: &mut Canvas, partial: bool) {
        let mut f = std::mem::take(&mut self.fonts);
        self.paint_with(c, &mut f, partial);
        self.fonts = f;
    }

    /// The menu bar along the top.
    fn draw_bar(&mut self, c: &mut Canvas, f: &mut text::Fonts) {
        let w = c.w as i32;
        c.rect(0, 0, w, BAR_H, BAR_BG);
        c.rect(0, BAR_H - 1, w, 1, EDGE);
        let mut x = 14;
        c.image(x, (BAR_H - ICON_PX as i32) / 2, ICON_PX, &self.icon);
        x += ICON_PX as i32 + 8;
        x += f.draw(c, x, (BAR_H - 20) / 2, "CUT", LILAC, 17.0, true) + 18;
        self.bar.titles.clear();
        for (i, (title, _)) in MENUS.iter().enumerate() {
            let tw = f.width(title, 14.0, true) + 24;
            let open = self.bar.open == Some(i);
            if open {
                c.rect(x, 0, tw, BAR_H - 1, 0x241240);
                c.rect(x, BAR_H - 3, tw, 2, GREEN);
            }
            f.draw(c, x + 12, (BAR_H - 17) / 2, title, if open { WHITE } else { TEXT }, 14.0, true);
            self.bar.titles.push((x, x + tw));
            x += tw;
        }
        // Where the keyboard ways in are, if there's room.
        let hints = [("Alt", "menu"), ("H", "keys")];
        let hw: i32 = hints.iter().map(|(k, d)| f.width(k, 13.0, true) + 12 + 6 + f.width(d, 13.0, false) + 14).sum();
        if x + hw + 20 < w {
            let mut hx = w - 14 - hw + 14;
            for (k, d) in hints {
                let kw = f.width(k, 13.0, true) + 12;
                c.boxed(hx, (BAR_H - 22) / 2, kw, 22, 0x0d2410, 0x1d5a22);
                f.draw(c, hx + 6, (BAR_H - 22) / 2 + 3, k, GREEN, 13.0, true);
                hx += kw + 6;
                hx += f.draw(c, hx, (BAR_H - 22) / 2 + 3, d, LABEL_TEXT, 13.0, false) + 14;
            }
        }
    }

    /// The open menu's dropdown, drawn over everything else.
    fn draw_dropdown(&mut self, c: &mut Canvas, f: &mut text::Fonts) {
        let Some(open) = self.bar.open else { return };
        let items = MENUS[open].1;
        let (w, h) = (c.w as i32, c.h as i32);
        let px = 15.0;
        let row_h = 30;
        let states: Vec<Option<(String, u32)>> = items.iter().map(|(_, k, _)| self.item_state(*k)).collect();
        let lw = items.iter().map(|(l, _, _)| f.width(l, px, false)).max().unwrap_or(0);
        let sw = states.iter().map(|s| s.as_ref().map_or(0, |(t, _)| f.width(t, 13.0, false))).max().unwrap_or(0);
        let kw = items.iter().map(|(_, _, k)| f.width(k, 13.0, true)).max().unwrap_or(0) + 12;
        let dw = (16 + lw + 24 + sw + 16 + kw + 14).min(w - 16);
        let dh = (items.len() as i32 * row_h + 12).min(h - BAR_H - 8);
        let dx = self.bar.titles[open].0.min(w - 8 - dw).max(8);
        let dy = BAR_H;
        self.bar.drop = (dx, dy, dw, dh);
        c.boxed(dx, dy, dw, dh, 0x14141b, PURPLE);
        self.bar.rows.clear();
        let mut y = dy + 6;
        for (i, ((label, _, key), state)) in items.iter().zip(&states).enumerate() {
            self.bar.rows.push((y, y + row_h));
            let sel = i == self.bar.sel;
            if sel {
                c.rect(dx + 4, y + 1, dw - 8, row_h - 2, 0x241240);
                c.rect(dx + 4, y + 1, 3, row_h - 2, GREEN);
            }
            f.draw(c, dx + 16, y + (row_h - 19) / 2, label, if sel { WHITE } else { TEXT }, px, false);
            let kx = dx + dw - 14 - kw;
            c.boxed(kx, y + 4, kw, row_h - 8, 0x0d2410, 0x1d5a22);
            let tw = f.width(key, 13.0, true);
            f.draw(c, kx + (kw - tw) / 2, y + (row_h - 17) / 2, key, GREEN, 13.0, true);
            if let Some((t, col)) = state {
                let tw = f.width(t, 13.0, false);
                f.draw(c, kx - 14 - tw, y + (row_h - 17) / 2, t, *col, 13.0, false);
            }
            y += row_h;
        }
    }

    /// One line along the bottom: transport on the left, the sample in the middle,
    /// input / profile / level on the right. When narrow it drops the least important
    /// pieces rather than wrapping.
    fn draw_status(&mut self, c: &mut Canvas, f: &mut text::Fonts, speed: f64, level: f32) {
        let (w, h) = (c.w as i32, c.h as i32);
        let y0 = h - STATUS_H;
        c.rect(0, y0, w, STATUS_H, BAR_BG);
        c.rect(0, y0, w, 1, EDGE);
        // (pieces of (text, colour, bold), priority: higher survives longer)
        type Seg = (Vec<(String, u32, bool)>, u8);
        let lab = |t: &str| (t.to_string(), LABEL_TEXT, false);
        let val = |t: &str, col: u32| (t.to_string(), col, true);
        let mut left: Vec<Seg> = Vec::new();
        let sc = if speed > 0.02 { GREEN } else if speed < -0.02 { WHITE } else { DIM };
        left.push((vec![val(&format!("{:+.2}x", speed), sc)], 10));
        if self.shared.recording.load(Relaxed) {
            let secs = self.shared.rec_frames.load(Relaxed) / self.out_rate as u64;
            let mut seg = vec![val("\u{f111} REC", RED), val(&format!("{}:{:02}", secs / 60, secs % 60), WHITE)];
            seg.extend(self.take_contents(val, lab));
            left.push((seg, 10));
        } else if self.shared.rec_request.load(Relaxed) {
            let mut seg = vec![val("REC ARMED", WHITE)];
            seg.extend(self.take_contents(val, lab));
            left.push((seg, 10));
        }
        left.push((vec![lab("MOTOR"), if self.motor { val("ON", GREEN) } else { val("OFF", DIM) }], 9));
        left.push((vec![val(if self.rpm45 { "45" } else { "33" }, LILAC), lab("rpm")], 3));
        left.push((vec![lab("FADER"), if self.fader_open { val("OPEN", GREEN) } else { val("CUT", WHITE) }], 8));
        left.push((vec![lab("VOL"), val(&format!("{:.0}%", self.volume * 100.0), WHITE)], 6));
        if self.backing.is_some() {
            let state = if self.backing_on { val(&format!("\u{f04b} {:.0}%", self.backing_vol * 100.0), GREEN) } else { val("paused", DIM) };
            // Whether takes get it too (U), so it's never a surprise afterwards.
            let into = if self.backing_in_take { val("\u{f111} IN TAKES", RED) } else { ("not in takes".to_string(), DIM, false) };
            left.push((vec![lab("BACKING"), state, into], 7));
        }
        if self.click {
            left.push((vec![lab("CLICK"), val(&format!("{:.0}", self.bpm), LILAC)], 4));
        }
        let kind = self.sens_kind as usize;
        let ver_text = format!("v{VERSION}");
        let news = self.has_news();
        let mut right: Vec<Seg> = vec![
            (if news { vec![val("\u{f005} NEW", GREEN), val(&ver_text, WHITE)] } else { vec![(ver_text.clone(), DIM, false)] }, if news { 7 } else { 1 }),
            (vec![val(kind_name(self.sens_kind), LILAC), lab(self.motion_name()), val(&format!("x{:.2}", self.sens[kind]), WHITE)], 5),
            (match &self.profile {
                Some(p) => vec![lab("PROFILE"), val(&format!("{p}{}", if self.profile_dirty { "*" } else { "" }), GREEN)],
                None => vec![lab("PROFILE"), val("none", DIM)],
            }, 2),
        ];
        let meter_w = 70;
        let px = 14.0;
        let gap = 7;
        let pad = 12;
        let seg_w = |f: &mut text::Fonts, s: &Seg| -> i32 {
            s.0.iter().map(|(t, _, b)| f.width(t, px, *b)).sum::<i32>() + gap * (s.0.len() as i32 - 1) + 2 * pad + 1
        };
        // Drop the lowest-priority pieces until the sample gets at least ~160 px.
        loop {
            let used: i32 = left.iter().chain(&right).map(|s| seg_w(f, s)).sum::<i32>() + meter_w + 2 * pad;
            if used + 160 <= w {
                break;
            }
            let lowest = left.iter().chain(&right).map(|s| s.1).filter(|&p| p < 10).min();
            let Some(p) = lowest else { break };
            if let Some(i) = right.iter().position(|s| s.1 == p) {
                right.remove(i);
            } else if let Some(i) = left.iter().position(|s| s.1 == p) {
                left.remove(i);
            }
        }
        let ty = y0 + (STATUS_H - 18) / 2;
        let draw_seg = |c: &mut Canvas, f: &mut text::Fonts, x: i32, s: &Seg| {
            let mut tx = x + pad;
            for (t, col, b) in &s.0 {
                tx += f.draw(c, tx, ty, t, *col, px, *b) + gap;
            }
        };
        let mut x = 0;
        for s in &left {
            draw_seg(c, f, x, s);
            x += seg_w(f, s);
            c.rect(x - 1, y0 + 6, 1, STATUS_H - 12, EDGE);
        }
        // Right side, from the edge inwards: level meter, then the rest.
        let mut rx = w - pad - meter_w;
        c.boxed(rx, y0 + (STATUS_H - 10) / 2, meter_w, 10, PANEL, EDGE);
        let lw = (level.min(1.0) * (meter_w - 2) as f32) as i32;
        c.rect(rx + 1, y0 + (STATUS_H - 10) / 2 + 1, lw, 8, if level > 0.95 { RED } else if level > 0.7 { LILAC } else { GREEN });
        rx -= pad;
        self.version_box = (0, 0, 0, 0);
        for s in &right {
            let sw = seg_w(f, s);
            rx -= sw;
            c.rect(rx + sw - 1, y0 + 6, 1, STATUS_H - 12, EDGE);
            if s.0.iter().any(|(t, _, _)| *t == ver_text) {
                self.version_box = (rx, y0, sw, STATUS_H);
                if news {
                    c.rect(rx + 3, y0 + 4, sw - 7, STATUS_H - 8, 0x0d2410);
                }
            }
            draw_seg(c, f, rx, s);
        }
        // The sample: name, section length, loop, truncated to the space left.
        let room = rx - x - 2 * pad;
        let name = if self.src.name.is_empty() { "no sample".to_string() } else { self.src.name.clone() };
        let detail = if self.src.name.is_empty() {
            String::new()
        } else {
            format!("  {:.2} s{}", self.rec.region_len as f64 / self.rec.rate, if self.loop_section { "  LOOP" } else { "" })
        };
        let adv = f.advance(px, true);
        let dwid = f.width(&detail, px, false);
        let max_name = (((room - dwid) as f32 / adv) as usize).max(4);
        let shown = if name.chars().count() > max_name {
            format!("{}..", name.chars().take(max_name.saturating_sub(2)).collect::<String>())
        } else {
            name
        };
        let nx = x + pad;
        let nw = f.draw(c, nx, ty, &shown, if self.src.name.is_empty() { DIM } else { WHITE }, px, true);
        if room - nw > dwid {
            f.draw(c, nx + nw, ty, &detail, if self.loop_section { GREEN } else { LABEL_TEXT }, px, false);
        }
    }

    fn paint_with(&mut self, c: &mut Canvas, f: &mut text::Fonts, partial: bool) {
        let (w, h) = (c.w as i32, c.h as i32);
        if !partial {
            c.fill(BG);
        }
        let pos = self.shared.pos();
        let speed = self.shared.speed();
        let level = f32::from_bits(self.shared.level.load(Relaxed));
        let rate = self.rec.rate;
        if !partial {
            self.draw_bar(c, f);
        }
        let y = BAR_H + 10;

        // Overview of the whole file with the chosen section.
        let overview = (12, y, w - 24, 64);
        let (ox, oy, ow, oh) = overview;
        c.rect(ox, oy, ow, oh, PANEL);
        let len = self.src.len().max(1) as f64;
        self.view = (self.view.0.max(0.0), self.view.1.min(len).max(self.view.0 + 1.0));
        let key = (self.view.0 as u64, self.view.1 as u64, ow.max(1) as usize);
        if self.peaks_key != key || self.peaks.len() != ow as usize {
            self.peaks = peaks_range(&self.src.l, &self.src.r, self.view.0, self.view.1, ow.max(1) as usize);
            self.peaks_key = key;
        }
        let (v0, vw) = (self.view.0, self.view.1 - self.view.0);
        let to_x = |s: f64| ox + ((s - v0) / vw * ow as f64) as i32;
        let rx0 = to_x(self.region.0 as f64).clamp(ox - 2, ox + ow + 2);
        let rx1 = to_x(self.region.1 as f64).clamp(ox - 2, ox + ow + 2);
        c.rect(rx0, oy, (rx1 - rx0).max(1), oh, 0x24123f);
        if let Some(x0) = self.region_drag {
            let (a, b) = (x0.min(self.cursor.0) as i32, x0.max(self.cursor.0) as i32);
            c.rect(a, oy, b - a, oh, 0x3a1d66);
        }
        let mid = oy + oh / 2;
        let amp = oh as f32 / 2.0 - 2.0;
        for (i, (lo, hi)) in self.peaks.iter().enumerate() {
            let x = ox + i as i32;
            let col = if x >= rx0 && x <= rx1 { LILAC } else { DIM };
            c.vline(x, mid - (hi.min(1.0) * amp) as i32, mid - (lo.max(-1.0) * amp) as i32, col);
        }
        if rx0 >= ox && rx0 <= ox + ow {
            c.rect(rx0, oy, 2, oh, WHITE);
        }
        if rx1 >= ox && rx1 <= ox + ow {
            c.rect(rx1 - 2, oy, 2, oh, WHITE);
        }
        // When zoomed: the times at each edge and how far in.
        if vw < len - 1.0 {
            let sr = self.src.rate as f64;
            let l = format!("{:.3}s", v0 / sr);
            let r = format!("{:.3}s  zoom {:.0}x", (v0 + vw) / sr, len / vw);
            f.draw(c, ox + 6, oy + oh - 17, &l, LABEL_TEXT, 12.0, false);
            let rw = f.width(&r, 12.0, false);
            f.draw(c, ox + ow - 6 - rw, oy + oh - 17, &r, LABEL_TEXT, 12.0, false);
        }
        if self.src.name.is_empty() {
            let t = "drop a sample here, or press O to open one";
            let tw = f.width(t, 16.0, false);
            f.draw(c, ox + (ow - tw) / 2, oy + (oh - 20) / 2, t, LABEL_TEXT, 16.0, false);
        }
        let src_pos = self.region.0 as f64 + pos * self.src.rate as f64 / rate;
        let px_needle = to_x(src_pos);
        if src_pos >= 0.0 && src_pos <= len && px_needle >= ox && px_needle < ox + ow {
            c.vline(px_needle, oy, oy + oh, GREEN);
        }

        // Scrolling close-up around the needle.
        let zoom = (12, oy + oh + 8, w - 24, 120.min((h - oy - oh) / 5).max(60));
        let (zx, zy, zw, zh) = zoom;
        c.rect(zx, zy, zw, zh, PANEL);
        let spp = 2.4 * rate / zw as f64;
        let needle = self.rec.cue + pos;
        let zmid = zy + zh / 2;
        let zamp = zh as f32 / 2.0 - 4.0;
        let mono = &self.rec.mono;
        let (r0, r1) = (self.rec.cue, self.rec.cue + self.rec.region_len as f64);
        for i in 0..zw {
            let a = needle + (i - zw / 2) as f64 * spp;
            let b = a + spp.max(1.0);
            if b < 0.0 || a >= mono.len() as f64 {
                continue;
            }
            let (ia, ib) = (a.max(0.0) as usize, (b as usize).min(mono.len()));
            let (mut lo, mut hi) = (0f32, 0f32);
            for v in &mono[ia..ib.max(ia + 1).min(mono.len())] {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
            let col = if a >= r0 && a < r1 { LILAC } else { DIM };
            c.vline(zx + i, zmid - (hi.min(1.0) * zamp) as i32, zmid - (lo.max(-1.0) * zamp) as i32, col);
        }
        c.rect(zx + zw / 2 - 1, zy, 2, zh, GREEN);
        if let Some((msg, t)) = &self.note {
            if t.elapsed() < Duration::from_secs(4) {
                let mw = f.width(msg, 14.0, false) + 16;
                c.boxed(zx + 6, zy + zh - 28, mw.min(zw - 12), 22, CHIP, WHITE);
                f.draw(c, zx + 14, zy + zh - 26, msg, WHITE, 14.0, false);
            }
        }

        // Below: the record, and the key guide beside it (or under it when narrow).
        let top = zy + zh + 12;
        let (ax, aw, ah) = (14, w - 28, (h - top - STATUS_H - 10).max(60));
        let (pcx, pcy, d, guide) = if !self.show_guide {
            // Full view: just the record.
            let d = ah.min(aw);
            (w / 2, top + ah / 2, d, (0, 0, 0, 0))
        } else {
            let d = aw.min(ah * 52 / 100);
            (w / 2, top + d / 2, d, (ax, top + d + 14, aw, ah - d - 14))
        };
        let pr = (d / 2 - 4).max(20) as f64;
        self.lay = Layout { overview, platter: (pcx as f64, pcy as f64, pr) };

        let angle = pos / rate / SEC_PER_REV * std::f64::consts::TAU;
        if self.platter_img.as_ref().is_none_or(|p| p.r != pr || p.held != self.hand) {
            self.platter_img = Some(ui::PlatterImage::new(pr, self.hand));
        }
        c.platter(pcx, pcy, self.platter_img.as_ref().unwrap(), angle);
        // Where your finger is on the tablet, with the tablet's centre as the spindle
        // and its half-height (~70 mm) as the platter's edge.
        if let Some(Kind::TabletTouch) = self.direct {
            let k = pr / 70.0;
            let (x, y) = self.direct_prev;
            // Kept on the record, so partial redraws (which repaint only the record's
            // square) never leave a trail.
            let (dx, dy) = (x * k, y * k);
            let s = ((pr - 8.0) / dx.hypot(dy).max(1e-9)).min(1.0);
            c.dot(pcx as f64 + dx * s, pcy as f64 + dy * s, 7.0, GREEN);
        }

        if self.show_guide && !partial {
            Self::draw_guide(c, f, guide);
        }
        self.draw_status(c, f, speed, level);
        self.draw_dropdown(c, f);
        if let Some(mut m) = self.menu.take() {
            self.draw_menu(c, f, &mut m);
            self.menu = Some(m);
        }
        if let Some(mut m) = self.folders.take() {
            self.draw_folders(c, f, &mut m);
            self.folders = Some(m);
        }
        if let Some(mut m) = self.dev_menu.take() {
            self.draw_devices(c, f, &mut m);
            self.dev_menu = Some(m);
        }
        if let Some(mut m) = self.whats_new.take() {
            self.draw_whats_new(c, f, &mut m);
            self.whats_new = Some(m);
        }
    }

    fn draw_menu(&self, c: &mut Canvas, f: &mut text::Fonts, m: &mut ProfileMenu) {
        let mut rows = vec![match &m.naming {
            Some(name) => Row::input("name", name),
            None => Row::new("+  Save current settings as a profile", GREEN, true),
        }];
        for (i, name) in m.names.iter().enumerate() {
            let i = i + 1;
            let mut row = Row::new(name, TEXT, false);
            if m.confirm_delete == Some(i) {
                row = row.tag("Del again to delete", RED);
            } else if m.confirm_revert && i == m.sel {
                row = row.tag("Enter again to undo your changes", WHITE);
            } else if self.profile.as_deref() == Some(name.as_str()) {
                row = row.tag(if self.profile_dirty { "\u{f111} active, changed" } else { "\u{f111} active" }, GREEN);
            }
            rows.push(row);
        }
        if m.names.is_empty() {
            rows.push(Row::new("no profiles saved yet", DIM, false));
        }
        let sub = self.profile.as_ref().map(|p| (format!("active: {p}{}", if self.profile_dirty { " *" } else { "" }), GREEN));
        let hints: &[(&str, &str)] = if m.naming.is_some() {
            &[("Enter", "save"), ("Esc", "cancel")]
        } else {
            &[("\u{2191}\u{2193}", "choose"), ("Enter", "load / save"), ("Del", "delete"), ("Esc", "close")]
        };
        draw_panel(c, f, "\u{f1de}  PROFILES", sub, &rows, m.sel, hints, &mut m.rows, &mut m.panel);
    }

    fn draw_folders(&self, c: &mut Canvas, f: &mut text::Fonts, m: &mut FolderMenu) {
        let (title, sub, rows, hints): (&str, Option<(String, u32)>, Vec<Row>, &[(&str, &str)]) = match &m.stage {
            FolderStage::Pick => (
                "\u{f07c}  FOLDERS",
                Some(("Enter to change".to_string(), DIM)),
                vec![
                    Row::new("Samples", WHITE, true).tag(&library::pretty(&self.samples_dir), LILAC),
                    Row::new("Takes (recordings)", WHITE, true).tag(&library::pretty(&self.takes_dir.lock().unwrap()), LILAC),
                ],
                &[("\u{2191}\u{2193}", "choose"), ("Enter", "change"), ("Esc", "close")],
            ),
            FolderStage::Browse { which, dir, subdirs, naming } => {
                let mut rows = vec![
                    Row::new("\u{f00c}  Use this folder", GREEN, true),
                    match naming {
                        Some(n) => Row::input("new folder", n),
                        None => Row::new("+  New folder here", GREEN, false),
                    },
                    Row::new("\u{f062}  ..", TEXT, false),
                ];
                rows.extend(subdirs.iter().map(|d| Row::new(&format!("\u{f07b}  {d}"), TEXT, false)));
                let title = if *which == Which::Samples { "\u{f07c}  SAMPLES FOLDER" } else { "\u{f07c}  TAKES FOLDER" };
                let hints: &[(&str, &str)] = if naming.is_some() {
                    &[("Enter", "create"), ("Esc", "cancel")]
                } else {
                    &[("Enter", "open / use"), ("Bksp", "up"), ("Esc", "back")]
                };
                (title, Some((library::pretty(dir), LILAC)), rows, hints)
            }
            FolderStage::Move { which, to } => {
                let from = self.folder(*which);
                (
                    "\u{f07c}  MOVE FILES?",
                    Some((format!("{}  \u{2192}  {}", library::pretty(&from), library::pretty(to)), LILAC)),
                    vec![
                        Row::new("Move my files to the new folder", GREEN, true),
                        Row::new("Just use the new folder (leave files where they are)", TEXT, false),
                    ],
                    &[("Enter", "choose"), ("Esc", "back")],
                )
            }
        };
        draw_panel(c, f, title, sub, &rows, m.sel, hints, &mut m.rows, &mut m.panel);
    }

    /// The key guide: groups in columns, largest font that fits the space.
    fn draw_guide(c: &mut Canvas, f: &mut text::Fonts, (gx, gy, gw, gh): (i32, i32, i32, i32)) {
        for px in [21.0f32, 20.0, 19.0, 18.0, 17.0, 16.0, 15.0, 14.0, 13.0, 12.0, 11.0] {
            let line_h = (px * 1.55) as i32;
            let head_h = (px * 1.9) as i32;
            let gap = (px * 0.9) as i32;
            let key_pad = 12;
            // One key width for every group, so all the descriptions line up.
            let all = || GUIDE.iter().flat_map(|(_, _, items)| items.iter());
            let kw = all().map(|(k, _)| f.width(k, px, true)).max().unwrap_or(0) + key_pad;
            let dw = all().map(|(_, d)| f.width(d, px, false)).max().unwrap_or(0);
            let col_w = kw + 10 + dw + 28;
            // Fixed columns, so the guide looks the same at every window size.
            let mut spots = Vec::new();
            let mut tallest = 0;
            for (col, groups) in GUIDE_COLUMNS.iter().enumerate() {
                let mut y = 0;
                for &i in groups.iter() {
                    spots.push((i, gx + col as i32 * col_w, gy + y));
                    y += head_h + GUIDE[i].2.len() as i32 * line_h + gap;
                }
                tallest = tallest.max(y - gap);
            }
            let used = GUIDE_COLUMNS.len() as i32 * col_w - 28;
            if (tallest > gh || used > gw) && px > 11.0 {
                continue;
            }
            // Centred under the record.
            let shift = ((gw - used) / 2).max(0);
            for s in spots.iter_mut() {
                s.1 += shift;
            }
            for (i, x, y) in spots {
                let (icon, title, items) = GUIDE[i];
                let hw = f.draw(c, x, y, &format!("{icon} {title}"), WHITE, px * 1.1, true);
                c.rect(x, y + (px * 1.45) as i32, (col_w - 24).max(hw), 1, PURPLE);
                let mut iy = y + head_h;
                for (k, d) in items {
                    c.boxed(x, iy, kw, line_h - 4, 0x0d2410, 0x1d5a22);
                    let tw = f.width(k, px, true);
                    f.draw(c, x + (kw - tw) / 2, iy + (line_h - 4 - (px * 1.2) as i32) / 2, k, GREEN, px, true);
                    f.draw(c, x + kw + 10, iy + (line_h - 4 - (px * 1.2) as i32) / 2, d, TEXT, px, false);
                    iy += line_h;
                }
            }
            return;
        }
    }
}

impl ApplicationHandler<InEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let title = if self.src.name.is_empty() { "CUT".to_string() } else { format!("CUT - {}", self.src.name) };
        let attrs = Window::default_attributes()
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0))
            .with_window_icon(window_icon())
            // WM_CLASS, so launchers and window rules can match the window.
            .with_name("cut-deck", "CUT");
        let win = Rc::new(el.create_window(attrs).expect("create window"));
        let ctx = softbuffer::Context::new(win.clone()).expect("softbuffer context");
        self.surface = Some(softbuffer::Surface::new(&ctx, win.clone()).expect("softbuffer surface"));
        self.window = Some(win);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if !matches!(event, WindowEvent::RedrawRequested) {
            self.dirty = true;
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::RedrawRequested => self.draw(),
            WindowEvent::CursorMoved { position, .. } => self.cursor_moved(position.x, position.y),
            WindowEvent::MouseInput { state, button, .. } => self.mouse_button(button, state == ElementState::Pressed),
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x as f64 * SCROLL_PX, y as f64 * SCROLL_PX),
                    MouseScrollDelta::PixelDelta(p) => (p.x, p.y),
                };
                // Over the top waveform the wheel zooms (up = in) and sideways scrolling pans.
                if !self.overlay_open() && self.over_overview() && !self.src.name.is_empty() {
                    if dy != 0.0 {
                        self.zoom_overview((dy / SCROLL_PX * -0.12).exp(), self.cursor.0);
                    }
                    if dx != 0.0 {
                        self.pan_overview(-dx);
                    }
                    return;
                }
                self.scrolled(dx, dy);
            }
            WindowEvent::ModifiersChanged(m) => self.ctrl = m.state().control_key(),
            WindowEvent::KeyboardInput { event, .. } => {
                // The menu takes key presses; releases still go through so held fader keys
                // don't get stuck.
                if event.state == ElementState::Pressed {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        if !self.overlay_open() {
                            if self.bar_key(code) {
                                return;
                            }
                            if matches!(code, KeyCode::F10 | KeyCode::AltLeft) && self.bar.open.is_none() {
                                self.open_bar(0);
                                return;
                            }
                        }
                    }
                    if self.whats_new.is_some() {
                        self.whats_new_key(&event);
                        return;
                    }
                    if self.dev_menu.is_some() {
                        self.dev_key(&event);
                        return;
                    }
                    if self.folders.is_some() {
                        self.folder_key(&event);
                        return;
                    }
                    if self.menu.is_some() {
                        self.menu_key(&event);
                        return;
                    }
                }
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.key(code, event.state == ElementState::Pressed, event.repeat);
                }
            }
            WindowEvent::DroppedFile(p) => self.load_path(&p),
            WindowEvent::Focused(true) => {
                self.focused = true;
                self.desk.set_focused(true);
                self.sync_devices();
            }
            WindowEvent::Focused(false) => {
                self.focused = false;
                self.desk.set_focused(false);
                self.desk.pause_touchpad(false);
                self.direct = None;
                self.fader_keys.clear();
                self.shift = false;
                self.mouse_scratch = false;
                self.dev_fader.clear();
                self.dev_hold.clear();
                self.update_fader();
                self.update_hand();
                self.sync_devices();
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: InEvent) {
        if std::env::var_os("CUT_DEBUG").is_some() {
            eprintln!("{ev:?} hand={} u={:.0}", self.hand, self.hand_u);
        }
        match ev {
            InEvent::Device(info) => {
                self.device_added(info);
                self.dirty = true;
            }
            InEvent::Gone(inst) => {
                self.device_gone(inst);
                self.dirty = true;
            }
            InEvent::Control { dev, ctl, value, t } => {
                if self.device_control(dev, ctl, value, t) {
                    self.moved = true;
                    if self.dev_menu.is_some() {
                        self.dirty = true;
                    }
                }
            }
            // A touch surface set up to work as the other kind is read as that kind.
            InEvent::Down { dev, kind, t, x, y, fingers } => {
                self.moved = true;
                // Touch surfaces light up in the Devices list too.
                if self.dev_flash(&dev) {
                    self.dirty = true;
                }
                let kind = self.touch_kind(&dev, kind);
                self.direct_event(InEvent::Down { dev, kind, t, x, y, fingers });
            }
            InEvent::Move { dev, kind, t, x, y, fingers } => {
                self.moved = true;
                let kind = self.touch_kind(&dev, kind);
                self.direct_event(InEvent::Move { dev, kind, t, x, y, fingers });
            }
            InEvent::Anchor { dev, kind, x, y, fingers } => {
                self.moved = true;
                let kind = self.touch_kind(&dev, kind);
                self.direct_event(InEvent::Anchor { dev, kind, x, y, fingers });
            }
            InEvent::Up { dev, kind } => {
                self.moved = true;
                let kind = self.touch_kind(&dev, kind);
                self.direct_event(InEvent::Up { dev, kind });
            }
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.quit {
            el.exit();
            return;
        }
        while self.trash.pop().is_ok() {}
        self.update_hand();
        let status = self.take_status.lock().unwrap().take();
        if let Some(s) = status {
            self.say(s);
        }
        if let Some(rx) = &self.open_rx {
            if let Ok(res) = rx.try_recv() {
                self.open_rx = None;
                if let Some(p) = res {
                    self.load_path(&p);
                }
            }
        }
        if let Some(rx) = &self.backing_rx {
            if let Ok(res) = rx.try_recv() {
                self.backing_rx = None;
                if let Some((p, src)) = res {
                    self.backing_loaded(p, src);
                }
            }
        }
        if let Some(rx) = &self.save_rx {
            if let Ok(res) = rx.try_recv() {
                self.save_rx = None;
                if let Some((name, folder, new_folder)) = res {
                    self.save_section(&name, &folder, &new_folder);
                }
            }
        }
        // Keep redrawing while a device row is lit up, so the highlight goes away on time.
        if self.dev_menu.as_ref().is_some_and(|m| m.flash.as_ref().is_some_and(|(_, t)| t.elapsed() < Duration::from_millis(1000))) {
            self.dirty = true;
        }
        // Draw only when something changed, at most ~60 times a second. When nothing is
        // moving, check back less often (input wakes us straight away anyway).
        let changed = self.dirty || self.moved || self.frame_sig() != self.last_sig;
        if changed && self.last_draw.elapsed() >= Duration::from_millis(16) {
            self.last_draw = Instant::now();
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        let wait = if changed { 4 } else { 25 };
        el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(wait)));
    }
}

struct Audio {
    stream: cpal::Stream,
    tx: rtrb::Producer<Msg>,
    trash_rx: rtrb::Consumer<Arc<Record>>,
    take_rx: rtrb::Consumer<f32>,
    shared: Arc<Shared>,
    out_rate: u32,
}

/// JACK (PipeWire's) first: it honours small buffers cleanly. Plain ALSA as a fallback.
fn open_audio(rec: Arc<Record>) -> anyhow::Result<Audio> {
    let mut last_err = anyhow::anyhow!("no audio host available");
    for id in [cpal::HostId::Jack, cpal::HostId::Alsa] {
        let attempt = || -> anyhow::Result<Audio> {
            let host = cpal::host_from_id(id)?;
            let device = host.default_output_device().context("no output device")?;
            let def = device.default_output_config()?;
            let out_rate = def.sample_rate().0;
            let channels = def.channels() as usize;
            let (tx, rx) = rtrb::RingBuffer::<Msg>::new(8192);
            let (trash_tx, trash_rx) = rtrb::RingBuffer::new(64);
            let (take_tx, take_rx) = rtrb::RingBuffer::new(out_rate as usize * 4);
            let shared = Arc::new(Shared::default());
            let mut engine = Engine::new(rec.clone(), out_rate as f64, rx, trash_tx, shared.clone(), take_tx);
            let config = cpal::StreamConfig { channels: channels as u16, sample_rate: cpal::SampleRate(out_rate), buffer_size: cpal::BufferSize::Default };
            let errors = std::sync::atomic::AtomicU32::new(0);
            let stream = device.build_output_stream(
                &config,
                move |data: &mut [f32], _| engine.process(data, channels),
                move |e| {
                    if errors.fetch_add(1, Relaxed) < 3 {
                        eprintln!("audio error: {e}");
                    }
                },
                None,
            )?;
            stream.play()?;
            eprintln!("audio: {} @ {out_rate} Hz via {}", device.name().unwrap_or_default(), id.name());
            Ok(Audio { stream, tx, trash_rx, take_rx, shared, out_rate })
        };
        match attempt() {
            Ok(a) => return Ok(a),
            Err(e) => {
                eprintln!("audio via {} unavailable: {e:#}", id.name());
                last_err = e;
            }
        }
    }
    Err(last_err)
}

/// One row of a menu panel.
struct Row {
    text: String,
    color: u32,
    bold: bool,
    /// Right-aligned note, e.g. "active".
    tag: Option<(String, u32)>,
    /// A text box being typed in; `text` is its label.
    input: Option<String>,
}

impl Row {
    fn new(text: &str, color: u32, bold: bool) -> Self {
        Row { text: text.to_string(), color, bold, tag: None, input: None }
    }

    fn input(label: &str, value: &str) -> Self {
        Row { input: Some(value.to_string()), ..Row::new(label, LABEL_TEXT, false) }
    }

    fn tag(mut self, t: &str, color: u32) -> Self {
        self.tag = Some((t.to_string(), color));
        self
    }
}

/// A menu panel over the dimmed deck, in the app's style. Writes where each row and the
/// panel ended up, for the pointer.
#[allow(clippy::too_many_arguments)]
fn draw_panel(
    c: &mut Canvas,
    f: &mut text::Fonts,
    title: &str,
    sub: Option<(String, u32)>,
    rows: &[Row],
    sel: usize,
    hints: &[(&str, &str)],
    out_rows: &mut Vec<(i32, i32)>,
    out_panel: &mut (i32, i32, i32, i32),
) {
    let (w, h) = (c.w as i32, c.h as i32);
    for p in c.px.iter_mut() {
        *p = ((*p >> 2) & 0x3f3f3f) + ((*p >> 3) & 0x1f1f1f);
    }
    let px = 17.0;
    let row_h = 36;
    let pw = 680.min(w - 32);
    let max_rows = ((h - 200) / row_h).max(3) as usize;
    let total = rows.len().max(1);
    let shown = total.min(max_rows);
    let ph = 70 + shown as i32 * row_h + 64;
    let (x0, y0) = ((w - pw) / 2, ((h - ph) / 2).max(16));
    *out_panel = (x0, y0, pw, ph);
    c.boxed(x0, y0, pw, ph, 0x14141b, PURPLE);
    c.rect(x0, y0, pw, 3, PURPLE);
    let tw = f.draw(c, x0 + 22, y0 + 20, title, WHITE, 21.0, true);
    if let Some((t, col)) = sub {
        let room = ((pw - 66 - tw) as f32 / f.advance(14.0, false)) as usize;
        let t = if t.chars().count() > room { format!("..{}", t.chars().skip(t.chars().count() + 2 - room.max(3)).collect::<String>()) } else { t };
        let w2 = f.width(&t, 14.0, false);
        f.draw(c, x0 + pw - 22 - w2, y0 + 25, &t, col, 14.0, false);
    }
    c.rect(x0 + 22, y0 + 56, pw - 44, 1, 0x2c2c3a);

    // Scroll so the selection stays visible.
    let first = sel.saturating_sub(shown - 1).min(total - shown);
    out_rows.clear();
    out_rows.extend(std::iter::repeat_n((-1, -1), first));
    let mut y = y0 + 66;
    for (i, row) in rows.iter().enumerate().skip(first).take(shown) {
        let ty = y + (row_h - 22) / 2;
        out_rows.push((y, y + row_h));
        if i == sel {
            c.rect(x0 + 12, y + 2, pw - 24, row_h - 4, 0x241240);
            c.rect(x0 + 12, y + 2, 3, row_h - 4, GREEN);
        }
        match &row.input {
            Some(v) => {
                f.draw(c, x0 + 26, ty + 2, &row.text, row.color, 14.0, false);
                let bx = x0 + 26 + f.width(&row.text, 14.0, false) + 12;
                c.boxed(bx, y + 5, x0 + pw - 26 - bx, row_h - 10, 0x0b0b10, GREEN);
                let vw = f.draw(c, bx + 10, ty, v, WHITE, px, false);
                c.rect(bx + 12 + vw, y + 10, 2, row_h - 20, GREEN);
            }
            None => {
                let col = if i == sel && row.color == TEXT { WHITE } else { row.color };
                let used = f.draw(c, x0 + 26, ty, &row.text, col, px, row.bold);
                if let Some((t, tc)) = &row.tag {
                    let room = ((pw - 70 - used) as f32 / f.advance(14.0, false)) as usize;
                    let t = if t.chars().count() > room { format!("..{}", t.chars().skip(t.chars().count() + 2 - room.max(3)).collect::<String>()) } else { t.clone() };
                    let w2 = f.width(&t, 14.0, false);
                    f.draw(c, x0 + pw - 26 - w2, ty + 2, &t, *tc, 14.0, false);
                }
            }
        }
        y += row_h;
    }

    c.rect(x0 + 22, y + 8, pw - 44, 1, 0x2c2c3a);
    let mut hx = x0 + 22;
    let hy = y + 22;
    for (k, d) in hints {
        let kw = f.width(k, 14.0, true) + 12;
        c.boxed(hx, hy, kw, 24, 0x0d2410, 0x1d5a22);
        f.draw(c, hx + 6, hy + 3, k, GREEN, 14.0, true);
        hx += kw + 8;
        hx += f.draw(c, hx, hy + 3, d, TEXT, 14.0, false) + 18;
    }
}

/// The torn record as the window's icon (taskbars, alt-tab, window lists).
fn window_icon() -> Option<winit::window::Icon> {
    let size = 64;
    let rgba: Vec<u8> = ui::record_icon(size)
        .iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8])
        .collect();
    winit::window::Icon::from_rgba(rgba, size as u32, size as u32).ok()
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Mouse => "mouse",
        Kind::Touchpad => "touchpad",
        Kind::TabletTouch => "tablet touch",
        Kind::Controller => "controller",
    }
}

fn wrap_angle(a: f64) -> f64 {
    use std::f64::consts::{PI, TAU};
    if a > PI {
        a - TAU
    } else if a < -PI {
        a + TAU
    } else {
        a
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `cut-deck --write-icon PATH [SIZE]`: save the app icon as a PNG (for the launcher).
    if args.first().map(String::as_str) == Some("--write-icon") {
        let path = args.get(1).context("--write-icon needs a path")?;
        let size: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(256);
        ui::write_png(Path::new(path), size, &ui::record_icon(size))?;
        return Ok(());
    }
    // `cut-deck --grant-access VENDOR PRODUCT NAME`: run as root by the Devices menu.
    if args.first().map(String::as_str) == Some("--grant-access") {
        let (Some(v), Some(p), Some(n)) = (args.get(1), args.get(2), args.get(3)) else {
            anyhow::bail!("usage: cut-deck --grant-access VENDOR PRODUCT NAME");
        };
        devices::grant_access(v, p, n)?;
        println!("access given to {n} ({v}:{p})");
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("--version") {
        println!("CUT {VERSION}");
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("--render-test") {
        let out = args.get(1).map(String::as_str).unwrap_or("render-test.wav");
        return engine::render_test(args.get(2).map(String::as_str), out);
    }

    // Ask PipeWire for a small quantum; its default (1024) adds ~20 ms between hand and ear.
    if std::env::var_os("PIPEWIRE_LATENCY").is_none() {
        std::env::set_var("PIPEWIRE_LATENCY", "128/48000");
    }

    let src = match args.first() {
        Some(p) => deck::load(Path::new(p))?,
        None => Source::silent(),
    };
    let rec = Arc::new(deck::build(&src, 0, src.len(), true, false, false));

    let audio = open_audio(rec.clone())?;
    let Audio { stream, tx, trash_rx, take_rx, shared, out_rate } = audio;
    let take_status = Arc::new(Mutex::new(None));
    let takes_dir = Arc::new(Mutex::new(library::root().join("Takes")));
    let save_mp3 = Arc::new(std::sync::atomic::AtomicBool::new(false));
    recorder::spawn(shared.clone(), take_rx, out_rate, take_status.clone(), takes_dir.clone(), save_mp3.clone());

    let el = EventLoop::<InEvent>::with_user_event().build()?;
    let in_status = Arc::new(input::Status::default());
    let (found, denied) = input::start(el.create_proxy(), in_status.clone());
    midi::start(el.create_proxy());

    let src_len = src.len();
    let mut app = App {
        window: None,
        surface: None,
        tx,
        trash: trash_rx,
        shared,
        take_status,
        _stream: stream,
        out_rate,
        region: (0, src.len()),
        path: args.first().map(PathBuf::from),
        src,
        rec,
        peaks: Vec::new(),
        peaks_key: (0, 0, 0),
        view: (0.0, src_len as f64),
        loop_section: false,
        mode: [Mode::Auto, Mode::Auto, Mode::Platter, Mode::Auto],
        dir: 1.0,
        sens: [1.0; 4],
        sens_kind: Kind::Touchpad,
        smooth_hz: 70.0,
        eq: true,
        noise: false,
        invert: false,
        // Opens paused, ready to go: M (or Enter) starts the platter.
        motor: false,
        rpm45: false,
        volume: 0.7,
        click: false,
        bpm: 90.0,
        fader_keys: HashSet::new(),
        fader_open: true,
        cursor: (0.0, 0.0),
        last_angle: 0.0,
        mouse_scratch: false,
        shift: false,
        ctrl: false,
        scroll_last: None,
        hand: false,
        hand_u: 0.0,
        region_drag: None,
        in_status: in_status.clone(),
        lay: layout(1280, 800),
        fonts: text::Fonts::load(),
        desk: desk::Desk::new(),
        focused: true,
        direct: None,
        direct_prev: (0.0, 0.0),
        direct_angle: 0.0,
        stroke_cov: (1e-3, 0.0, 0.0),
        stroke_axis: (1.0, 0.0),
        note: None,
        open_rx: None,
        save_rx: None,
        menu: None,
        bar: MenuBar::default(),
        quit: false,
        platter_img: None,
        icon: ui::record_icon(ICON_PX),
        dirty: true,
        moved: false,
        last_sig: (0, 0, 0, false),
        folders: None,
        samples_dir: library::root(),
        takes_dir: takes_dir.clone(),
        save_mp3: save_mp3.clone(),
        show_guide: false,
        profile: None,
        profile_dirty: false,
        last_draw: Instant::now(),
        devs: HashMap::new(),
        dev_profiles: devices::load_all(),
        dev_menu: None,
        dev_last: HashMap::new(),
        dev_pressed: HashSet::new(),
        dev_fader: HashSet::new(),
        dev_hold: HashSet::new(),
        vol_acc: HashMap::new(),
        jog_last: None,
        started: Instant::now(),
        version_seen: String::new(),
        whats_new: None,
        version_box: (0, 0, 0, 0),
        backing: None,
        backing_prev: None,
        backing_path: None,
        backing_on: false,
        backing_vol: 0.7,
        backing_in_take: false,
        backing_rx: None,
    };
    // Settings used to live in ~/.config/scratchdeck (the app's old name).
    let new_dir = App::settings_path().parent().map(Path::to_path_buf);
    if let Some(new_dir) = new_dir {
        let old_dir = new_dir.with_file_name("scratchdeck");
        if !new_dir.exists() && old_dir.is_dir() {
            let _ = std::fs::rename(&old_dir, &new_dir);
        }
    }
    app.load_settings();
    app.reload_backing();
    let found_note = if !found.is_empty() {
        format!("direct input: {}", found.join(", "))
    } else if denied > 0 {
        "no access to touchpad/tablet devices - run input-access.sh from the cut folder for 2-finger holds".to_string()
    } else {
        String::new()
    };
    if !found_note.is_empty() {
        eprintln!("{found_note}");
        app.say(found_note);
    }
    // Killed or closed mid-take: finish the take file before exiting.
    let closing = app.shared.clone();
    *desk::EXIT_HOOK.lock().unwrap() = Some(Arc::new(move || closing.close_take()));
    let closing = app.shared.clone();
    let res = el.run_app(&mut app);
    closing.close_take();
    drop(app);
    res?;
    Ok(())
}

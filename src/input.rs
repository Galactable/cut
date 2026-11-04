//! Direct input from /dev/input, read beside X11: touch surfaces (laptop touchpad,
//! tablet touch) and any other device with buttons, wheels or axes.
//!
//! X only tells us "scrolling" for a touchpad, with the
//! timing smeared by the desktop. Reading the kernel devices gives finger contact (so
//! two fingers down = hand on the record, lift = let go), millimetre positions and the
//! kernel's own timestamps.
//!
//! Devices are picked up as they're plugged in. Each one gets a reader thread that can
//! take the device from the desktop (EVIOCGRAB) while the app asks it to.

use std::{
    collections::{HashMap, HashSet},
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering::Relaxed},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use evdev::{raw_stream::RawDevice, AbsoluteAxisType as Abs, InputEventKind, Key, Synchronization};
use winit::event_loop::EventLoopProxy;

use crate::devices::{Class, Ctl, DevInfo};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// The mouse, through X (never from this module; it shares the per-input settings).
    Mouse,
    /// A laptop touchpad.
    Touchpad,
    /// The finger surface of a drawing tablet: bigger, so you scratch bigger.
    TabletTouch,
    /// A jog wheel, dial, ring or axis on a device set up in the Devices menu.
    Controller,
}

/// Positions are millimetres from the centre of the surface, y pointing down. `dev` is
/// the device's `inst` (see DevInfo).
#[derive(Debug)]
pub enum InEvent {
    Down { dev: Arc<str>, kind: Kind, t: u64, x: f64, y: f64, fingers: u8 },
    Move { dev: Arc<str>, kind: Kind, t: u64, x: f64, y: f64, fingers: u8 },
    /// Fingers were added or lifted while still holding: carry on from here without a jump.
    Anchor { dev: Arc<str>, kind: Kind, x: f64, y: f64, fingers: u8 },
    Up { dev: Arc<str>, kind: Kind },
    /// A control on a non-touch device changed (only sent while `forward` is set).
    Control { dev: Arc<str>, ctl: Ctl, value: i32, t: u64 },
    /// A device turned up, or became readable.
    Device(DevInfo),
    /// A device was unplugged.
    Gone(Arc<str>),
}

#[derive(Default)]
pub struct Status {
    pub touch: AtomicBool,
}

pub fn micros(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(0)
}

struct Axis {
    centre: f64,
    per_mm: f64,
}

impl Axis {
    fn new(min: i32, max: i32, resolution: i32, fallback_mm: f64) -> Self {
        let span = (max - min).max(1) as f64;
        let per_mm = if resolution > 0 { resolution as f64 } else { span / fallback_mm };
        Axis { centre: (min as f64 + max as f64) / 2.0, per_mm }
    }
    fn mm(&self, v: i32) -> f64 {
        (v as f64 - self.centre) / self.per_mm
    }
}

/// How often unplugged/plugged devices are noticed.
const RESCAN: Duration = Duration::from_millis(1500);

/// Opens every device we're allowed to read, then keeps watching for new ones. Returns
/// the touch surfaces found at start and how many touchpads/tablets couldn't be opened
/// for lack of permission.
pub fn start(proxy: EventLoopProxy<InEvent>, status: Arc<Status>) -> (Vec<String>, usize) {
    let open: Arc<Mutex<HashSet<PathBuf>>> = Arc::default();
    let mut denied: HashMap<PathBuf, bool> = HashMap::new();
    let (found, n_denied) = scan(&proxy, &status, &open, &mut denied);
    std::thread::spawn(move || loop {
        std::thread::sleep(RESCAN);
        scan(&proxy, &status, &open, &mut denied);
        // Forget unplugged unreadable nodes, so a new device on the same node is reported.
        denied.retain(|p, _| p.exists());
    });
    (found, n_denied)
}

fn scan(
    proxy: &EventLoopProxy<InEvent>,
    status: &Arc<Status>,
    open: &Arc<Mutex<HashSet<PathBuf>>>,
    denied: &mut HashMap<PathBuf, bool>,
) -> (Vec<String>, usize) {
    let mut found = Vec::new();
    let mut n_denied = 0;
    let Ok(dir) = std::fs::read_dir("/dev/input") else { return (found, 0) };
    let mut paths: Vec<PathBuf> = dir.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("event"))).collect();
    paths.sort();
    for path in paths {
        if open.lock().unwrap().contains(&path) {
            continue;
        }
        let dev = match RawDevice::open(&path) {
            Ok(d) => d,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    if is_wanted_sysfs(&path) {
                        n_denied += 1;
                    }
                    // Report each unreadable device once (until it's unplugged).
                    if !denied.contains_key(&path) {
                        if let Some(info) = sysfs_info(&path) {
                            let _ = proxy.send_event(InEvent::Device(info));
                        }
                        denied.insert(path.clone(), true);
                    }
                }
                continue;
            }
        };
        denied.remove(&path);
        let info = dev_info(&dev, &path);
        if let Class::Touch(_) = info.class {
            status.touch.store(true, Relaxed);
            found.push(format!("touch: {}", info.name));
        }
        open.lock().unwrap().insert(path.clone());
        let _ = proxy.send_event(InEvent::Device(info.clone()));
        let (p, open) = (proxy.clone(), open.clone());
        std::thread::spawn(move || {
            match info.class {
                Class::Touch(kind) => touch_loop(dev, &p, &info, kind),
                _ => control_loop(dev, &p, &info),
            }
            open.lock().unwrap().remove(&path);
            let _ = p.send_event(InEvent::Gone(info.inst.clone()));
        });
    }
    (found, n_denied)
}

fn is_tablet_touch(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("wacom") || n.contains("finger") || n.contains("huion") || n.contains("xp-pen")
}

fn node(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().to_string()
}

/// Whether an unreadable node is one we'd want, judged from sysfs (readable by anyone).
fn is_wanted_sysfs(path: &Path) -> bool {
    let name = std::fs::read_to_string(format!("/sys/class/input/{}/device/name", node(path))).unwrap_or_default().to_lowercase();
    name.contains("touchpad") || name.contains("finger")
}

/// udev's ID_INPUT_* flags for a node, e.g. "TOUCHPAD", "JOYSTICK".
fn udev_flags(path: &Path) -> HashSet<String> {
    let devno = std::fs::read_to_string(format!("/sys/class/input/{}/dev", node(path))).unwrap_or_default();
    std::fs::read_to_string(format!("/run/udev/data/c{}", devno.trim()))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix("E:ID_INPUT_"))
        .filter_map(|l| l.strip_suffix("=1"))
        .map(str::to_string)
        .collect()
}

fn hex_attr(path: &Path, attr: &str) -> u16 {
    std::fs::read_to_string(format!("/sys/class/input/{}/device/id/{attr}", node(path))).ok().and_then(|s| u16::from_str_radix(s.trim(), 16).ok()).unwrap_or(0)
}

fn dev_id(vendor: u16, product: u16, name: &str) -> String {
    format!("{vendor:04x}:{product:04x} {name}")
}

/// What we can tell about a device we can't open.
fn sysfs_info(path: &Path) -> Option<DevInfo> {
    let name = std::fs::read_to_string(format!("/sys/class/input/{}/device/name", node(path))).ok()?.trim().to_string();
    let flags = udev_flags(path);
    let has = |f: &str| flags.contains(f);
    let class = if has("TOUCHPAD") {
        Class::Touch(Kind::Touchpad)
    } else if has("TOUCHSCREEN") {
        Class::Touch(Kind::TabletTouch)
    } else if has("TABLET_PAD") || has("JOYSTICK") {
        Class::Controls
    } else if has("TABLET") {
        Class::Pen
    } else if has("MOUSE") || has("POINTINGSTICK") {
        Class::Mouse
    } else if has("KEYBOARD") {
        Class::Keyboard
    } else {
        Class::Other
    };
    let (vendor, product) = (hex_attr(path, "vendor"), hex_attr(path, "product"));
    Some(DevInfo {
        inst: path.to_string_lossy().into(),
        id: dev_id(vendor, product, &name),
        name,
        class,
        readable: false,
        vendor,
        product,
        ranges: HashMap::new(),
        grab: Arc::default(),
        forward: Arc::default(),
    })
}

fn dev_info(dev: &RawDevice, path: &Path) -> DevInfo {
    let keys = dev.supported_keys();
    let abs = dev.supported_absolute_axes();
    let has_key = |k| keys.is_some_and(|s| s.contains(k));
    let has_abs = |a| abs.is_some_and(|s| s.contains(a));
    let name = dev.name().unwrap_or("?").to_string();
    let flags = udev_flags(path);
    let class = if has_key(Key::BTN_TOOL_FINGER) && has_abs(Abs::ABS_MT_POSITION_X) && has_abs(Abs::ABS_MT_SLOT) {
        let tablet = flags.contains("TOUCHSCREEN") || is_tablet_touch(&name);
        Class::Touch(if tablet { Kind::TabletTouch } else { Kind::Touchpad })
    } else if has_key(Key::BTN_TOOL_PEN) {
        // Pens are left alone by default: CUT is touch-only.
        Class::Pen
    } else if flags.contains("MOUSE") && !flags.contains("JOYSTICK") {
        Class::Mouse
    } else if flags.contains("KEYBOARD") && !flags.contains("JOYSTICK") && !flags.contains("TABLET_PAD") {
        Class::Keyboard
    } else if keys.is_some() || abs.is_some() || dev.supported_relative_axes().is_some() {
        Class::Controls
    } else {
        Class::Other
    };
    let mut ranges = HashMap::new();
    if let (Some(set), Ok(st)) = (abs, dev.get_abs_state()) {
        for a in set.iter() {
            let i = &st[a.0 as usize];
            ranges.insert(a.0, (i.minimum, i.maximum));
        }
    }
    let id = dev.input_id();
    DevInfo {
        inst: path.to_string_lossy().into(),
        id: dev_id(id.vendor(), id.product(), &name),
        name,
        class,
        readable: true,
        vendor: id.vendor(),
        product: id.product(),
        ranges,
        grab: Arc::default(),
        forward: Arc::default(),
    }
}

/// Waits up to 50 ms for input, taking or giving back the device as asked meanwhile.
/// False once the device is gone.
fn wait(dev: &mut RawDevice, want_grab: &AtomicBool, grabbed: &mut bool) -> Option<bool> {
    let want = want_grab.load(Relaxed);
    if want != *grabbed {
        let ok = if want { dev.grab() } else { dev.ungrab() };
        if ok.is_ok() {
            *grabbed = want;
        }
    }
    let mut pfd = libc::pollfd { fd: dev.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    // SAFETY: one valid pollfd, owned by this stack frame.
    let n = unsafe { libc::poll(&mut pfd, 1, 50) };
    if n < 0 {
        return (std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted).then_some(false);
    }
    if pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        return None;
    }
    Some(n > 0)
}

/// Buttons, wheels and axes, passed on raw; the app decides what they mean.
fn control_loop(mut dev: RawDevice, proxy: &EventLoopProxy<InEvent>, info: &DevInfo) {
    let mut grabbed = false;
    loop {
        match wait(&mut dev, &info.grab, &mut grabbed) {
            None => return,
            Some(false) => continue,
            Some(true) => {}
        }
        let Ok(events) = dev.fetch_events() else { return };
        let forward = info.forward.load(Relaxed);
        for ev in events {
            if !forward {
                continue;
            }
            let ctl = match ev.kind() {
                // Key repeats (2) say nothing new.
                InputEventKind::Key(k) if ev.value() != 2 => Ctl::Key(k.code()),
                InputEventKind::AbsAxis(a) => Ctl::Abs(a.0),
                // Hi-res wheel copies (11, 12) double up the plain ones.
                InputEventKind::RelAxis(r) if r.0 != 11 && r.0 != 12 => Ctl::Rel(r.0),
                _ => continue,
            };
            let m = InEvent::Control { dev: info.inst.clone(), ctl, value: ev.value(), t: micros(ev.timestamp()) };
            if proxy.send_event(m).is_err() {
                return;
            }
        }
    }
}

fn touch_loop(mut dev: RawDevice, proxy: &EventLoopProxy<InEvent>, info: &DevInfo, kind: Kind) {
    let Ok(st) = dev.get_abs_state() else { return };
    let axis = |a: Abs, mm| {
        let i = &st[a.0 as usize];
        Axis::new(i.minimum, i.maximum, i.resolution, mm)
    };
    let (ax, ay) = (axis(Abs::ABS_MT_POSITION_X, 100.0), axis(Abs::ABS_MT_POSITION_Y, 60.0));
    let nslots = (st[Abs::ABS_MT_SLOT.0 as usize].maximum + 1).clamp(1, 16) as usize;
    let d = info.inst.clone();
    // (x, y) per slot while a finger is on it.
    let mut slots: Vec<Option<(i32, i32)>> = vec![None; nslots];
    // Tracking id per slot, so a finger swapped in the same slot isn't read as a jump.
    let mut ids = vec![-1i32; nslots];
    let mut held_ids = Vec::new();
    let mut cur = 0usize;
    let mut holding = false;
    let mut held_mask = 0u32;
    let mut grabbed = false;
    loop {
        match wait(&mut dev, &info.grab, &mut grabbed) {
            None => return,
            Some(false) => continue,
            Some(true) => {}
        }
        let Ok(events) = dev.fetch_events() else { return };
        for ev in events {
            match ev.kind() {
                InputEventKind::AbsAxis(Abs::ABS_MT_SLOT) => cur = (ev.value().max(0) as usize).min(nslots - 1),
                InputEventKind::AbsAxis(Abs::ABS_MT_TRACKING_ID) => {
                    ids[cur] = ev.value();
                    slots[cur] = if ev.value() < 0 { None } else { Some((i32::MIN, i32::MIN)) };
                }
                InputEventKind::AbsAxis(Abs::ABS_MT_POSITION_X) => {
                    if let Some(s) = slots[cur].as_mut() {
                        s.0 = ev.value();
                    }
                }
                InputEventKind::AbsAxis(Abs::ABS_MT_POSITION_Y) => {
                    if let Some(s) = slots[cur].as_mut() {
                        s.1 = ev.value();
                    }
                }
                InputEventKind::Synchronization(Synchronization::SYN_REPORT) => {
                    let mut mask = 0u32;
                    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
                    for (i, s) in slots.iter().enumerate() {
                        if let Some((x, y)) = s {
                            if *x != i32::MIN && *y != i32::MIN {
                                mask |= 1 << i;
                                sx += ax.mm(*x);
                                sy += ay.mm(*y);
                                n += 1.0;
                            }
                        }
                    }
                    let t = micros(ev.timestamp());
                    // Any finger holds the record (on a touchpad one finger only holds, two
                    // scratch; the app decides). Once holding, any finger keeps the hold, so a
                    // finger skipping off the surface mid-stroke doesn't let go.
                    let fingers = n as u8;
                    let dev = d.clone();
                    let msg = if n >= 1.0 {
                        let (x, y) = (sx / n, sy / n);
                        if !holding {
                            holding = true;
                            held_mask = mask;
                            held_ids = ids.clone();
                            Some(InEvent::Down { dev, kind, t, x, y, fingers })
                        } else if mask != held_mask || ids != held_ids {
                            held_mask = mask;
                            held_ids = ids.clone();
                            Some(InEvent::Anchor { dev, kind, x, y, fingers })
                        } else {
                            Some(InEvent::Move { dev, kind, t, x, y, fingers })
                        }
                    } else if holding {
                        holding = false;
                        Some(InEvent::Up { dev, kind })
                    } else {
                        None
                    };
                    if let Some(m) = msg {
                        if proxy.send_event(m).is_err() {
                            return;
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

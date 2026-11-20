//! Input that doesn't come through the window: touch surfaces (laptop touchpad, tablet
//! touch) and any other device with buttons, wheels or axes. What the rest of CUT sees is
//! the same on every system; how devices are read is per system (Linux: `input/linux.rs`).

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use winit::event_loop::EventLoopProxy;

use crate::devices::{Ctl, DevInfo};

#[cfg(target_os = "linux")]
mod linux;

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
// Only Linux reads devices directly so far; elsewhere most events aren't sent yet.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
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

/// What the touchpad is doing, written by its reader the moment events arrive. On Wayland
/// CUT can't switch the desktop's tap-to-click off, so the window uses this to recognise a
/// click the desktop made from a tap (see `desk::Desk::ignore_press`).
pub struct Pad {
    /// The touchpad's own button is pressed (a real click, not a tap).
    pub click: AtomicBool,
    /// When the last short touch (a tap) ended, in micros like `t`.
    pub tap_end: AtomicU64,
}

pub static PAD: Pad = Pad { click: AtomicBool::new(false), tap_end: AtomicU64::new(0) };

#[derive(Default)]
pub struct Status {
    pub touch: AtomicBool,
}

pub fn micros(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(0)
}

/// Starts reading devices. Returns the touch surfaces found at start and how many
/// touchpads/tablets couldn't be opened for lack of permission.
pub fn start(proxy: EventLoopProxy<InEvent>, status: Arc<Status>) -> (Vec<String>, usize) {
    #[cfg(target_os = "linux")]
    return linux::start(proxy, status);
    // No direct device reading here yet: the mouse and keyboard come through the window.
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (proxy, status);
        (Vec::new(), 0)
    }
}

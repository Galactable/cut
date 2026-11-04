//! Keeps the desktop (X11) from also acting on touches that CUT uses.
//!
//! CUT reads touch surfaces directly, but X still sees them too: a two-finger
//! scratch would scroll, tap or click whatever is under the cursor. So:
//! - while you hold the record on a touchpad, X's copy of that touchpad is paused
//!   (libinput "send events" off), and resumed when you let go;
//! - while CUT is focused, X stops using a tablet's touch surface entirely; it comes back
//!   when you switch away or quit;
//! - while CUT is focused and the pointer is over its window, touchpad tap-to-click is
//!   off (taps are record stops here; tap-and-drag would hold the click down for
//!   ~300 ms). Moving the pointer off the window brings tapping straight back, so you can
//!   tap another window to switch to it.
//! Everything is restored on exit, on a panic, and when killed.

use std::{
    process::Command,
    sync::{mpsc, Mutex},
};

const SEND_EVENTS: &str = "libinput Send Events Mode Enabled";
const TAPPING: &str = "libinput Tapping Enabled";

/// Run when CUT is killed (Ctrl+C, SIGTERM), before it exits: closes an open take.
pub static EXIT_HOOK: Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

/// Commands that put the desktop back the way we found it, for the panic hook.
static RESTORE: Mutex<Vec<Vec<String>>> = Mutex::new(Vec::new());

fn xinput(args: &[&str]) {
    let _ = Command::new("xinput").args(args).output();
}

pub struct Desk {
    touchpads: Vec<String>,
    /// Touchpads that had tap-to-click on, to switch off while focused.
    tappers: Vec<String>,
    tablet_touch: Vec<String>,
    tx: mpsc::Sender<Vec<String>>,
    paused: bool,
    taken: bool,
    taps_off: bool,
}

impl Desk {
    pub fn new() -> Self {
        let names = Command::new("xinput")
            .args(["list", "--name-only"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        let mut touchpads = Vec::new();
        let mut tappers = Vec::new();
        let mut tablet_touch = Vec::new();
        for n in names.lines().map(str::trim) {
            let l = n.to_lowercase();
            if l.contains("touchpad") {
                touchpads.push(n.to_string());
                let tapping = Command::new("xinput")
                    .args(["list-props", n])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|p| p.contains(&format!("{TAPPING} (")) && p.trim_end().ends_with('1')))
                    .unwrap_or(false);
                if tapping {
                    tappers.push(n.to_string());
                }
            } else if l.contains("finger touch") || (l.contains("wacom") && l.contains("touch")) {
                // Leave it alone if the user already switched it off themselves.
                let enabled = Command::new("xinput")
                    .args(["list-props", n])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|p| p.contains("Device Enabled") && p.trim_end().ends_with('1')))
                    .unwrap_or(false);
                if enabled {
                    tablet_touch.push(n.to_string());
                }
            }
        }
        // xinput takes a few ms per call; keep that off the UI thread, in order.
        let (tx, rx) = mpsc::channel::<Vec<String>>();
        std::thread::spawn(move || {
            for args in rx {
                let a: Vec<&str> = args.iter().map(String::as_str).collect();
                xinput(&a);
            }
        });
        let mut restore = Vec::new();
        for t in &touchpads {
            restore.push(vec!["set-prop".into(), t.clone(), SEND_EVENTS.into(), "0".into(), "0".into()]);
        }
        for t in &tablet_touch {
            restore.push(vec!["enable".into(), t.clone()]);
        }
        for t in &tappers {
            restore.push(vec!["set-prop".into(), t.clone(), TAPPING.into(), "1".into()]);
        }
        *RESTORE.lock().unwrap() = restore;
        // Being killed (Ctrl+C, SIGTERM) skips destructors, so restore from a handler too.
        let _ = ctrlc::set_handler(|| {
            restore_all();
            if let Some(hook) = EXIT_HOOK.lock().ok().and_then(|h| h.clone()) {
                hook();
            }
            std::process::exit(0);
        });
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_all();
            prev(info);
        }));
        Desk { touchpads, tappers, tablet_touch, tx, paused: false, taken: false, taps_off: false }
    }

    fn run(&self, args: &[&str]) {
        let _ = self.tx.send(args.iter().map(|s| s.to_string()).collect());
    }

    /// Pause (true) or resume (false) the desktop's use of the laptop touchpad.
    pub fn pause_touchpad(&mut self, pause: bool) {
        if pause == self.paused {
            return;
        }
        self.paused = pause;
        let v = if pause { "1" } else { "0" };
        for t in &self.touchpads {
            self.run(&["set-prop", t, SEND_EVENTS, v, "0"]);
        }
    }

    /// While focused: take the tablet's touch surface from the desktop and switch off
    /// touchpad tap-to-click. Unfocused: give both back.
    pub fn set_focused(&mut self, take: bool) {
        if take == self.taken {
            return;
        }
        self.taken = take;
        for t in &self.tablet_touch {
            self.run(&[if take { "disable" } else { "enable" }, t]);
        }
    }

    /// Tap-to-click off while the pointer is over CUT's focused window (taps are record
    /// stops there), back on as soon as it leaves, so a tap can still switch windows.
    pub fn set_taps_off(&mut self, off: bool) {
        if off == self.taps_off {
            return;
        }
        self.taps_off = off;
        for t in &self.tappers {
            self.run(&["set-prop", t, TAPPING, if off { "0" } else { "1" }]);
        }
    }
}

fn restore_all() {
    if let Ok(cmds) = RESTORE.lock() {
        for c in cmds.iter() {
            let a: Vec<&str> = c.iter().map(String::as_str).collect();
            xinput(&a);
        }
    }
}

impl Drop for Desk {
    fn drop(&mut self) {
        restore_all();
    }
}

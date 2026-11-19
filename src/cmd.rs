//! Everything CUT can be told to do, by name. The keyboard, the menus and device buttons
//! all point at these, so a command doesn't need a key: most only live in the menus (and
//! on any device button you give them), and the keyboard keeps just what you reach for
//! while playing.

use winit::keyboard::KeyCode;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    // File
    Open,
    Capture,
    PrevFile,
    NextFile,
    Save,
    SaveFormat,
    Profiles,
    Folders,
    Devices,
    Quit,
    // Section
    WholeSample,
    Zoom,
    Loop,
    BackToCue,
    /// From a menu or a plain command: jump to it. Holding a key or button sets it.
    HotCue(usize),
    // Deck
    Motor,
    Rpm33,
    Rpm45,
    Hamster,
    VolumeUp,
    VolumeDown,
    // Scratch
    Motion,
    Flip,
    LessSensitive,
    MoreSensitive,
    Smoother,
    Snappier,
    // Sound
    Eq,
    Noise,
    // Takes
    Record,
    Click,
    /// The free click's BPM, or with a beat grid, the track's tempo in fine steps.
    BpmDown,
    BpmUp,
    // Backing track
    LoadBacking,
    PlayBacking,
    BackingQuieter,
    BackingLouder,
    BackingInTakes,
    /// Turns the beat-fixing keys (`BEAT_KEYS`) on or off.
    FixBeat,
    Tap,
    HalfTempo,
    DoubleTempo,
    BeatsEarlier,
    BeatsLater,
    Redetect,
    // View
    Guide,
    Fullscreen,
    Background,
    RecordImage,
    LabelOnly,
    Dim,
    Grooves,
    CueLine,
    Trainer,
    FreezeStrip,
    ZoomStrip,
    WhatsNew,
}

impl Cmd {
    /// Commands that step a value up or down: their menu stays open, so you can click
    /// again.
    pub fn steps(self) -> bool {
        use Cmd::*;
        matches!(
            self,
            VolumeUp | VolumeDown | LessSensitive | MoreSensitive | Smoother | Snappier | Motion | BpmDown | BpmUp
                | BackingQuieter | BackingLouder | BeatsEarlier | BeatsLater | HalfTempo | DoubleTempo | Dim | ZoomStrip
        )
    }
}

/// The keyboard: (key, command, how the key is shown). Space (fader) and Shift (hold the
/// record) aren't commands and are handled before this. Quit needs Ctrl.
pub const KEYS: &[(KeyCode, Cmd, &str)] = &[
    (KeyCode::KeyM, Cmd::Motor, "M"),
    (KeyCode::KeyC, Cmd::BackToCue, "C"),
    (KeyCode::F2, Cmd::HotCue(0), "F2"),
    (KeyCode::F3, Cmd::HotCue(1), "F3"),
    (KeyCode::F4, Cmd::HotCue(2), "F4"),
    (KeyCode::KeyR, Cmd::Record, "R"),
    (KeyCode::KeyT, Cmd::Click, "T"),
    (KeyCode::KeyI, Cmd::Hamster, "I"),
    (KeyCode::ArrowUp, Cmd::VolumeUp, "Up"),
    (KeyCode::ArrowDown, Cmd::VolumeDown, "Down"),
    (KeyCode::KeyO, Cmd::Open, "O"),
    (KeyCode::KeyW, Cmd::Capture, "W"),
    (KeyCode::Comma, Cmd::PrevFile, ","),
    (KeyCode::Period, Cmd::NextFile, "."),
    (KeyCode::KeyS, Cmd::Save, "S"),
    (KeyCode::KeyA, Cmd::WholeSample, "A"),
    (KeyCode::KeyZ, Cmd::Zoom, "Z"),
    (KeyCode::KeyG, Cmd::Loop, "G"),
    (KeyCode::KeyY, Cmd::LoadBacking, "Y"),
    (KeyCode::KeyB, Cmd::PlayBacking, "B"),
    (KeyCode::KeyH, Cmd::Guide, "H"),
    (KeyCode::F11, Cmd::Fullscreen, "F11"),
    (KeyCode::F12, Cmd::Trainer, "F12"),
    (KeyCode::Backslash, Cmd::FreezeStrip, "\\"),
    (KeyCode::KeyQ, Cmd::Quit, "Ctrl+Q"),
];

/// Extra keys while fixing the beat (Backing > Fix the beat); Esc stops. With Ctrl,
/// earlier/later move a whole beat and the tempo steps by 1 BPM.
pub const BEAT_KEYS: &[(KeyCode, Cmd, &str)] = &[
    (KeyCode::Slash, Cmd::Tap, "/"),
    (KeyCode::Digit7, Cmd::HalfTempo, "7"),
    (KeyCode::Digit8, Cmd::DoubleTempo, "8"),
    (KeyCode::Semicolon, Cmd::BeatsEarlier, ";"),
    (KeyCode::Quote, Cmd::BeatsLater, "'"),
    (KeyCode::Digit9, Cmd::BpmDown, "9"),
    (KeyCode::Digit0, Cmd::BpmUp, "0"),
    (KeyCode::Digit1, Cmd::Redetect, "1"),
];

/// The command a key runs, if it has one (`fixing`: the beat keys are on too).
pub fn for_key(code: KeyCode, fixing: bool) -> Option<Cmd> {
    let beat = if fixing { BEAT_KEYS } else { &[] };
    KEYS.iter().chain(beat).find(|(k, _, _)| *k == code).map(|&(_, c, _)| c)
}

/// How a command's key is shown in the menus: blank for commands without one.
pub fn key_name(cmd: Cmd) -> &'static str {
    KEYS.iter().chain(BEAT_KEYS).find(|(_, c, _)| *c == cmd).map_or("", |&(_, _, n)| n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_key_does_one_thing() {
        for (i, (k, _, _)) in KEYS.iter().chain(BEAT_KEYS).enumerate() {
            assert!(!KEYS.iter().chain(BEAT_KEYS).skip(i + 1).any(|(o, _, _)| o == k), "{k:?} is used twice");
        }
    }

    #[test]
    fn beat_keys_only_while_fixing() {
        assert_eq!(for_key(KeyCode::Digit7, false), None);
        assert_eq!(for_key(KeyCode::Digit7, true), Some(Cmd::HalfTempo));
        assert_eq!(for_key(KeyCode::KeyM, true), Some(Cmd::Motor));
    }
}

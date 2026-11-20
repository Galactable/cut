//! MIDI controllers (DJ controllers, pad and knob boxes). Every controller is listened
//! to, including ones plugged in later; the app decides what the notes and CCs mean.
//! Linux reads them through the ALSA sequencer, Windows through its MIDI API (midir).

use winit::event_loop::EventLoopProxy;

use crate::{devices::Ctl, input::InEvent};

#[cfg(target_os = "linux")]
mod alsa;
#[cfg(windows)]
mod windows;

pub fn start(proxy: EventLoopProxy<InEvent>) {
    std::thread::spawn(move || {
        #[cfg(target_os = "linux")]
        let res = alsa::run(proxy);
        #[cfg(windows)]
        let res = windows::run(proxy);
        #[cfg(not(any(target_os = "linux", windows)))]
        let res: anyhow::Result<()> = {
            let _ = proxy;
            Ok(())
        };
        if let Err(e) = res {
            eprintln!("MIDI unavailable: {e}");
        }
    });
}

/// The control and value in one raw MIDI message, if it's one CUT uses: notes (note off
/// is velocity 0), CCs and pitch bend (centred on 0, -8192 to 8191).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse(msg: &[u8]) -> Option<(Ctl, i32)> {
    let (&status, data) = msg.split_first()?;
    let ch = status & 15;
    let byte = |i: usize| data.get(i).map(|b| b & 127);
    match status & 0xf0 {
        0x90 => Some((Ctl::Note(ch, byte(0)?), byte(1)? as i32)),
        0x80 => Some((Ctl::Note(ch, byte(0)?), 0)),
        0xb0 => Some((Ctl::Cc(ch, byte(0)?), byte(1)? as i32)),
        0xe0 => Some((Ctl::Bend(ch), ((byte(1)? as i32) << 7 | byte(0)? as i32) - 8192)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_notes_ccs_and_bend() {
        assert_eq!(parse(&[0x91, 60, 100]), Some((Ctl::Note(1, 60), 100)));
        assert_eq!(parse(&[0x81, 60, 64]), Some((Ctl::Note(1, 60), 0)));
        assert_eq!(parse(&[0xb0, 7, 127]), Some((Ctl::Cc(0, 7), 127)));
        assert_eq!(parse(&[0xe2, 0, 64]), Some((Ctl::Bend(2), 0)));
        assert_eq!(parse(&[0xe0, 127, 127]), Some((Ctl::Bend(0), 8191)));
        assert_eq!(parse(&[0xf8]), None);
        assert_eq!(parse(&[0x90, 60]), None);
    }
}

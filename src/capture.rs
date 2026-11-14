//! Records whatever the computer is playing (a video, a film, a browser tab) so it can go
//! straight onto the record, without recording it elsewhere and importing the file.
//!
//! `parec` reads the monitor of the default output, which works the same on PipeWire and
//! PulseAudio. The audio is kept in memory until the capture is stopped, then trimmed of
//! the silence around it and saved to the library.

use std::{
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed},
        mpsc, Arc,
    },
};

use crate::{deck::Source, library};

/// Longest capture, in seconds. Past this it stops by itself (about 230 MB in memory).
pub const MAX_SECS: u64 = 600;
/// Quieter than this is the silence before and after what was playing (about -80 dB).
const SILENT: f32 = 1e-4;

pub struct Capture {
    child: Child,
    pub rate: u32,
    /// Frames heard so far, and their recent peak, for the status bar.
    pub frames: Arc<AtomicU64>,
    pub level: Arc<AtomicU32>,
    /// Stop was asked for; the result is on its way.
    pub stopping: bool,
    stop: Arc<AtomicBool>,
    /// The saved file and its audio, once the capture has ended (by `stop`, by reaching
    /// `MAX_SECS`, or because the recorder failed).
    pub done: mpsc::Receiver<Result<(PathBuf, Source), String>>,
}

impl Capture {
    /// Starts recording the default output. `dir` is where the capture is saved.
    pub fn start(rate: u32, dir: PathBuf, mp3: bool) -> Result<Capture, String> {
        let mut child = Command::new("parec")
            .args([
                "--client-name=CUT",
                "--stream-name=recording what's playing",
                "--device=@DEFAULT_MONITOR@",
                "--format=float32le",
                "--channels=2",
                "--latency-msec=20",
                &format!("--rate={rate}"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => "recording what's playing needs `parec` (package pulseaudio-utils, or libpulse on Arch)".to_string(),
                _ => format!("couldn't start parec: {e}"),
            })?;
        let mut out = child.stdout.take().expect("piped");
        let mut err = child.stderr.take().expect("piped");
        let frames = Arc::new(AtomicU64::new(0));
        let level = Arc::new(AtomicU32::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, done) = mpsc::channel();
        let (frames2, level2, stop2) = (frames.clone(), level.clone(), stop.clone());
        std::thread::spawn(move || {
            let (mut l, mut r) = (Vec::new(), Vec::new());
            let mut buf = [0u8; 16384];
            // Bytes of a frame split across two reads.
            let mut rest: Vec<u8> = Vec::new();
            let max = MAX_SECS as usize * rate as usize;
            while l.len() < max {
                let n = match out.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                rest.extend_from_slice(&buf[..n]);
                let whole = rest.len() / 8 * 8;
                let mut peak = 0f32;
                for f in rest[..whole].chunks_exact(8) {
                    let a = f32::from_le_bytes([f[0], f[1], f[2], f[3]]);
                    let b = f32::from_le_bytes([f[4], f[5], f[6], f[7]]);
                    peak = peak.max(a.abs()).max(b.abs());
                    l.push(a);
                    r.push(b);
                }
                rest.drain(..whole);
                frames2.store(l.len() as u64, Relaxed);
                let old = f32::from_bits(level2.load(Relaxed));
                level2.store(peak.max(old * 0.8).to_bits(), Relaxed);
            }
            // The recorder ending by itself with nothing heard: say why.
            if l.is_empty() && !stop2.load(Relaxed) {
                let mut why = String::new();
                let _ = err.read_to_string(&mut why);
                let why = why.lines().last().unwrap_or("").trim().to_string();
                let _ = tx.send(Err(if why.is_empty() { "couldn't record what's playing".to_string() } else { format!("couldn't record what's playing: {why}") }));
                return;
            }
            let _ = tx.send(finish(l, r, rate, dir, mp3));
        });
        Ok(Capture { child, rate, frames, level, stopping: false, stop, done })
    }

    /// Ends the capture; the result arrives on `done` once it's trimmed and saved.
    pub fn stop(&mut self) {
        self.stopping = true;
        self.stop.store(true, Relaxed);
        let _ = self.child.kill();
    }

    pub fn secs(&self) -> u64 {
        self.frames.load(Relaxed) / self.rate as u64
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The part of a capture with sound in it: silence before the first sound and after the
/// last is dropped, keeping a short breath each side. None if it was silent throughout.
fn sounding(l: &[f32], r: &[f32], rate: u32) -> Option<(usize, usize)> {
    let loud = |i: usize| l[i].abs() > SILENT || r[i].abs() > SILENT;
    let first = (0..l.len()).find(|&i| loud(i))?;
    let last = (0..l.len()).rfind(|&i| loud(i))?;
    let (lead, tail) = (rate as usize / 20, rate as usize / 4);
    Some((first.saturating_sub(lead), (last + 1 + tail).min(l.len())))
}

fn finish(mut l: Vec<f32>, mut r: Vec<f32>, rate: u32, dir: PathBuf, mp3: bool) -> Result<(PathBuf, Source), String> {
    let Some((a, b)) = sounding(&l, &r, rate) else {
        return Err("nothing was playing, so there's nothing to load".to_string());
    };
    l.truncate(b);
    r.truncate(b);
    l.drain(..a);
    r.drain(..a);
    let stem = format!("capture-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    let path = library::unique(&dir, &stem, if mp3 { "mp3" } else { "wav" });
    let saved = if mp3 { library::save_mp3(&path, &l, &r, rate) } else { library::save_wav(&path, &l, &r, rate) };
    saved.map_err(|e| format!("couldn't save the capture: {e:#}"))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok((path, Source { l, r, rate, name }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_around_the_sound_is_trimmed() {
        let rate = 1000;
        let mut l = vec![0.0f32; 5000];
        for x in &mut l[2000..3000] {
            *x = 0.5;
        }
        let r = l.clone();
        // 50 ms kept before, 250 ms after.
        assert_eq!(sounding(&l, &r, rate), Some((1950, 3250)));
        assert_eq!(sounding(&[0.0; 100], &[0.0; 100], rate), None);
        // Sound right up to both ends stays whole.
        assert_eq!(sounding(&[0.5; 100], &[0.5; 100], rate), Some((0, 100)));
    }

    /// Needs a running sound server, so it's not part of the normal run:
    /// `cargo test -- --ignored live`.
    #[test]
    #[ignore]
    fn live_capture_starts_and_stops() {
        let dir = std::env::temp_dir().join(format!("cut-capture-live-{}", std::process::id()));
        let mut cap = Capture::start(48000, dir.clone(), false).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1000));
        let heard = cap.frames.load(Relaxed);
        assert!(heard > 24000, "only {heard} frames in a second");
        cap.stop();
        let res = cap.done.recv_timeout(std::time::Duration::from_secs(3)).expect("ends promptly");
        match res {
            Ok((path, src)) => eprintln!("captured {} frames to {}", src.len(), path.display()),
            Err(e) => assert!(e.contains("nothing was playing"), "{e}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_capture_is_saved_and_loads_back() {
        let dir = std::env::temp_dir().join(format!("cut-capture-{}", std::process::id()));
        let rate = 48000;
        let mut l = vec![0.0f32; rate as usize];
        l.extend((0..rate).map(|i| if i % 2 == 0 { 0.5 } else { -0.5 }));
        let r = l.clone();
        let (path, src) = finish(l, r, rate, dir.clone(), false).unwrap();
        assert_eq!(src.len(), rate as usize + rate as usize / 20);
        assert_eq!(crate::deck::load(&path).unwrap().len(), src.len());
        assert!(finish(vec![0.0; 100], vec![0.0; 100], rate, dir.clone(), false).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

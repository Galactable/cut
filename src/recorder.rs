//! Writes takes to WAV off the audio thread.
//!
//! The WAV header (which says how long the file is) is rewritten every half second, so a
//! crash, a kill or a power cut loses at most that much and leaves a file that plays.
//! Quitting the app closes the take properly first.

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering::Relaxed},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use rtrb::Consumer;

use crate::engine::Shared;

/// Below this the whole take counts as silence (about -80 dB).
const SILENT: f32 = 1e-4;

/// `takes` is read at the start of each take, so changing the folder applies to the next one.
/// `mp3`: when a take ends, convert it from WAV to MP3.
pub fn spawn(shared: Arc<Shared>, mut rx: Consumer<f32>, rate: u32, status: Arc<Mutex<Option<String>>>, takes: Arc<Mutex<PathBuf>>, mp3: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut writer: Option<(hound::WavWriter<std::io::BufWriter<std::fs::File>>, PathBuf)> = None;
        let mut checkpoint = Instant::now();
        // Loudest sample in the take so far: a take that never made a sound isn't kept.
        let mut peak = 0f32;
        // A take couldn't be opened: what the engine pushed before it stopped has to go,
        // or it would turn up at the start of the next take.
        let mut discard = false;
        loop {
            let quitting = shared.quitting.load(Relaxed);
            if discard {
                // Counted before looking at the flag: once the engine has stopped, all
                // of these are from the failed take. A new take sets the flag before it
                // pushes anything, so its audio is never among them.
                let stale = rx.slots();
                if !shared.recording.load(Relaxed) {
                    if let Ok(chunk) = rx.read_chunk(stale) {
                        chunk.commit_all();
                    }
                    discard = false;
                }
            }
            let on = shared.recording.load(Relaxed) && !quitting;
            if on && writer.is_none() && !discard {
                let dir = takes.lock().unwrap().clone();
                let _ = std::fs::create_dir_all(&dir);
                // Never the name of an earlier take: two takes can start in the same second.
                let path = crate::library::unique(&dir, &format!("take-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")), "wav");
                let spec = hound::WavSpec { channels: 2, sample_rate: rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
                match hound::WavWriter::create(&path, spec) {
                    Ok(w) => {
                        writer = Some((w, path));
                        checkpoint = Instant::now();
                        peak = 0.0;
                    }
                    Err(e) => {
                        *status.lock().unwrap() = Some(format!("can't record: {e}"));
                        shared.rec_request.store(false, Relaxed);
                        discard = true;
                    }
                }
            }
            // The engine only pushes while recording, so draining only with a file open
            // never throws away the first moments of a take. Whole frames only, so every
            // checkpoint is a valid file.
            if let Some((w, _)) = writer.as_mut() {
                while rx.slots() >= 2 {
                    for _ in 0..2 {
                        if let Ok(s) = rx.pop() {
                            peak = peak.max(s.abs());
                            let _ = w.write_sample(s);
                        }
                    }
                }
                if checkpoint.elapsed() >= Duration::from_millis(500) {
                    let _ = w.flush();
                    checkpoint = Instant::now();
                }
            }
            if !on {
                if let Some((w, path)) = writer.take() {
                    let silent = peak < SILENT;
                    let msg = match w.finalize() {
                        Ok(()) if silent => {
                            let _ = std::fs::remove_file(&path);
                            Some("take was silent (motor off or fader cut?), so it wasn't kept".to_string())
                        }
                        // No MP3 conversion while closing: the WAV is kept as it is.
                        Ok(()) if mp3.load(Relaxed) && !quitting => {
                            // On its own thread: a long take takes a while to encode, and
                            // this one has to be free to write the next take meanwhile,
                            // or the engine's buffer fills and that take loses audio.
                            *status.lock().unwrap() = Some("converting take to MP3...".into());
                            let status = status.clone();
                            std::thread::spawn(move || {
                                let msg = match crate::library::wav_to_mp3(&path) {
                                    Ok(p) => format!("saved take {}", p.display()),
                                    Err(e) => format!("kept the WAV take; MP3 conversion failed: {e:#}"),
                                };
                                *status.lock().unwrap() = Some(msg);
                            });
                            None
                        }
                        Ok(()) => Some(format!("saved take {}", path.display())),
                        Err(e) => Some(format!("take failed: {e}")),
                    };
                    if let Some(msg) = msg {
                        if quitting {
                            eprintln!("{msg}");
                        }
                        *status.lock().unwrap() = Some(msg);
                    }
                }
            }
            if quitting {
                shared.rec_closed.store(true, Relaxed);
                return;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_take_is_not_kept() {
        let dir = std::env::temp_dir().join(format!("cut-rec-silent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let shared = Arc::new(Shared::default());
        let status: Arc<Mutex<Option<String>>> = Arc::default();
        let (mut tx, rx) = rtrb::RingBuffer::new(48000);
        spawn(shared.clone(), rx, 48000, status.clone(), Arc::new(Mutex::new(dir.clone())), Arc::default());
        shared.recording.store(true, Relaxed);
        for _ in 0..9600 {
            let _ = tx.push(0.0);
        }
        std::thread::sleep(Duration::from_millis(100));
        shared.recording.store(false, Relaxed);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0), 0);
        assert!(status.lock().unwrap().as_deref().unwrap_or("").contains("silent"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Records one take of `frames` frames of a quiet tone and waits for it to close.
    fn one_take(shared: &Shared, tx: &mut rtrb::Producer<f32>, frames: usize) {
        shared.recording.store(true, Relaxed);
        let mut i = 0;
        while i < frames * 2 {
            if tx.push((i as f32 * 0.001).sin() * 0.1).is_ok() {
                i += 1;
            } else {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        std::thread::sleep(Duration::from_millis(60));
        shared.recording.store(false, Relaxed);
        std::thread::sleep(Duration::from_millis(60));
    }

    fn files(dir: &std::path::Path, ext: &str) -> usize {
        std::fs::read_dir(dir).map(|d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x == ext)).count()).unwrap_or(0)
    }

    #[test]
    fn takes_in_the_same_second_are_both_kept() {
        let dir = std::env::temp_dir().join(format!("cut-rec-same-second-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let shared = Arc::new(Shared::default());
        let (mut tx, rx) = rtrb::RingBuffer::new(48000);
        spawn(shared.clone(), rx, 48000, Arc::default(), Arc::new(Mutex::new(dir.clone())), Arc::default());
        one_take(&shared, &mut tx, 4800);
        one_take(&shared, &mut tx, 4800);
        assert_eq!(files(&dir, "wav"), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn next_take_starts_while_the_last_one_converts_to_mp3() {
        let dir = std::env::temp_dir().join(format!("cut-rec-mp3-busy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let shared = Arc::new(Shared::default());
        let (mut tx, rx) = rtrb::RingBuffer::new(48000 * 4);
        spawn(shared.clone(), rx, 48000, Arc::default(), Arc::new(Mutex::new(dir.clone())), Arc::new(AtomicBool::new(true)));
        // Long enough that encoding it takes well over the moment the next take needs.
        one_take(&shared, &mut tx, 48000 * 90);
        let mp3_done_before = files(&dir, "mp3") > 0;
        shared.recording.store(true, Relaxed);
        let t0 = Instant::now();
        while files(&dir, "wav") < 2 && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let opened_in = t0.elapsed();
        let mp3_done_when_opened = files(&dir, "mp3") > 0;
        shared.recording.store(false, Relaxed);
        assert!(!mp3_done_before && !mp3_done_when_opened, "test take converted too fast to tell");
        assert!(opened_in < Duration::from_millis(200), "next take waited {opened_in:?} for the MP3");
        // The conversion still finishes.
        let t0 = Instant::now();
        while files(&dir, "mp3") == 0 && t0.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(files(&dir, "mp3"), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_take_that_couldnt_be_opened_leaves_nothing_for_the_next() {
        let base = std::env::temp_dir().join(format!("cut-rec-failed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // A folder under a plain file can't be made, so the take can't be opened.
        std::fs::write(base.join("file"), b"").unwrap();
        let takes = Arc::new(Mutex::new(base.join("file").join("takes")));
        let shared = Arc::new(Shared::default());
        let status: Arc<Mutex<Option<String>>> = Arc::default();
        let (mut tx, rx) = rtrb::RingBuffer::new(48000);
        spawn(shared.clone(), rx, 48000, status.clone(), takes.clone(), Arc::default());
        // The engine starts the take and pushes some audio before it hears it failed.
        shared.rec_request.store(true, Relaxed);
        shared.recording.store(true, Relaxed);
        for _ in 0..2000 {
            let _ = tx.push(0.9);
        }
        std::thread::sleep(Duration::from_millis(60));
        assert!(!shared.rec_request.load(Relaxed), "the failed take is called off");
        assert!(status.lock().unwrap().as_deref().unwrap_or("").contains("can't record"));
        shared.recording.store(false, Relaxed);
        std::thread::sleep(Duration::from_millis(60));

        // The next take, with a folder that works, holds only its own audio.
        let good = base.join("takes");
        *takes.lock().unwrap() = good.clone();
        shared.recording.store(true, Relaxed);
        for _ in 0..1000 {
            let _ = tx.push(0.25);
        }
        std::thread::sleep(Duration::from_millis(60));
        shared.recording.store(false, Relaxed);
        std::thread::sleep(Duration::from_millis(60));
        let file = std::fs::read_dir(&good).unwrap().next().unwrap().unwrap().path();
        let samples: Vec<f32> = hound::WavReader::open(&file).unwrap().samples::<f32>().map(Result::unwrap).collect();
        assert_eq!(samples.len(), 1000);
        assert!(samples.iter().all(|&s| s == 0.25));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn take_survives_a_crash_and_closes_on_quit() {
        let dir = std::env::temp_dir().join(format!("cut-rec-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let shared = Arc::new(Shared::default());
        let (mut tx, rx) = rtrb::RingBuffer::new(48000 * 4);
        spawn(shared.clone(), rx, 48000, Arc::default(), Arc::new(Mutex::new(dir.clone())), Arc::default());
        shared.recording.store(true, Relaxed);
        for i in 0..48000 * 2 {
            let _ = tx.push((i as f32 * 0.001).sin());
        }
        std::thread::sleep(Duration::from_millis(800));
        let file = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
        // Mid-take, as a crash would leave it: already a readable WAV with the audio so far.
        let r = hound::WavReader::open(&file).expect("valid while recording");
        assert_eq!(r.duration(), 48000);
        // Quitting mid-take closes it properly.
        for _ in 0..1000 {
            let _ = tx.push(0.0);
        }
        std::thread::sleep(Duration::from_millis(50));
        shared.close_take();
        assert!(shared.rec_closed.load(Relaxed));
        let r = hound::WavReader::open(&file).unwrap();
        assert_eq!(r.duration(), 48000 + 500);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

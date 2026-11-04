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

/// `takes` is read at the start of each take, so changing the folder applies to the next one.
/// `mp3`: when a take ends, convert it from WAV to MP3.
pub fn spawn(shared: Arc<Shared>, mut rx: Consumer<f32>, rate: u32, status: Arc<Mutex<Option<String>>>, takes: Arc<Mutex<PathBuf>>, mp3: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut writer: Option<(hound::WavWriter<std::io::BufWriter<std::fs::File>>, PathBuf)> = None;
        let mut checkpoint = Instant::now();
        loop {
            let quitting = shared.quitting.load(Relaxed);
            let on = shared.recording.load(Relaxed) && !quitting;
            if on && writer.is_none() {
                let dir = takes.lock().unwrap().clone();
                let _ = std::fs::create_dir_all(&dir);
                let path = dir.join(format!("take-{}.wav", chrono::Local::now().format("%Y%m%d-%H%M%S")));
                let spec = hound::WavSpec { channels: 2, sample_rate: rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
                match hound::WavWriter::create(&path, spec) {
                    Ok(w) => {
                        writer = Some((w, path));
                        checkpoint = Instant::now();
                    }
                    Err(e) => {
                        *status.lock().unwrap() = Some(format!("can't record: {e}"));
                        shared.rec_request.store(false, Relaxed);
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
                    let msg = match w.finalize() {
                        // No MP3 conversion while closing: the WAV is kept as it is.
                        Ok(()) if mp3.load(Relaxed) && !quitting => {
                            *status.lock().unwrap() = Some("converting take to MP3...".into());
                            match crate::library::wav_to_mp3(&path) {
                                Ok(p) => format!("saved take {}", p.display()),
                                Err(e) => format!("kept the WAV take; MP3 conversion failed: {e:#}"),
                            }
                        }
                        Ok(()) => format!("saved take {}", path.display()),
                        Err(e) => format!("take failed: {e}"),
                    };
                    if quitting {
                        eprintln!("{msg}");
                    }
                    *status.lock().unwrap() = Some(msg);
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

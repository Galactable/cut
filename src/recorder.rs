//! Writes takes to WAV off the audio thread.

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering::Relaxed},
        Arc, Mutex,
    },
    time::Duration,
};

use rtrb::Consumer;

use crate::engine::Shared;

/// `takes` is read at the start of each take, so changing the folder applies to the next one.
/// `mp3`: when a take ends, convert it from WAV to MP3.
pub fn spawn(shared: Arc<Shared>, mut rx: Consumer<f32>, rate: u32, status: Arc<Mutex<Option<String>>>, takes: Arc<Mutex<PathBuf>>, mp3: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut writer: Option<(hound::WavWriter<std::io::BufWriter<std::fs::File>>, PathBuf)> = None;
        loop {
            let on = shared.recording.load(Relaxed);
            if on && writer.is_none() {
                let dir = takes.lock().unwrap().clone();
                let _ = std::fs::create_dir_all(&dir);
                let path = dir.join(format!("take-{}.wav", chrono::Local::now().format("%Y%m%d-%H%M%S")));
                let spec = hound::WavSpec { channels: 2, sample_rate: rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
                match hound::WavWriter::create(&path, spec) {
                    Ok(w) => writer = Some((w, path)),
                    Err(e) => {
                        *status.lock().unwrap() = Some(format!("can't record: {e}"));
                        shared.rec_request.store(false, Relaxed);
                    }
                }
            }
            // The engine only pushes while recording, so draining only with a file open
            // never throws away the first moments of a take.
            if let Some((w, _)) = writer.as_mut() {
                while let Ok(s) = rx.pop() {
                    let _ = w.write_sample(s);
                }
            }
            if !on {
                if let Some((w, path)) = writer.take() {
                    let msg = match w.finalize() {
                        Ok(()) if mp3.load(Relaxed) => {
                            *status.lock().unwrap() = Some("converting take to MP3...".into());
                            match crate::library::wav_to_mp3(&path) {
                                Ok(p) => format!("saved take {}", p.display()),
                                Err(e) => format!("kept the WAV take; MP3 conversion failed: {e:#}"),
                            }
                        }
                        Ok(()) => format!("saved take {}", path.display()),
                        Err(e) => format!("take failed: {e}"),
                    };
                    *status.lock().unwrap() = Some(msg);
                }
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    });
}

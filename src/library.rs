//! The sample library: <samples folder>/<Folder>/<name>.wav (default ~/Music/CUT)

use std::path::{Path, PathBuf};

pub const FOLDERS: &str = "Vocals|Cuts|Horns|Stabs|Drums|FX|Other";
const AUDIO_EXT: [&str; 8] = ["wav", "mp3", "flac", "ogg", "aif", "aiff", "m4a", "opus"];

pub fn root() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| ".".into());
    home.join("Music").join("CUT")
}

/// Keeps names filesystem- and FL-friendly.
pub fn clean(s: &str) -> String {
    let s: String = s
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_().,'&".contains(c) { c } else { '_' })
        .collect();
    s.trim().to_string()
}

/// `dir/name.ext`, or `name 2.ext`, `name 3.ext`... if taken.
pub fn unique(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{name}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|i| dir.join(format!("{name} {i}.{ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

/// 320 kbps stereo MP3: about a fifth the size of a 24-bit WAV, and plenty for scratch
/// samples. (MP3 adds a few ms of silence at the start; use WAV where alignment is critical.)
pub fn save_mp3(path: &Path, l: &[f32], r: &[f32], rate: u32) -> anyhow::Result<()> {
    use mp3lame_encoder::{Bitrate, Builder, DualPcm, FlushNoGap, Quality};
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut b = Builder::new().ok_or_else(|| anyhow::anyhow!("couldn't start the MP3 encoder"))?;
    let e = |x: &dyn std::fmt::Debug| anyhow::anyhow!("MP3 encoder: {x:?}");
    b.set_num_channels(2).map_err(|x| e(&x))?;
    b.set_sample_rate(rate).map_err(|x| e(&x))?;
    b.set_brate(Bitrate::Kbps320).map_err(|x| e(&x))?;
    b.set_quality(Quality::Best).map_err(|x| e(&x))?;
    let mut enc = b.build().map_err(|x| e(&x))?;
    let clip = |v: &[f32]| v.iter().map(|x| x.clamp(-1.0, 1.0)).collect::<Vec<f32>>();
    let mut out = Vec::new();
    for start in (0..l.len()).step_by(16384) {
        let end = (start + 16384).min(l.len());
        let (cl, cr) = (clip(&l[start..end]), clip(&r[start..end]));
        out.reserve(mp3lame_encoder::max_required_buffer_size(end - start));
        enc.encode_to_vec(DualPcm { left: &cl, right: &cr }, &mut out).map_err(|x| e(&x))?;
    }
    out.reserve(7200);
    enc.flush_to_vec::<FlushNoGap>(&mut out).map_err(|x| e(&x))?;
    std::fs::write(path, out)?;
    Ok(())
}

/// Converts a finished WAV take to MP3 next to it and removes the WAV.
pub fn wav_to_mp3(wav: &Path) -> anyhow::Result<PathBuf> {
    let mut reader = hound::WavReader::open(wav)?;
    let rate = reader.spec().sample_rate;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let full = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|v| v.map(|v| v as f32 / full)).collect::<Result<_, _>>()?
        }
    };
    let l: Vec<f32> = samples.iter().step_by(2).copied().collect();
    let r: Vec<f32> = samples.iter().skip(1).step_by(2).copied().collect();
    let mp3 = wav.with_extension("mp3");
    save_mp3(&mp3, &l, &r, rate)?;
    std::fs::remove_file(wav)?;
    Ok(mp3)
}

/// 24-bit WAV, which FL Studio and everything else reads.
pub fn save_wav(path: &Path, l: &[f32], r: &[f32], rate: u32) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let spec = hound::WavSpec { channels: 2, sample_rate: rate, bits_per_sample: 24, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec)?;
    let full = (1 << 23) as f32 - 1.0;
    for (a, b) in l.iter().zip(r) {
        w.write_sample((a.clamp(-1.0, 1.0) * full) as i32)?;
        w.write_sample((b.clamp(-1.0, 1.0) * full) as i32)?;
    }
    w.finalize()?;
    Ok(())
}

/// The audio file `step` places before/after `current` in its folder (wrapping).
pub fn neighbour(current: &Path, step: isize) -> Option<PathBuf> {
    let dir = current.parent()?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| AUDIO_EXT.contains(&e.to_ascii_lowercase().as_str()))
        })
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    let i = files.iter().position(|p| p == current)? as isize;
    let n = files.len() as isize;
    Some(files[(i + step).rem_euclid(n) as usize].clone())
}

/// Moves everything in `from` into `to` (merging; anything already there by the same name
/// is left where it was). Returns (moved, skipped).
pub fn move_contents(from: &Path, to: &Path) -> anyhow::Result<(usize, usize)> {
    if to.starts_with(from) {
        anyhow::bail!("can't move a folder into itself");
    }
    std::fs::create_dir_all(to)?;
    let (mut moved, mut skipped) = (0, 0);
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if target.exists() {
            skipped += 1;
            continue;
        }
        // rename is instant on the same disk; across disks, copy then delete.
        if std::fs::rename(entry.path(), &target).is_err() {
            copy_all(&entry.path(), &target)?;
            if entry.path().is_dir() {
                std::fs::remove_dir_all(entry.path())?;
            } else {
                std::fs::remove_file(entry.path())?;
            }
        }
        moved += 1;
    }
    // Tidy up the old folder if that emptied it.
    let _ = std::fs::remove_dir(from);
    Ok((moved, skipped))
}

fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            copy_all(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

/// `~/...` for display.
pub fn pretty(p: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    match p.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => p.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_and_merges() {
        let base = std::env::temp_dir().join(format!("cut-test-{}", std::process::id()));
        let (from, to) = (base.join("old"), base.join("new"));
        std::fs::create_dir_all(from.join("Vocals")).unwrap();
        std::fs::create_dir_all(from.join("Takes")).unwrap();
        std::fs::write(from.join("Vocals/ahh.wav"), b"a").unwrap();
        std::fs::write(from.join("Takes/take-1.wav"), b"t").unwrap();
        std::fs::write(from.join("clash.wav"), b"old").unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(to.join("clash.wav"), b"new").unwrap();

        let (moved, skipped) = move_contents(&from, &to).unwrap();
        assert_eq!((moved, skipped), (2, 1));
        assert_eq!(std::fs::read(to.join("Vocals/ahh.wav")).unwrap(), b"a");
        assert_eq!(std::fs::read(to.join("Takes/take-1.wav")).unwrap(), b"t");
        // A same-named file already there is never overwritten; the old one stays put.
        assert_eq!(std::fs::read(to.join("clash.wav")).unwrap(), b"new");
        assert_eq!(std::fs::read(from.join("clash.wav")).unwrap(), b"old");
        assert!(move_contents(&to, &to.join("inside")).is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }
}

#[cfg(test)]
mod mp3_tests {
    use super::*;

    #[test]
    fn writes_a_playable_mp3() {
        let dir = std::env::temp_dir().join(format!("cut-mp3-{}", std::process::id()));
        let rate = 44100;
        let l: Vec<f32> = (0..rate).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5).collect();
        let wav = dir.join("t.wav");
        save_wav(&wav, &l, &l, rate as u32).unwrap();
        let mp3 = wav_to_mp3(&wav).unwrap();
        assert!(!wav.exists());
        let wav_size = 44 + rate * 2 * 3;
        let mp3_size = std::fs::metadata(&mp3).unwrap().len() as usize;
        assert!(mp3_size > 10_000 && mp3_size < wav_size / 3, "mp3 {mp3_size} bytes vs wav {wav_size}");
        // Reads back through the same decoder the app loads files with.
        let back = crate::deck::load(&mp3).unwrap();
        assert!(back.len() as f64 > rate as f64 * 0.95);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

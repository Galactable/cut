//! Open and save dialogs. Linux uses `zenity`; Windows uses its own native dialogs. Each
//! call blocks until the person answers, so run it off the UI thread.

use std::path::{Path, PathBuf};

/// What a file dialog shows.
#[derive(Clone, Copy)]
pub enum Files {
    Audio,
    Images,
}

impl Files {
    fn name(self) -> &'static str {
        match self {
            Files::Audio => "Audio",
            Files::Images => "Images",
        }
    }

    fn exts(self) -> &'static [&'static str] {
        match self {
            Files::Audio => &["wav", "mp3", "flac", "ogg", "aif", "aiff", "m4a", "opus"],
            Files::Images => &["png", "jpg", "jpeg", "webp"],
        }
    }
}

/// How the person said to save a section.
pub enum SaveAs {
    /// From the save form (Linux): (name, folder, new folder), each possibly blank.
    #[cfg_attr(windows, allow(dead_code))]
    Form(String, String, String),
    /// A file chosen in the system's Save As dialog (Windows).
    #[cfg_attr(not(windows), allow(dead_code))]
    Path(PathBuf),
}

/// Asks for a file to open, starting in `start`.
pub fn open_file(title: &str, start: &Path, files: Files) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new()
            .set_title(title)
            .set_directory(start)
            .add_filter(files.name(), files.exts())
            .add_filter("All files", &["*"])
            .pick_file()
    }
    #[cfg(not(windows))]
    {
        let globs: Vec<String> = files.exts().iter().flat_map(|e| [format!("*.{e}"), format!("*.{}", e.to_uppercase())]).collect();
        let out = std::process::Command::new("zenity")
            .args([
                "--file-selection",
                &format!("--title={title}"),
                &format!("--filename={}/", start.display()),
                &format!("--file-filter={} | {}", files.name(), globs.join(" ")),
                "--file-filter=All files | *",
            ])
            .output();
        out.ok().filter(|o| o.status.success()).map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
    }
}

/// Asks where to save a section. On Linux: a form with a name, a folder from `folders`
/// and a new-folder box, under `samples`. On Windows: the Save As dialog, starting in
/// `samples` with `suggested` as the name and `ext` as the type.
pub fn save_section(samples: &Path, folders: &str, suggested: &str, ext: &str) -> Option<SaveAs> {
    #[cfg(windows)]
    {
        let _ = folders;
        let _ = std::fs::create_dir_all(samples);
        rfd::FileDialog::new()
            .set_title("Save section to library")
            .set_directory(samples)
            .set_file_name(format!("{suggested}.{ext}"))
            .add_filter(ext.to_uppercase(), &[ext])
            .save_file()
            .map(SaveAs::Path)
    }
    #[cfg(not(windows))]
    {
        let _ = (samples, suggested, ext);
        let out = std::process::Command::new("zenity")
            .args([
                "--forms",
                "--title=Save section to library",
                "--text=Blank name = automatic. Type a new folder name to create one.",
                "--add-entry=Name",
                "--add-combo=Folder",
                &format!("--combo-values={folders}"),
                "--add-entry=...or new folder",
                &format!("--separator={FORM_SEP}"),
            ])
            .output();
        out.ok().filter(|o| o.status.success()).map(|o| {
            let (name, folder, new_folder) = form_fields(&String::from_utf8_lossy(&o.stdout));
            SaveAs::Form(name, folder, new_folder)
        })
    }
}

/// Between the save form's fields in zenity's answer: a character nobody types into a
/// name (zenity's own default, `|`, can be part of one).
#[cfg_attr(windows, allow(dead_code))]
const FORM_SEP: char = '\u{1f}';

/// The save form's answer: (name, folder, new folder).
#[cfg_attr(windows, allow(dead_code))]
fn form_fields(out: &str) -> (String, String, String) {
    let out = out.strip_suffix('\n').unwrap_or(out);
    let mut it = out.split(FORM_SEP).map(String::from);
    (it.next().unwrap_or_default(), it.next().unwrap_or_default(), it.next().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_in_the_name_stays_in_the_name() {
        let out = format!("scratch | take{FORM_SEP}Cuts{FORM_SEP}\n");
        assert_eq!(form_fields(&out), ("scratch | take".into(), "Cuts".into(), String::new()));
        let out = format!("{FORM_SEP}{FORM_SEP}My | Folder\n");
        assert_eq!(form_fields(&out), (String::new(), String::new(), "My | Folder".into()));
    }
}

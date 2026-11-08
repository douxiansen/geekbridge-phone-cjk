// SPDX-License-Identifier: GPL-3.0-or-later
//! The few things the UI needs from the operating system that are not drawing: where recordings
//! go by default and opening a folder in the file manager. Each has one small implementation per
//! OS, so a Windows or macOS port only fills in the `cfg` arms.

use std::io;
use std::path::{Path, PathBuf};

/// Where call recordings go when the user chose nothing: `~/Music/GeekBridge Calls`, or the
/// platform's music folder.
pub fn default_recordings_dir() -> PathBuf {
    dirs::audio_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Music")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("GeekBridge Calls")
}

/// `path` with the home folder written as `~`, for showing to people.
pub fn tilde(path: &Path) -> String {
    tilde_with(path, dirs::home_dir().as_deref())
}

fn tilde_with(path: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home {
        if let Ok(rest) = path.strip_prefix(home) {
            return if rest.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", rest.display())
            };
        }
    }
    path.display().to_string()
}

/// Expands a leading `~` the user typed.
pub fn expand_tilde(text: &str) -> PathBuf {
    if let Some(rest) = text.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    if text == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(text)
}

/// Opens a folder in the system's file manager, creating it first if it does not exist yet.
pub fn open_folder(path: &Path) -> io::Result<()> {
    ensure_dir(path)?;
    open_with_system(path)
}

/// Creates `path` (and its parents) when missing.
pub fn ensure_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

#[cfg(target_os = "linux")]
fn open_with_system(path: &Path) -> io::Result<()> {
    std::process::Command::new("xdg-open")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(target_os = "macos")]
fn open_with_system(path: &Path) -> io::Result<()> {
    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(target_os = "windows")]
fn open_with_system(path: &Path) -> io::Result<()> {
    std::process::Command::new("explorer")
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn open_with_system(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "opening folders is not supported on this system",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_folder_is_shown_as_a_tilde() {
        let home = Path::new("/home/ana");
        assert_eq!(
            tilde_with(Path::new("/home/ana/Music/GeekBridge Calls"), Some(home)),
            "~/Music/GeekBridge Calls"
        );
        assert_eq!(tilde_with(Path::new("/home/ana"), Some(home)), "~");
        assert_eq!(
            tilde_with(Path::new("/srv/calls"), Some(home)),
            "/srv/calls"
        );
        assert_eq!(
            tilde_with(Path::new("/home/anabel/x"), Some(home)),
            "/home/anabel/x"
        );
        assert_eq!(tilde_with(Path::new("/x"), None), "/x");
    }

    #[test]
    fn the_default_folder_is_named_for_the_app() {
        assert!(default_recordings_dir().ends_with("GeekBridge Calls"));
    }

    #[test]
    fn a_typed_tilde_is_expanded_but_other_text_is_kept() {
        assert_eq!(expand_tilde("/srv/calls"), PathBuf::from("/srv/calls"));
        assert_eq!(expand_tilde("relative/dir"), PathBuf::from("relative/dir"));
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expand_tilde("~/Calls"), home.join("Calls"));
            assert_eq!(expand_tilde("~"), home);
        }
    }

    #[test]
    fn a_missing_folder_is_created() {
        // Opening one in a file manager is not for a test; only the creation is checked.
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a").join("b");
        ensure_dir(&target).unwrap();
        ensure_dir(&target).unwrap();
        assert!(target.is_dir());
    }
}

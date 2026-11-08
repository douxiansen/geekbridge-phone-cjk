// SPDX-License-Identifier: GPL-3.0-or-later
//! Small JSON files in the user's config and data folders.

use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const APP_DIR: &str = "geekbridge-phone";

/// Where the app keeps its files.
#[derive(Debug, Clone)]
pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
}

impl Paths {
    /// The platform's standard folders.
    pub fn standard() -> Self {
        Self {
            config: dirs::config_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join(APP_DIR),
            data: dirs::data_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join(APP_DIR),
        }
    }

    /// Both folders under one root (for tests).
    pub fn under(root: &Path) -> Self {
        Self {
            config: root.join("config"),
            data: root.join("data"),
        }
    }
}

/// Reads `path`, or returns the default when it is missing. A file that cannot
/// be read is set aside as `.bad` so it is not overwritten blindly.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> T {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            log::warn!(
                "{} is unreadable ({e}); keeping it as .bad and starting fresh",
                path.display()
            );
            let _ = fs::rename(path, path.with_extension("bad"));
            T::default()
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => T::default(),
        Err(e) => {
            log::warn!("cannot read {}: {e}", path.display());
            T::default()
        }
    }
}

/// Writes `value` atomically: a temp file, then a rename, so a crash never
/// leaves half a file.
pub fn save<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(value).map_err(io::Error::other)?,
    )?;
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gbp-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn round_trips() {
        let dir = temp("rt");
        let p = dir.join("s.json");
        let s = Settings {
            notify_texts: false,
            ..Settings::default()
        };
        save(&p, &s).unwrap();
        assert_eq!(load::<Settings>(&p), s);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_file_is_default_and_corrupt_is_set_aside() {
        let dir = temp("bad");
        let p = dir.join("s.json");
        assert_eq!(load::<Settings>(&p), Settings::default());
        fs::create_dir_all(&dir).unwrap();
        fs::write(&p, b"{ not json").unwrap();
        assert_eq!(load::<Settings>(&p), Settings::default());
        assert!(dir.join("s.bad").exists());
        let _ = fs::remove_dir_all(dir);
    }
}

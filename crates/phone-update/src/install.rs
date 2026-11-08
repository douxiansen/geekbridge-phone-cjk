// SPDX-License-Identifier: GPL-3.0-or-later
//! Replacing the running program with a verified release.

use crate::release::UpdateError;
use flate2::read::GzDecoder;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// The program inside the release tarball, relative to its top folder.
const BINARY_IN_TARBALL: &str = "bin/geekbridge-phone";
/// The largest binary accepted from a tarball.
const MAX_BINARY_BYTES: u64 = 200 * 1024 * 1024;

/// Whether `path` is a safe relative path (no `..`, not absolute).
fn is_safe_relative(path: &Path) -> bool {
    path.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        && !path.has_root()
}

/// Reads the program out of the tarball without extracting anything else.
fn read_binary(tarball: &Path) -> Result<Vec<u8>, UpdateError> {
    let file = std::fs::File::open(tarball)?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    for entry in archive
        .entries()
        .map_err(|e| UpdateError::Install(e.to_string()))?
    {
        let entry = entry.map_err(|e| UpdateError::Install(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| UpdateError::Install(e.to_string()))?
            .into_owned();
        if !is_safe_relative(&path) {
            return Err(UpdateError::Refused(format!(
                "the release contains an unsafe path: {}",
                path.display()
            )));
        }
        // <top folder>/bin/geekbridge-phone
        let mut parts = path.components();
        parts.next();
        if parts.as_path() == Path::new(BINARY_IN_TARBALL) && entry.header().entry_type().is_file()
        {
            let mut data = Vec::new();
            entry.take(MAX_BINARY_BYTES + 1).read_to_end(&mut data)?;
            if data.len() as u64 > MAX_BINARY_BYTES {
                return Err(UpdateError::Refused(
                    "the program in the release is larger than expected".into(),
                ));
            }
            return Ok(data);
        }
    }
    Err(UpdateError::Install(format!(
        "the release has no {BINARY_IN_TARBALL}"
    )))
}

/// Installs the program from a verified `tarball` over `target` (the running executable).
///
/// The new program is written next to the old one, checked to run (`--version`), and swapped in
/// with a rename, which is atomic and safe while the old program is running. The old one is kept
/// as `<target>.previous` so a bad release can be rolled back by hand.
pub fn install(tarball: &Path, target: &Path) -> Result<PathBuf, UpdateError> {
    let binary = read_binary(tarball)?;
    let dir = target
        .parent()
        .ok_or_else(|| UpdateError::Install("no folder for the program".into()))?;
    let staged = dir.join(".geekbridge-phone.new");
    write_executable(&staged, &binary)?;
    if let Err(e) = smoke_test(&staged) {
        let _ = std::fs::remove_file(&staged);
        return Err(e);
    }
    let previous = target.with_extension("previous");
    let _ = std::fs::remove_file(&previous);
    if target.exists() {
        // A hard link keeps the old program under its new name without copying it.
        if std::fs::hard_link(target, &previous).is_err() {
            let _ = std::fs::copy(target, &previous);
        }
    }
    std::fs::rename(&staged, target).map_err(|e| {
        let _ = std::fs::remove_file(&staged);
        UpdateError::Install(format!("cannot replace {}: {e}", target.display()))
    })?;
    Ok(previous)
}

fn write_executable(path: &Path, data: &[u8]) -> Result<(), UpdateError> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o755);
    }
    let mut f = opts.open(path)?;
    f.write_all(data)?;
    f.sync_all()?;
    Ok(())
}

/// The new program must run and answer `--version`.
///
/// Running a file that was just written can fail with "text file busy" (ETXTBSY): if another
/// thread starts a subprocess while our write handle is open, the child briefly inherits that
/// handle and `exec` of the file is refused until the child lets go. It passes within
/// milliseconds, so the test is simply retried.
fn smoke_test(path: &Path) -> Result<(), UpdateError> {
    const TEXT_FILE_BUSY: i32 = 26;
    let mut attempt = 0;
    let out = loop {
        match std::process::Command::new(path).arg("--version").output() {
            Ok(out) => break out,
            Err(e) if e.raw_os_error() == Some(TEXT_FILE_BUSY) && attempt < 40 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                return Err(UpdateError::Install(format!(
                    "the new version does not run: {e}"
                )))
            }
        }
    };
    if !out.status.success() {
        return Err(UpdateError::Install(
            "the new version failed its self-test".into(),
        ));
    }
    Ok(())
}

/// Whether the running program can be replaced (it lives in a folder we can write to, as in a
/// per-user install; a system package must be updated with the package manager).
pub fn can_replace(target: &Path) -> bool {
    let Some(dir) = target.parent() else {
        return false;
    };
    let probe = dir.join(".geekbridge-phone.probe");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;

    /// A tarball with the given (path, contents, mode) entries.
    fn tarball(entries: &[(&str, &[u8], u32)]) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        let gz = GzEncoder::new(
            std::fs::File::create(file.path()).unwrap(),
            Compression::fast(),
        );
        let mut b = tar::Builder::new(gz);
        for (path, data, mode) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(*mode);
            h.set_entry_type(tar::EntryType::Regular);
            h.set_cksum();
            // `append_data` would reject `..`, which is exactly what one test needs to write.
            let mut raw = h.clone();
            raw.set_path("placeholder").unwrap();
            let name = path.as_bytes();
            raw.as_old_mut().name[..name.len()].copy_from_slice(name);
            raw.set_cksum();
            b.append(&raw, *data).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap();
        file
    }

    const GOOD: &[u8] = b"#!/bin/sh\necho geekbridge-phone 0.2.0\n";

    #[test]
    fn installs_the_binary_atomically_and_keeps_the_previous_one() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("geekbridge-phone");
        std::fs::write(&target, b"#!/bin/sh\necho old\n").unwrap();
        let tar = tarball(&[
            (
                "geekbridge-phone-0.2.0-linux-x86_64/bin/geekbridge-phone",
                GOOD,
                0o755,
            ),
            (
                "geekbridge-phone-0.2.0-linux-x86_64/README.md",
                b"hi",
                0o644,
            ),
        ]);
        let previous = install(tar.path(), &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), GOOD);
        assert_eq!(std::fs::read(&previous).unwrap(), b"#!/bin/sh\necho old\n");
        assert!(!dir.path().join(".geekbridge-phone.new").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&target).unwrap().permissions().mode() & 0o111,
                0o111
            );
        }
    }

    #[test]
    fn a_binary_that_will_not_run_is_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("geekbridge-phone");
        std::fs::write(&target, b"#!/bin/sh\necho old\n").unwrap();
        let tar = tarball(&[(
            "g-0.2.0/bin/geekbridge-phone",
            b"#!/bin/sh\nexit 3\n",
            0o755,
        )]);
        assert!(install(tar.path(), &target).is_err());
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"#!/bin/sh\necho old\n",
            "the old program is untouched"
        );
        assert!(!dir.path().join(".geekbridge-phone.new").exists());
    }

    #[test]
    fn a_tarball_without_the_binary_or_with_unsafe_paths_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("geekbridge-phone");
        std::fs::write(&target, b"old").unwrap();
        let none = tarball(&[("g/README.md", b"x", 0o644)]);
        assert!(matches!(
            install(none.path(), &target),
            Err(UpdateError::Install(_))
        ));
        let evil = tarball(&[
            ("../evil", b"x", 0o755),
            ("g/bin/geekbridge-phone", GOOD, 0o755),
        ]);
        assert!(matches!(
            install(evil.path(), &target),
            Err(UpdateError::Refused(_))
        ));
        let abs = tarball(&[("/etc/passwd", b"x", 0o644)]);
        assert!(matches!(
            install(abs.path(), &target),
            Err(UpdateError::Refused(_))
        ));
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
    }

    #[test]
    fn can_replace_reflects_folder_permissions() {
        let dir = tempfile::tempdir().unwrap();
        assert!(can_replace(&dir.path().join("geekbridge-phone")));
        assert!(!can_replace(Path::new("/proc/geekbridge-phone")));
    }
}

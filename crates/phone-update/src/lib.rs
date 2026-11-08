// SPDX-License-Identifier: GPL-3.0-or-later
//! Self-update from GitHub releases.
//!
//! A release carries a tarball per platform, a detached Ed25519 signature of its SHA-256
//! digest (`<tarball>.sig`, hex) and a `SHA256SUMS` file. The client checks the latest release
//! over HTTPS, downloads the tarball for its platform, verifies the checksum and the signature
//! against the public key built into the app, and only then replaces its own binary. A
//! compromised download host or a tampered asset therefore cannot get code onto the machine.

pub mod http;
pub mod install;
pub mod release;
pub mod verify;
pub mod version;

pub use http::{Http, UreqHttp};
pub use release::{Update, UpdateError};
pub use version::Version;

/// The signing public key (hex, 32 bytes) whose private half signs releases in CI.
/// Replaced by `scripts/rotate-update-key.sh`; see `docs/RELEASING.md`.
pub const PUBLIC_KEY_HEX: &str = include_str!("public_key.hex");

/// The repository releases are read from.
pub const REPO: &str = "fspecii/geekbridge-phone";

/// The platform suffix of release assets, such as `linux-x86_64`.
pub fn platform_suffix() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

use std::path::{Path, PathBuf};

/// Downloads `update`, verifies its checksum and signature, and installs it over `exe`.
/// `work_dir` holds the download while it is checked (it is deleted afterwards).
/// Returns where the previous program was kept.
pub fn apply(
    http: &dyn Http,
    update: &Update,
    work_dir: &Path,
    exe: &Path,
) -> Result<PathBuf, UpdateError> {
    let tarball = release::download_and_verify(http, update, work_dir, PUBLIC_KEY_HEX)?;
    let result = install::install(&tarball, exe);
    let _ = std::fs::remove_file(&tarball);
    result
}

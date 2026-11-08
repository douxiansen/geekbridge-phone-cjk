// SPDX-License-Identifier: GPL-3.0-or-later
//! The whole update flow against a fake GitHub: check, download, verify, install.
//! The signature check uses the key built into the app, so the fixtures are signed with a
//! throwaway key and the test goes through `release`/`install` directly with that key.

use ed25519_dalek::SigningKey;
use flate2::write::GzEncoder;
use flate2::Compression;
use phone_update::release::{self, UpdateError};
use phone_update::verify::{hex, sign_digest};
use phone_update::{install, Http, Version};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

struct Fake {
    text: HashMap<String, String>,
    files: HashMap<String, Vec<u8>>,
}

impl Http for Fake {
    fn get_text(&self, url: &str) -> Result<String, UpdateError> {
        self.text
            .get(url)
            .cloned()
            .ok_or_else(|| UpdateError::Network(format!("404 {url}")))
    }
    fn download(&self, url: &str, dest: &Path) -> Result<String, UpdateError> {
        let body = self
            .files
            .get(url)
            .ok_or_else(|| UpdateError::Network(format!("404 {url}")))?;
        std::fs::write(dest, body)?;
        Ok(hex(&Sha256::digest(body)))
    }
}

fn tarball_with_program(script: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let gz = GzEncoder::new(&mut out, Compression::fast());
        let mut b = tar::Builder::new(gz);
        let mut h = tar::Header::new_gnu();
        h.set_size(script.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        b.append_data(
            &mut h,
            "geekbridge-phone-0.2.0-linux-x86_64/bin/geekbridge-phone",
            script,
        )
        .unwrap();
        b.into_inner().unwrap().finish().unwrap();
    }
    out
}

#[test]
fn a_release_found_on_github_is_downloaded_verified_and_installed() {
    let platform = "linux-x86_64";
    let base = "https://github.com/o/r/releases/download/v0.2.0";
    let name = format!("geekbridge-phone-0.2.0-{platform}.tar.gz");
    let program = b"#!/bin/sh\necho geekbridge-phone 0.2.0\n";
    let tar = tarball_with_program(program);
    let digest = hex(&Sha256::digest(&tar));
    let key = SigningKey::from_bytes(&[42u8; 32]);
    let public = hex(key.verifying_key().as_bytes());

    let mut fake = Fake {
        text: HashMap::new(),
        files: HashMap::new(),
    };
    fake.text.insert(
        "https://api.github.com/repos/o/r/releases/latest".into(),
        format!(
            r#"{{"tag_name":"v0.2.0","body":"New things","assets":[
              {{"name":"{name}","browser_download_url":"{base}/{name}"}},
              {{"name":"{name}.sig","browser_download_url":"{base}/{name}.sig"}},
              {{"name":"SHA256SUMS","browser_download_url":"{base}/SHA256SUMS"}}]}}"#
        ),
    );
    fake.files.insert(format!("{base}/{name}"), tar);
    fake.text.insert(
        format!("{base}/{name}.sig"),
        sign_digest(&key, &digest).unwrap(),
    );
    fake.text
        .insert(format!("{base}/SHA256SUMS"), format!("{digest}  {name}\n"));

    let update = release::check(&fake, "o/r", &Version::parse("0.1.0").unwrap(), platform)
        .unwrap()
        .unwrap();
    assert_eq!(update.version.to_string(), "0.2.0");

    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("geekbridge-phone");
    std::fs::write(&exe, b"#!/bin/sh\necho geekbridge-phone 0.1.0\n").unwrap();

    let tarball =
        release::download_and_verify(&fake, &update, &dir.path().join("work"), &public).unwrap();
    let previous = install::install(&tarball, &exe).unwrap();

    assert_eq!(std::fs::read(&exe).unwrap(), program);
    assert!(std::fs::read(previous).unwrap().ends_with(b"0.1.0\n"));
    // The installed program runs and reports the new version.
    let out = std::process::Command::new(&exe)
        .arg("--version")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("0.2.0"));
}

#[test]
fn the_built_in_public_key_is_a_real_ed25519_key() {
    let bytes: [u8; 32] = phone_update::verify::from_hex(phone_update::PUBLIC_KEY_HEX)
        .unwrap()
        .try_into()
        .unwrap();
    assert!(ed25519_dalek::VerifyingKey::from_bytes(&bytes).is_ok());
}

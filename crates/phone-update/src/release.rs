// SPDX-License-Identifier: GPL-3.0-or-later
//! Finding and fetching the latest release.

use crate::http::{url_allowed, Http};
use crate::verify;
use crate::version::Version;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("network error: {0}")]
    Network(String),
    #[error("{0}")]
    Refused(String),
    #[error("the release information could not be read: {0}")]
    Parse(String),
    #[error("this release has no build for this system ({0})")]
    NoAsset(String),
    #[error("the download's signature is not valid: it was not installed")]
    BadSignature,
    #[error("the download does not match its checksum: it was not installed")]
    ChecksumMismatch,
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Install(String),
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    body: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// A newer release this system can install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub version: Version,
    /// The release notes, as written on GitHub.
    pub notes: String,
    pub tarball_name: String,
    pub tarball_url: String,
    pub sig_url: String,
    pub sums_url: String,
}

/// Asks GitHub for the latest release and returns it if it is newer than `current`.
pub fn check(
    http: &dyn Http,
    repo: &str,
    current: &Version,
    platform: &str,
) -> Result<Option<Update>, UpdateError> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let json = http.get_text(&url)?;
    let release: Release =
        serde_json::from_str(&json).map_err(|e| UpdateError::Parse(e.to_string()))?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let Some(version) = Version::parse(&release.tag_name) else {
        return Err(UpdateError::Parse(format!(
            "unrecognised version tag {:?}",
            release.tag_name
        )));
    };
    if version <= *current {
        return Ok(None);
    }
    let tarball_name = format!("geekbridge-phone-{version}-{platform}.tar.gz");
    let find = |name: &str| {
        release
            .assets
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.browser_download_url.clone())
    };
    let (Some(tarball_url), Some(sig_url), Some(sums_url)) = (
        find(&tarball_name),
        find(&format!("{tarball_name}.sig")),
        find("SHA256SUMS"),
    ) else {
        return Err(UpdateError::NoAsset(platform.to_string()));
    };
    for u in [&tarball_url, &sig_url, &sums_url] {
        if !url_allowed(u) {
            return Err(UpdateError::Refused(format!("the release points at {u}")));
        }
    }
    Ok(Some(Update {
        version,
        notes: release.body.unwrap_or_default(),
        tarball_name,
        tarball_url,
        sig_url,
        sums_url,
    }))
}

/// Downloads the update into `dir` and verifies its checksum and signature. Only a file that
/// passes both is returned.
pub fn download_and_verify(
    http: &dyn Http,
    update: &Update,
    dir: &Path,
    public_key_hex: &str,
) -> Result<PathBuf, UpdateError> {
    std::fs::create_dir_all(dir)?;
    let dest = dir.join(&update.tarball_name);
    let digest = http.download(&update.tarball_url, &dest)?;
    let result = (|| {
        let sums = http.get_text(&update.sums_url)?;
        let expected = verify::digest_from_sums(&sums, &update.tarball_name)
            .ok_or(UpdateError::ChecksumMismatch)?;
        if expected != digest {
            return Err(UpdateError::ChecksumMismatch);
        }
        let sig = http.get_text(&update.sig_url)?;
        verify::verify_digest(public_key_hex, &digest, &sig)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use std::collections::HashMap;

    /// A fake GitHub: canned text per URL and file bodies per URL.
    #[derive(Default)]
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
            use sha2::{Digest, Sha256};
            let body = self
                .files
                .get(url)
                .ok_or_else(|| UpdateError::Network(format!("404 {url}")))?;
            std::fs::write(dest, body)?;
            Ok(verify::hex(&Sha256::digest(body)))
        }
    }

    const P: &str = "linux-x86_64";
    const BASE: &str = "https://github.com/o/r/releases/download/v0.2.0";

    fn release_json(tag: &str, prerelease: bool, with_assets: bool) -> String {
        let assets = if with_assets {
            format!(
                r#"[{{"name":"geekbridge-phone-0.2.0-{P}.tar.gz","browser_download_url":"{BASE}/geekbridge-phone-0.2.0-{P}.tar.gz"}},
                    {{"name":"geekbridge-phone-0.2.0-{P}.tar.gz.sig","browser_download_url":"{BASE}/geekbridge-phone-0.2.0-{P}.tar.gz.sig"}},
                    {{"name":"SHA256SUMS","browser_download_url":"{BASE}/SHA256SUMS"}}]"#
            )
        } else {
            "[]".into()
        };
        format!(
            r#"{{"tag_name":"{tag}","draft":false,"prerelease":{prerelease},"body":"Fixes things","assets":{assets}}}"#
        )
    }

    fn fake_with(json: String) -> Fake {
        let mut f = Fake::default();
        f.text.insert(
            "https://api.github.com/repos/o/r/releases/latest".into(),
            json,
        );
        f
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn a_newer_release_is_offered_with_its_assets() {
        let u = check(
            &fake_with(release_json("v0.2.0", false, true)),
            "o/r",
            &v("0.1.0"),
            P,
        )
        .unwrap()
        .unwrap();
        assert_eq!(u.version, v("0.2.0"));
        assert_eq!(u.notes, "Fixes things");
        assert!(u.tarball_url.ends_with("0.2.0-linux-x86_64.tar.gz"));
    }

    #[test]
    fn same_older_prerelease_and_draft_offer_nothing() {
        assert!(check(
            &fake_with(release_json("v0.1.0", false, true)),
            "o/r",
            &v("0.1.0"),
            P
        )
        .unwrap()
        .is_none());
        assert!(check(
            &fake_with(release_json("v0.0.9", false, true)),
            "o/r",
            &v("0.1.0"),
            P
        )
        .unwrap()
        .is_none());
        assert!(check(
            &fake_with(release_json("v0.2.0", true, true)),
            "o/r",
            &v("0.1.0"),
            P
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn a_release_without_a_build_for_this_system_is_reported() {
        let e = check(
            &fake_with(release_json("v0.2.0", false, false)),
            "o/r",
            &v("0.1.0"),
            P,
        )
        .unwrap_err();
        assert!(matches!(e, UpdateError::NoAsset(_)));
        let e = check(
            &fake_with(release_json("v0.2.0", false, true)),
            "o/r",
            &v("0.1.0"),
            "windows-x86_64",
        )
        .unwrap_err();
        assert!(matches!(e, UpdateError::NoAsset(_)));
    }

    #[test]
    fn nonsense_from_the_server_is_an_error_not_a_panic() {
        assert!(check(&fake_with("{ not json".into()), "o/r", &v("0.1.0"), P).is_err());
        assert!(check(
            &fake_with(release_json("nightly", false, true)),
            "o/r",
            &v("0.1.0"),
            P
        )
        .is_err());
    }

    #[test]
    fn assets_pointing_off_github_are_refused() {
        let json = release_json("v0.2.0", false, true)
            .replace("https://github.com/o/r", "https://evil.example/o/r");
        assert!(matches!(
            check(&fake_with(json), "o/r", &v("0.1.0"), P),
            Err(UpdateError::Refused(_))
        ));
    }

    fn signed_fixture(body: &[u8]) -> (Fake, Update, String) {
        use sha2::{Digest, Sha256};
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).unwrap();
        let key = SigningKey::from_bytes(&seed);
        let digest = verify::hex(&Sha256::digest(body));
        let name = format!("geekbridge-phone-0.2.0-{P}.tar.gz");
        let update = Update {
            version: v("0.2.0"),
            notes: String::new(),
            tarball_name: name.clone(),
            tarball_url: format!("{BASE}/{name}"),
            sig_url: format!("{BASE}/{name}.sig"),
            sums_url: format!("{BASE}/SHA256SUMS"),
        };
        let mut f = Fake::default();
        f.files.insert(update.tarball_url.clone(), body.to_vec());
        f.text.insert(
            update.sig_url.clone(),
            verify::sign_digest(&key, &digest).unwrap(),
        );
        f.text
            .insert(update.sums_url.clone(), format!("{digest}  {name}\n"));
        (f, update, verify::hex(key.verifying_key().as_bytes()))
    }

    #[test]
    fn a_correct_download_is_accepted() {
        let (fake, update, public) = signed_fixture(b"the real tarball");
        let dir = tempfile::tempdir().unwrap();
        let path = download_and_verify(&fake, &update, dir.path(), &public).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"the real tarball");
    }

    #[test]
    fn a_tampered_download_is_rejected_and_deleted() {
        let (mut fake, update, public) = signed_fixture(b"the real tarball");
        fake.files.insert(
            update.tarball_url.clone(),
            b"malicious replacement".to_vec(),
        );
        let dir = tempfile::tempdir().unwrap();
        let e = download_and_verify(&fake, &update, dir.path(), &public).unwrap_err();
        assert!(matches!(e, UpdateError::ChecksumMismatch), "{e}");
        assert!(!dir.path().join(&update.tarball_name).exists());
    }

    #[test]
    fn a_valid_checksum_with_a_signature_from_another_key_is_rejected() {
        let (fake, update, _) = signed_fixture(b"payload");
        let stranger = verify::hex(
            SigningKey::from_bytes(&[7u8; 32])
                .verifying_key()
                .as_bytes(),
        );
        let dir = tempfile::tempdir().unwrap();
        let e = download_and_verify(&fake, &update, dir.path(), &stranger).unwrap_err();
        assert!(matches!(e, UpdateError::BadSignature), "{e}");
        assert!(!dir.path().join(&update.tarball_name).exists());
    }

    #[test]
    fn a_missing_signature_file_is_rejected() {
        let (mut fake, update, public) = signed_fixture(b"payload");
        fake.text.remove(&update.sig_url);
        let dir = tempfile::tempdir().unwrap();
        assert!(download_and_verify(&fake, &update, dir.path(), &public).is_err());
    }
}

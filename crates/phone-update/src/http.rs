// SPDX-License-Identifier: GPL-3.0-or-later
//! The two network operations the updater needs, behind a trait so tests need no network.

use crate::release::UpdateError;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

/// The only hosts an update may be fetched from: GitHub's API, its release download host and
/// the CDNs it redirects release assets to.
const ALLOWED_HOSTS: &[&str] = &[
    "api.github.com",
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// The largest download accepted.
pub const MAX_DOWNLOAD_BYTES: u64 = 200 * 1024 * 1024;

pub trait Http {
    /// GETs a small text document (the release JSON or a signature).
    fn get_text(&self, url: &str) -> Result<String, UpdateError>;
    /// Downloads a file to `dest`, returning its SHA-256 as hex.
    fn download(&self, url: &str, dest: &Path) -> Result<String, UpdateError>;
}

/// Whether `url` is an https URL on an allowed host.
pub fn url_allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    // A user-info or port part would let `https://github.com@evil.example/` through.
    !host.contains('@') && !host.contains(':') && ALLOWED_HOSTS.contains(&host)
}

pub struct UreqHttp {
    agent: ureq::Agent,
}

impl UreqHttp {
    pub fn new(app_version: &str) -> Self {
        let agent = ureq::AgentBuilder::new()
            .user_agent(&format!("geekbridge-phone/{app_version}"))
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .redirects(3)
            .build();
        Self { agent }
    }

    fn get(&self, url: &str) -> Result<ureq::Response, UpdateError> {
        if !url_allowed(url) {
            return Err(UpdateError::Refused(format!("refusing to fetch {url}")));
        }
        let resp = self
            .agent
            .get(url)
            .set("Accept", "application/vnd.github+json")
            .call()
            .map_err(|e| UpdateError::Network(e.to_string()))?;
        // After redirects the final host must be allowed too.
        if !url_allowed(resp.get_url()) {
            return Err(UpdateError::Refused(format!(
                "redirected to {}",
                resp.get_url()
            )));
        }
        Ok(resp)
    }
}

impl Http for UreqHttp {
    fn get_text(&self, url: &str) -> Result<String, UpdateError> {
        let mut body = String::new();
        self.get(url)?
            .into_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body)
            .map_err(|e| UpdateError::Network(e.to_string()))?;
        Ok(body)
    }

    fn download(&self, url: &str, dest: &Path) -> Result<String, UpdateError> {
        use sha2::{Digest, Sha256};
        let mut reader = self.get(url)?.into_reader().take(MAX_DOWNLOAD_BYTES + 1);
        let mut file = std::fs::File::create(dest).map_err(UpdateError::Io)?;
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| UpdateError::Network(e.to_string()))?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > MAX_DOWNLOAD_BYTES {
                return Err(UpdateError::Refused(
                    "the download is larger than expected".into(),
                ));
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n]).map_err(UpdateError::Io)?;
        }
        file.sync_all().map_err(UpdateError::Io)?;
        Ok(crate::verify::hex(&hasher.finalize()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_github_hosts_are_allowed() {
        assert!(url_allowed(
            "https://api.github.com/repos/x/y/releases/latest"
        ));
        assert!(url_allowed(
            "https://github.com/x/y/releases/download/v1/a.tar.gz"
        ));
        assert!(url_allowed("https://objects.githubusercontent.com/abc"));
        assert!(!url_allowed("http://api.github.com/x"));
        assert!(!url_allowed("https://evil.example/x"));
        assert!(!url_allowed("https://github.com.evil.example/x"));
        assert!(!url_allowed("https://github.com@evil.example/x"));
        assert!(!url_allowed("https://github.com:444/x"));
        assert!(!url_allowed("file:///etc/passwd"));
        assert!(!url_allowed(""));
    }
}

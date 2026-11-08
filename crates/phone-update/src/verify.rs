// SPDX-License-Identifier: GPL-3.0-or-later
//! Checksums and signatures.

use crate::release::UpdateError;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    // `is_multiple_of` needs a newer Rust than our minimum (1.88).
    #[allow(clippy::manual_is_multiple_of)]
    let odd = s.len() % 2 != 0;
    if odd || !s.is_ascii() {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Signs a SHA-256 digest (hex) and returns the signature as hex.
pub fn sign_digest(key: &SigningKey, digest_hex: &str) -> Result<String, UpdateError> {
    let digest = from_hex(digest_hex)
        .filter(|d| d.len() == 32)
        .ok_or(UpdateError::BadSignature)?;
    Ok(hex(&key.sign(&digest).to_bytes()))
}

/// Checks that `sig_hex` is a valid signature of the digest by the key `public_hex`.
pub fn verify_digest(public_hex: &str, digest_hex: &str, sig_hex: &str) -> Result<(), UpdateError> {
    let public: [u8; 32] = from_hex(public_hex)
        .and_then(|v| v.try_into().ok())
        .ok_or(UpdateError::BadSignature)?;
    let key = VerifyingKey::from_bytes(&public).map_err(|_| UpdateError::BadSignature)?;
    let digest = from_hex(digest_hex)
        .filter(|d| d.len() == 32)
        .ok_or(UpdateError::BadSignature)?;
    let sig: [u8; 64] = from_hex(sig_hex)
        .and_then(|v| v.try_into().ok())
        .ok_or(UpdateError::BadSignature)?;
    key.verify(&digest, &Signature::from_bytes(&sig))
        .map_err(|_| UpdateError::BadSignature)
}

/// Finds `name`'s digest in a `SHA256SUMS` document (`<hex>  <name>` per line).
pub fn digest_from_sums(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (digest, file) = l.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| digest.trim().to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> SigningKey {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).unwrap();
        SigningKey::from_bytes(&seed)
    }

    const DIGEST: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    #[test]
    fn hex_round_trips() {
        assert_eq!(hex(&[0, 255, 16]), "00ff10");
        assert_eq!(from_hex("00ff10"), Some(vec![0, 255, 16]));
        assert_eq!(from_hex("0"), None);
        assert_eq!(from_hex("zz"), None);
    }

    #[test]
    fn a_valid_signature_verifies_and_anything_else_does_not() {
        let k = key();
        let public = hex(k.verifying_key().as_bytes());
        let sig = sign_digest(&k, DIGEST).unwrap();
        assert!(verify_digest(&public, DIGEST, &sig).is_ok());
        // A different digest, a different key, and a corrupted signature all fail.
        let other = "ff".repeat(32);
        assert!(verify_digest(&public, &other, &sig).is_err());
        let stranger = hex(key().verifying_key().as_bytes());
        assert!(verify_digest(&stranger, DIGEST, &sig).is_err());
        let mut bad = sig.clone();
        bad.replace_range(0..2, if &sig[0..2] == "00" { "01" } else { "00" });
        assert!(verify_digest(&public, DIGEST, &bad).is_err());
        assert!(verify_digest(&public, DIGEST, "not hex").is_err());
        assert!(verify_digest("short", DIGEST, &sig).is_err());
    }

    #[test]
    fn reads_a_digest_from_sha256sums() {
        let sums = "aaaa  other.tar.gz\nBBBB *geekbridge-phone-1.0.0-linux-x86_64.tar.gz\n";
        assert_eq!(
            digest_from_sums(sums, "geekbridge-phone-1.0.0-linux-x86_64.tar.gz").as_deref(),
            Some("bbbb")
        );
        assert_eq!(digest_from_sums(sums, "missing"), None);
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! Signs a release file in CI: `gbp-sign <file>` reads the private key (hex) from the
//! `UPDATE_SIGNING_KEY` environment variable and writes `<file>.sig` (a signature of the file's
//! SHA-256 digest, hex). Also prints the digest line for `SHA256SUMS`.

use ed25519_dalek::SigningKey;
use phone_update::verify::{from_hex, hex, sign_digest};
use sha2::{Digest, Sha256};
use std::io::Read;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: gbp-sign <file>");
        std::process::exit(2);
    });
    let secret = std::env::var("UPDATE_SIGNING_KEY").unwrap_or_else(|_| {
        eprintln!("UPDATE_SIGNING_KEY is not set");
        std::process::exit(2);
    });
    let seed: [u8; 32] = from_hex(&secret)
        .and_then(|v| v.try_into().ok())
        .unwrap_or_else(|| {
            eprintln!("UPDATE_SIGNING_KEY is not a 32-byte hex key");
            std::process::exit(2);
        });
    let key = SigningKey::from_bytes(&seed);

    let mut file = std::fs::File::open(&path).expect("open the file to sign");
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).expect("read");
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hex(&hasher.finalize());
    let sig = sign_digest(&key, &digest).expect("sign");
    std::fs::write(format!("{path}.sig"), &sig).expect("write the signature");
    let name = std::path::Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&path);
    println!("{digest}  {name}");
}

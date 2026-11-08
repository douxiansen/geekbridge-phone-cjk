// SPDX-License-Identifier: GPL-3.0-or-later
//! Generates an update-signing key pair. Prints the private key (hex) on stdout and the public
//! key (hex) on stderr. Keep the private key secret: it goes in the repository's
//! `UPDATE_SIGNING_KEY` Actions secret; the public key goes in `src/public_key.hex`.

use ed25519_dalek::SigningKey;
use phone_update::verify::hex;

fn main() {
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).expect("operating system randomness");
    let key = SigningKey::from_bytes(&seed);
    println!("{}", hex(&key.to_bytes()));
    eprintln!("{}", hex(key.verifying_key().as_bytes()));
}

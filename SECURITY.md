# Security

GeekBridge Phone talks only to your paired phone over Bluetooth and stores a cache of your contacts, call history and texts as plain JSON files in `~/.local/share/geekbridge-phone`. Protect that folder like the phone itself (full-disk encryption is the right tool).

The app has no telemetry. Its only network access is the optional update check against GitHub releases (HTTPS, host allow-list, no data about you sent). An update is installed only if its SHA-256 checksum matches `SHA256SUMS` and its Ed25519 signature verifies against the public key compiled into the app (`crates/phone-update/src/public_key.hex`). The signing key is a GitHub Actions secret; see docs/RELEASING.md for rotation and rollback.

## Reporting a vulnerability

Please report privately through GitHub's "Report a vulnerability" (Security tab) rather than a public issue. Include the version, your OS and, if possible, steps to reproduce. Do not include real message or contact content.

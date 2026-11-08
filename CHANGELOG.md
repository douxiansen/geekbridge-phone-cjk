# Changelog

## 0.1.0

First release, Linux.

- Calls: incoming banner with Answer / Decline, dialing, hang-up, call timer, history.
- Texts: read and send SMS over Bluetooth MAP, iMessage-style conversations, quick reply from the banner, history kept on disk.
- Contacts and call history over PBAP, search by name, number or keypad letters.
- Top-right overlay banners on Wayland (layer-shell); freedesktop notifications as a fallback.
- Call recording (off by default): both sides saved as one stereo WAV.
- SQLite cache: instant startup, incremental writes, contacts refetched only when stale.
- Self-update from GitHub releases, verified by checksum and Ed25519 signature.
- Portable Rust core: OBEX/MAP/PBAP client and the service have no OS dependencies.

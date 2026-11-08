# Changelog

## 0.1.3

- iPhone: a phone that has not been allowed to share contacts used to leave the app believing it had fresh (empty) contacts for 12 hours. An empty phonebook is now reported ("turn on Sync Contacts on the iPhone"), retried, and never replaces cached contacts.
- Examples to inspect a phone's OBEX and phonebook replies (`obex_raw`, `pbap_probe`).

## 0.1.2

iPhone support groundwork: a phone that refuses a connection (an iPhone before message access is allowed) is now reported as "allow access on your phone" instead of a garbled-data error.

## 0.1.1

Calls only: the phone's music and videos no longer play through your computer's speakers. Settings > Calls > Calls only (on by default) keeps the Bluetooth audio link for calls.

- Fix: a rare "text file busy" failure while installing an update.
- Fix: call state is published before the call events that announce it.

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

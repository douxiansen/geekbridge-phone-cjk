# Changelog

## 0.1.0 (unreleased)

First release, Linux.

- Calls: incoming banner with Answer / Decline, dialing, hang-up, call timer, history.
- Texts: read and send SMS over Bluetooth MAP, iMessage-style conversations, quick reply from the banner, history kept on disk.
- Contacts and call history over PBAP, search by name, number or keypad letters.
- Top-right overlay banners on Wayland (layer-shell); freedesktop notifications as a fallback.
- Portable Rust core: OBEX/MAP/PBAP client and the service have no OS dependencies.

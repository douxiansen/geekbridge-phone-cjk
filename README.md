<img src="packaging/linux/dev.geekbridge.Phone.svg" width="64" height="64" alt="">

# GeekBridge Phone

**Answer calls and read and send texts on your computer, through your Android phone, over plain Bluetooth. No app on the phone, no cloud, no account.**

An incoming call or text appears in the top-right corner of your screen like a macOS notification: answer or decline the call, or reply to the text right there. The look is macOS meets dot-matrix: true black or warm off-white, one red accent, dot-matrix numerals.

It speaks only standard Bluetooth profiles that every Android phone already has (Hands-Free for calls, Message Access for texts, Phone Book Access for contacts). Nothing is installed on the phone and nothing leaves your Bluetooth link.

<p align="center">
  <img src="docs/img/app-messages-dark.png" width="420" alt="Messages, dark: conversations and an iMessage-style thread">
  <img src="docs/img/app-keypad-light.png" width="420" alt="Keypad, light: dot-matrix number readout">
</p>
<p align="center">
  <img src="docs/img/overlay-stack-dark.png" width="360" alt="Top-right banners: a text with quick reply and a call in progress">
  <img src="docs/img/overlay-call-light.png" width="360" alt="Incoming call banner with Decline and Answer, light">
</p>

<sub>Screenshots use a demo phone with invented people (`geekbridge-phone --demo`).</sub>

> **Status: 0.1.0, Linux.** Verified on a Samsung Galaxy Z Fold5 (Android 17). Windows and macOS are planned: the protocol and core crates already have no OS dependencies, and [docs/PORTING.md](docs/PORTING.md) lists exactly what a new platform has to provide.

## What it does

| | |
| --- | --- |
| **Calls** | Incoming-call banner with Answer / Decline, dial from the keypad or contacts, hang up, call timer, missed-call and history list. Voice goes through the computer's Bluetooth hands-free link. |
| **Texts** | Read and send SMS, conversations like iMessage, notification banner with **quick reply**. Your history is kept on this computer. |
| **Contacts** | Your phone's contacts and call history, searchable by name, number or keypad letters. |
| **Recording** | Optional call recording (off by default): your voice and the other person's are saved as one stereo WAV, the other person on the left. Settings > Calls. Recording a call may require the other person's consent where you live. |
| **Overlay** | Top-right banners for calls and texts (Wayland layer-shell); a freedesktop notification where that is not available. |

## Requirements

- Linux with **BlueZ** and **PipeWire 1.4+** with its telephony module (Fedora 42+, Arch, Ubuntu 25.10+, Debian 13, openSUSE Tumbleweed). A Bluetooth controller in the computer.
- An **Android** phone **paired** with the computer. The first time, the phone asks to allow access to messages and contacts: tick "always allow".
- Overlay banners need a Wayland compositor with `wlr-layer-shell` (Hyprland, Sway, KDE, river, niri, ...). GNOME falls back to ordinary notifications.

## Install

From a release: unpack the tarball and run `./scripts/install.sh` (per user, no root; starts a background service). From source:

```sh
cargo build --release -p phone-app
./scripts/install.sh
```

Uninstall with `./scripts/install.sh --uninstall` (your data is kept in `~/.local/share/geekbridge-phone`).

## iPhone

An iPhone works the same way, as a "car kit", verified on a real iPhone. After pairing, open **Settings > Bluetooth**, tap the **ⓘ** next to your computer and switch on **Show Notifications** (texts) and **Sync Contacts**. Then:

| | iPhone |
| --- | --- |
| Calls: banner, Answer, Decline, Hang up, talk through the computer | Works |
| Contacts and call history | Works (needs Sync Contacts) |
| Reading texts | Works (needs Show Notifications); iMessages look like SMS |
| Sending texts | Not verified: Apple lets cars send texts only by voice, so expect it to be refused |

## Using it

1. Pair the phone in your system's Bluetooth settings.
2. Start **GeekBridge Phone**. The phone chip in the sidebar shows the phone's name and battery.
3. Allow message and contact access on the phone when it asks.

Troubleshooting:

- **"Allow message access on your phone"**: open the phone's Bluetooth settings, the computer's entry, and enable *Messages* and *Contacts*.
- **No calls**: calls need the phone connected for *hands-free* (phone calls). Check the Bluetooth entry on the phone, and that PipeWire's telephony module is loaded: `busctl --user tree org.pipewire.Telephony`.
- **Texts stop arriving after a while**: the phone allows one message session at a time. Close other apps that read messages over Bluetooth (for example another phone-link tool).
- **Call recording is silent on the phone's side**: the app records the phone's voice stream from PipeWire (`pw-record`, from `pipewire-utils`). Run `cargo run -p phone-linux --example rec_probe -- <phone address>` during a call to see whether the stream is found.
- **Contacts are empty**: some phones refuse contact access until you allow it in the Bluetooth entry; use *Reload* in Contacts afterwards.

## Privacy

Everything stays on the Bluetooth link between the phone and this computer. Contacts, call history and your text history are cached in a small SQLite database (`cache.db`, owner-only) under `~/.local/share/geekbridge-phone`, so the app starts instantly and does not re-read everything from the phone. Nothing is sent anywhere else and there is no telemetry. The only network access is the optional update check (below).

## Updates

The app updates itself from this repository's GitHub releases (*Settings > Updates*, on by default, about once a day; or `geekbridge-phone --check-update` / `--update`). It talks only to `api.github.com` and GitHub's release download hosts, over HTTPS, sends nothing about you, and installs a release only after checking both its SHA-256 checksum **and** an Ed25519 signature against the public key built into the app. The previous program is kept as `geekbridge-phone.previous` for a manual rollback. Turn the check off in Settings, or set `"check_updates": false` in `~/.config/geekbridge-phone/settings.json`. Installs managed by a package manager are left to it. How releases are made: [docs/RELEASING.md](docs/RELEASING.md).

## How it is built

A Rust workspace:

- `phone-obex`: a portable OBEX client with MAP (texts) and PBAP (phonebook). No OS code.
- `phone-core`: call tracking, contacts, conversations, settings, storage and the background service. No OS code.
- `phone-linux`: the Linux platform: BlueZ + PipeWire telephony over D-Bus, RFCOMM sockets, SDP.
- `phone-overlay`: the top-right banners (platform-neutral renderer plus a Wayland layer-shell presenter).
- `phone-app`: the egui application.
- `phone-update`: the self-updater (GitHub releases, checksum + signature verification, atomic install).

See [ARCHITECTURE.md](ARCHITECTURE.md) for the design and [docs/PORTING.md](docs/PORTING.md) for Windows and macOS.

## Development

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo run -p phone-app --features demo -- --demo   # a fake phone with invented data
```

## Licence and credits

GPL-3.0-or-later, see [LICENSE](LICENSE). The visual design system (palette, dot-matrix engine) comes from the GeekBridge desktop apps by the same author.

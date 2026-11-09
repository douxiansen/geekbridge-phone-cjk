# GeekBridge Phone on Windows

Calls, texts and contacts from an Android phone or an iPhone, with the call voice through the
computer's speakers and microphone, on Windows 10 and 11. No dongle, no phone app, no cloud.

## What works

| | |
| --- | --- |
| Pairing | From inside the app: scan, pick the phone, confirm the number. No trip to Windows Settings. |
| Texts and contacts | Over the phone's message and phonebook services (RFCOMM through WinRT). |
| Calls | Banner at the top right (the same one as on Linux), Answer, Decline, Hang up, dial, keypad tones. |
| Call voice | Moves to the computer by itself once a call connects. |
| Reconnect | Windows drops an idle Bluetooth link; the app brings it back. |

## How calls work

Windows keeps a *phone line* per Bluetooth phone and carries the call voice on it. Windows creates
that line when an installed (packaged) app registers as a call handler for the phone's hands-free
link. Phone Link does this; so does this app. The steps, in `crates/phone-windows/src/winline.rs`:

1. Find the phone's call-link transport (`PhoneLineTransportDevice`).
2. `RegisterApp()` if the app is not registered, then `ConnectAsync()`.
3. Windows now lists the phone line. Calls, answer, dial and keypad go through `PhoneLine` and
   `PhoneCall`; when a call is talking and its audio is on the phone, `ChangeAudioDevice(LocalDevice)`
   moves it to the computer.

Two things make step 2 work, both in `packaging/windows/AppxManifest.xml`:

- the `phoneCall` capability, and
- `<uap13:Extension Category="windows.phoneCallActivation"/>`, which marks the app as a call handler.

Without a package identity Windows refuses (`DeniedBySystem`). An unpackaged program therefore falls
back to `hfp.rs`, a small hands-free client over RFCOMM. It controls calls but cannot carry the
voice, because the voice channel belongs to Windows' own stack. While packaged, the app never opens
that fallback: a phone accepts one hands-free connection, and it must be Windows'.

## One-time setup for a user

1. Pair the phone from the app (Settings > Pair a phone, which also opens by itself when no phone is
   paired). The app then sets up the call link; the window says when it is ready.
2. Only then, on the phone: Settings > Bluetooth > the gear next to this computer > switch on **Calls**
   (call audio). The phone keeps it on only while this computer's call link is up; if it switches
   itself off, wait for the link (see the log below) and try again.
3. Ring the phone. The app shows a banner when this is still missing.

The app writes a log to `%LOCALAPPDATA%\GeekBridge\geekbridge-phone.log` (for the installed package:
`%LOCALAPPDATA%\Packages\GeekBridge.Phone_*\LocalCache\Local\GeekBridge\`). A line starting
`call link for ...` shows whether Windows accepted the registration.

## iPhone

The app tells an iPhone from other phones by Apple's Bluetooth vendor id, which Windows writes into
the id of the phone's call-link device (`VID&0001004C`), the same fact the Linux version reads from
BlueZ. For an iPhone the setup guide opens by itself, as on Linux:

1. Pair the iPhone from the app.
2. On the iPhone: Settings > Bluetooth, tap the **i** next to this computer, and switch on
   **Show Notifications** (texts) and **Sync Contacts**.
3. Ring the iPhone and answer from the banner. If the voice stays on the iPhone, tap **Audio** on its
   call screen and pick this computer once.

Apple lets a car send texts only by voice, so an iPhone may refuse to send one; a text sent from the
app through an iPhone to an Android phone did arrive. Tried on Windows with a real iPhone: pairing, the
setup guide, calls and sending a text work.

## Building and running it as a developer

Needs Rust (MSVC), Python 3, and **Developer Mode** (Settings > System > For developers).

```powershell
cd packaging\windows
.\build-dev-package.ps1            # builds and registers GeekBridge.Phone for you
explorer.exe shell:AppsFolder\GeekBridge.Phone_et8bt1m87t33c!App
```

A plain `cargo run -p phone-app` still runs (messages, contacts, call control through the fallback),
but without voice.

## Installing it (everyone else)

Download `GeekBridge-Phone-<version>-windows-x64.zip` from the Releases page, unzip it, right-click
`Install.ps1` and choose **Run with PowerShell**. The package is signed with the project's own
certificate (`GeekBridge.cer` in the zip), which Windows does not trust by default, so the script
asks for administrator rights once, trusts that certificate for packages, and installs the app.
`Uninstall.ps1` removes the app and, if you say yes, the trust. Windows 10 2004 or later, or
Windows 11; no other downloads are needed.

Maintainers build the release with `packaging/windows/build-release.ps1`. It needs the signing
certificate in the certificate store (the script's help shows how to create it); back that
certificate up, because every update must be signed by the same one. When the project outgrows a
self-signed certificate, signing for open-source projects (SignPath Foundation) or the Microsoft
Store would remove the trust prompt.

## What was tried and does not work

Written down so nobody repeats it:

- **Raw hands-free link from the app** (`hfp.rs`): the phone refuses to move the voice to it
  (`AT+BCC` answers `ERROR`), and Windows offers no way to attach a voice channel to it.
- **Publishing a hands-free service record through WinRT**: Windows generates the service class
  list itself and rejects an override.
- **Unpackaged app with the phone-line API**: `DeniedBySystem` for registration and `ConnectAsync`
  returns false.
- **`PhoneEnableBluetoothHandsFree`** (undocumented, in `PhoneOm.dll`): refuses with
  `0x800710dd` for every caller; not needed once the app is registered.
- **Extra restricted capabilities** (`phoneLineTransportManagement`, `phoneCallSystem`): no effect.

## Limits

- An Android phone needs **Calls** switched on for the computer (step 2 above); an iPhone needs
  Show Notifications and Sync Contacts (see iPhone).
- Windows 11 22H2 and later changed parts of the phone-line API. The app was developed against
  Windows 11 Enterprise LTSC 2024 with a Samsung Galaxy Z Fold5 and an iPhone; other phones and
  Windows builds are untested.
- Whether the setup survives a restart is untested.

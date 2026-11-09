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

1. Pair the phone from the app.
2. On the phone: Settings > Bluetooth > the gear next to this computer > switch on **Calls**
   (call audio). If it switches itself off again, unpair and pair once more from the app.
3. Ring the phone. The app shows a banner when this is still missing.

## Building and running it as a developer

Needs Rust (MSVC), Python 3, and **Developer Mode** (Settings > System > For developers).

```powershell
cd packaging\windows
.\build-dev-package.ps1            # builds and registers GeekBridge.Phone for you
explorer.exe shell:AppsFolder\GeekBridge.Phone_et8bt1m87t33c!App
```

A plain `cargo run -p phone-app` still runs (messages, contacts, call control through the fallback),
but without voice.

## Giving it to other people

Developer Mode installs an unsigned package for the developer only. For everyone else the package
must be signed. Free routes: a self-signed certificate with an installer script that trusts it
(needs one administrator prompt), or code signing for open-source projects such as SignPath
Foundation. The Microsoft Store signs for you but adds review and a fee.

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

- Needs the phone's **Calls** permission for the computer (step 2 above).
- Windows 11 22H2 and later changed parts of the phone-line API. The app was developed against
  Windows 11 Enterprise LTSC 2024 and a Samsung Galaxy Z Fold5; other phones and Windows builds
  are untested.
- Whether the setup survives a restart, and an iPhone, are untested.

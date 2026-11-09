# Porting to Windows and macOS

`phone-obex` and `phone-core` have no OS dependencies and are tested on Windows and macOS in CI. A port is a new crate that implements `phone_core::platform::Platform`, plus an overlay presenter.

## The Platform trait

```rust
fn phones(&self) -> Result<Vec<PhoneInfo>>;                 // paired phones: name, address, connected, battery
fn open_map(&self, address: &str) -> Result<Box<dyn Stream>>;   // RFCOMM stream to the phone's message service
fn open_pbap(&self, address: &str) -> Result<Box<dyn Stream>>;  // ... and its phonebook service
fn calls(&self, address: &str) -> Result<Vec<CallSnapshot>>;    // the phone's calls right now
fn dial / answer / hang_up / hang_up_all / send_tones
```

`Stream` is any `Read + Write + Send`. The OBEX layer above does the rest, so a port never implements MAP or PBAP itself.

## Windows (`phone-windows`)

> **Done.** See [WINDOWS.md](WINDOWS.md) for how it works and what did not. The plan below is the original one; calls ended up using Windows' own phone line instead of `Windows.ApplicationModel.Calls` through a hand-made hands-free client.

| Method | API |
| --- | --- |
| `phones` | `Windows.Devices.Enumeration` + `BluetoothDevice` (paired, connection status, name). Battery via the Bluetooth GATT/HFP battery property where available. |
| `open_map` / `open_pbap` | `RfcommDeviceService.FromIdAsync` for service ids `0x1132` (MAP MAS) and `0x112F` (PBAP) and a `StreamSocket`; wrap its streams in `Read + Write`. No manual SDP is needed: WinRT resolves the channel. Add timeouts. |
| `calls` / call control | `Windows.ApplicationModel.Calls`: `PhoneLineTransportDevice` (register the app for the phone's hands-free link), `PhoneLine` for calls and dialing, and `PhoneCallStore`/line events for snapshots. The open-source *My Phone* project shows this flow. |
| Call audio | Windows routes hands-free call audio itself once the phone is connected for telephony. Verify on hardware. |

Crate: `windows` (the Microsoft crate) with the `Devices_Bluetooth`, `Devices_Bluetooth_Rfcomm`, `Networking_Sockets`, `ApplicationModel_Calls` features.

## macOS (`phone-macos`)

| Method | API |
| --- | --- |
| `phones` | `IOBluetoothDevice.pairedDevices()`, `isConnected`, `name`. |
| `open_map` / `open_pbap` | `IOBluetoothDevice.performSDPQuery` for the service class, read the RFCOMM channel id from the record, then `openRFCOMMChannelSync`. Wrap the channel in `Read + Write` with timeouts. |
| calls | `IOBluetoothHandsFreeDevice` (macOS acts as the hands-free unit): delegate callbacks for call state, `dial`, `answer`/`hangUp`, `sendDTMF`. The open-source *PhoneBT* project does this with Android and iPhone. |
| Call audio | Open question: verify that call audio follows the HFP connection on the Mac. |

Crate: `objc2` + `objc2-io-bluetooth`. The app needs the Bluetooth usage description in its `Info.plist` and must be code-signed and notarised for distribution.

## The overlay presenter

`phone-overlay` renders banners to an RGBA pixmap on any OS. A presenter shows that pixmap in a topmost, non-activating, click-through-except-banner surface at the top-right:

- Windows: a layered `WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` window.
- macOS: an `NSPanel` with `NSWindowStyleMaskNonactivatingPanel`, level `NSStatusWindowLevel`.

## Open questions to settle on real hardware first

1. Call audio on Windows and macOS.
2. Windows RFCOMM service resolution with Android's "SMS/MMS" and phonebook services.
3. macOS MAP against an Android phone (Ferry proves the flow for iPhones).

Each is a small spike (connect, list messages, place a call) before building the full port.

## Starting the Windows port: a checklist

Setup on a Windows machine:

1. Install Rust with the MSVC toolchain (`rustup`), the Visual Studio Build Tools (C++ workload), and Git.
2. `git clone https://github.com/fspecii/geekbridge-phone` and run `cargo test -p phone-obex -p phone-core -p phone-update`. These pass on Windows in CI, so a failure here is a setup problem.
3. Pair your phone in Windows Settings > Bluetooth & devices first. The app never pairs.

Then build `crates/phone-windows` in small spikes, each proven on a real phone before the next. Every spike can be an `examples/` program, like the Linux ones (`live_probe`, `pbap_probe`, `obex_raw`):

1. **Phones**: list paired phones with name, address and connection state (`BluetoothDevice`).
2. **Messages**: open the message service with `RfcommDeviceService`, wrap the `StreamSocket` in `Read + Write`, and run `phone_obex::MapClient` over it (list folders, list messages). This is the biggest proof: it shows the OBEX layer needs nothing else.
3. **Contacts**: the same for the phonebook (`PbapClient`, see `pbap_probe`).
4. **Calls**: `PhoneLineTransportDevice` and `PhoneLine`: events for a ringing call, `Dial`, answer, hang up. Map them to `CallSnapshot`.
5. **Call audio**: check that voice reaches the PC speakers and microphone once the call is active.
6. Implement `phone_core::platform::Platform` from those pieces and select it in `phone-app` with `#[cfg(windows)]` (see how the Linux platform is chosen in `crates/phone-app/src/main.rs`).

After that: the corner banners (a `phone-overlay` presenter for Windows, see above), release packaging as a `.zip` in `.github/workflows/release.yml` (add a `windows-latest` job; the in-app updater already matches assets by `<os>-<arch>` and needs a Windows installer step in `phone-update/src/install.rs`, where replacing a running `.exe` means renaming it aside first), and the CI job that builds `phone-windows`.

The pitfalls found on Linux apply on every OS: a phone allows only one message session at a time, an empty phonebook is not "fresh", a refused OBEX connect still carries its fixed fields, and every socket needs a connect and read timeout.

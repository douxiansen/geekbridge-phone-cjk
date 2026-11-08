# Architecture

GeekBridge Phone talks to an Android phone over standard Bluetooth profiles. It never needs software on the phone.

```
                 +---------------------------+
                 | phone-app (egui) + overlay |   UI, banners, hotkeys
                 +-------------+-------------+
                               | Handle / Event channel
                 +-------------v-------------+
                 |        phone-core         |   service threads, state,
                 |  calls  contacts  texts   |   persistence (JSON)
                 +------+-------------+------+
        Platform trait  |             |  Read + Write streams
                 +------v-----+  +----v---------+
                 | phone-linux|  |  phone-obex  |   OBEX 1.0, MAP, PBAP
                 | (BlueZ,    |  | (no OS code) |
                 |  PipeWire) |  +--------------+
                 +------------+
```

## Why the phone needs nothing installed

| Need | Profile | Android side |
| --- | --- | --- |
| Answer, dial, hang up | HFP (Hands-Free) | the phone is the Audio Gateway |
| Read and send texts | MAP (Message Access) | the "SMS/MMS" service |
| Contacts, call history | PBAP (Phone Book Access) | the phonebook service |

MAP and PBAP are OBEX services on RFCOMM channels. The channel number differs per phone, so it is looked up with SDP over L2CAP (`phone-linux/src/sdp.rs`).

## phone-obex

A pure OBEX client over any `Read + Write` stream: packet codec, connect / set-path / get / put with multi-packet transfers, the bMessage format, MAP folder and message listings, and PBAP pulls. A fake phone server (`fake` feature) lets every layer above be tested without hardware.

## phone-core

`service::Service` owns five threads:

1. **phones**: lists paired phones and picks the one in use.
2. **calls**: polls the platform for the phone's calls every 500 ms and feeds `CallTracker`, which turns snapshots into events (it tolerates the gateway briefly reporting no calls, with a 1.5 s grace window).
3. **messages**: owns the phone's *single* MAP session. It polls the inbox and the sent folder, detects new texts by content (not the phone's handle, so reconnecting never re-announces old texts), sends queued texts, and reconnects with exponential backoff. A refusal (the phone's "allow access" prompt not yet answered) is reported distinctly.
4. **contacts**: pulls the phonebook and call history over PBAP when a phone connects, or on request.
5. **commands**: dispatches UI commands (dial, answer, hang up, send text, settings).

The UI reads `State` under a short lock and receives `Event`s on a channel. History is cached per phone so it grows beyond what the phone lists at once.

## Platform trait

Everything OS-specific is `phone_core::platform::Platform`: list phones, open the MAP and PBAP streams, and control calls. `phone-linux` implements it with BlueZ and PipeWire's telephony service over D-Bus plus raw RFCOMM sockets. See [docs/PORTING.md](docs/PORTING.md).

## Reliability choices

- Every Bluetooth socket has a connect timeout and read/write timeouts, so a phone that goes quiet can never hang the service or its shutdown.
- A phone allows only one MAP session; the message thread is its single owner and retries with backoff.
- Files are written atomically (temp + rename); a corrupt file is set aside as `.bad` instead of being overwritten.
- No panics on I/O paths; failures become `Notice` events the UI shows in a sentence.

# phone-app

The GeekBridge Phone desktop app (`geekbridge-phone`): a Bluetooth dialler and SMS client for
Android phones with nothing installed on the phone. egui, in the macOS layout with the
dot-matrix "Nothing OS" look from the GeekBridge design system.

```sh
cargo run -p phone-app                  # the real phone
cargo run -p phone-app -- --demo        # a fake phone with invented data; Help > Simulate ...
cargo run -p phone-app -- --demo --tour --theme dark     # visit every screen, for screenshots
cargo run -p phone-app -- --demo --scene ring            # also: active, dialing, text, denied, offline
cargo run -p phone-app -- tel:+15550100  # keypad with the number typed in (running app if any)
```

`--zoom 1.4` shows the 720 px layout in a wide window; `--size`, `--theme`, `--tab` and
`--minimised` (`--background`) are there for the same reason. See `--help`.

## Layout

| path | what |
| --- | --- |
| `src/model/` | view-model logic with tests: time formatting, thread grouping, keypad rules, compose limits, avatars, status text, list arithmetic |
| `src/ui/` | the visual kit: palette and fonts, dot-matrix text, icons, controls, the one-hovered-row list. Vendored from the GeekBridge design system; no phone knowledge |
| `src/views/` | the screens: keypad, contacts, recents, messages, settings, call card |
| `src/app.rs`, `chrome.rs`, `banners.rs` | the shell: sidebar, toolbar, banners, shortcuts, window life |
| `src/pump.rs` | the event loop (own thread, so alerts work while the window is minimised) |
| `src/notify.rs`, `notify_freedesktop.rs` | the `Notifier` seam and the built-in freedesktop notifier |
| `src/action.rs`, `instance.rs` | `UiAction` and the single-instance socket |
| `src/window.rs`, `raise_wayland.rs`, `platform_ui.rs` | the OS-specific bits: closing/minimising, bringing a minimised window back on Wayland, opening folders |
| `src/demo.rs` | `--demo` support (cargo feature `demo`, on by default) |

Everything is OS-neutral except `notify_freedesktop.rs`, `raise_wayland.rs` and the Linux arms in
`window.rs`, `platform_ui.rs` and `boot.rs`. Porting means: a `Platform` (see `docs/PORTING.md`), the arms marked `TODO(windows, macos)` in
`boot.rs`, `instance.rs`, `notify.rs`, `window.rs` and `platform_ui.rs`.

## Behaviour worth knowing

* The UI never blocks and never holds the service lock across a frame: each frame copies a small
  `Live` snapshot, and the larger lists are copied only when the event thread says they changed.
* Closing the window **minimises** it when "keep running" is on (there is no tray on every
  desktop); Ctrl+Q, Settings > Quit, or keep-running off quits. A second launch hands its request
  (`--open NUMBER`, `tel:`, or plain) to the first and exits; the first un-minimises and focuses.
  On Wayland that goes through `wlr-foreign-toplevel-management` (winit cannot), from the thread
  that received the request, because a minimised window is not redrawn.
* Lists show hover on exactly one row: it is worked out each frame from the pointer position
  (`ui::vlist`), never remembered, and withheld while scrolling.
* Unread state is also kept here (`ui.json` in the config folder): reading a thread in the window
  marks it read on this computer only.

## Wiring `phone-overlay` (or any other alert surface)

The seam is a `Notifier` built with a `NotifierHost`:

```rust
use phone_app::{run, RunOptions};
use phone_app::notify::{Notifier, NotifierHost, NotifierFactory, NotifyId, CallNotice, CallUpdate, CallEndedNotice, TextNotice};

let factory: NotifierFactory = Box::new(|host: NotifierHost| {
    let (overlay, actions) = phone_overlay::spawn(Default::default())?;   // fall back to None on Unsupported
    let ids = Arc::new(Mutex::new(Ids::default()));       // u64 banner id <-> NotifyId / call id / number
    // Route what the user does on a banner. `host` is Clone + Send.
    std::thread::spawn({ let host = host.clone(); let overlay = overlay.clone(); let ids = ids.clone(); move || {
        for action in actions {
            match action {
                Action::Answer(id)  => host.answer(&ids.call_id(id)),
                Action::Decline(id) => host.decline(&ids.call_id(id)),
                Action::HangUp(id)  => host.hang_up(&ids.call_id(id)),
                Action::Reply { id, number, text } => {
                    let token = host.reply(&number, &text);   // result arrives in Notifier::reply_result
                    ids.remember_reply(token, id);
                    overlay.reply_status(id, ReplyStatus::Sending);
                }
                Action::Open(id)    => host.open_conversation(&ids.number(id)),   // brings the window forward
                Action::Dismiss(_)  => {}
            }
        }
    }});
    Box::new(OverlayNotifier { overlay, ids })
});
run(RunOptions { notifier: Some(factory), ..RunOptions::default() })?;
```

`OverlayNotifier` implements `Notifier`: `incoming_call` shows a `Banner::Call(.. Incoming)`,
`call_updated` re-shows the same banner id as `Active { since }` / `Dialing` / `Held`,
`clear` removes the banner, `text` shows a `Banner::Text` (`can_reply: t.quick_reply`, close it
after `t.timeout_secs`), and `reply_result(token, ..)` calls
`overlay.reply_status(banner, Sent | Failed(msg))`. `NotifierHost::show_window()` and
`NotifierHost::open_conversation(number)` send a `UiAction`, which also un-minimises the window.

The event thread calls the notifier, never the UI thread; it decides from the user's settings
(`notify_calls`, `notify_missed`, `notify_texts`, `quick_reply`, `text_banner_secs`) whether to
call at all, and skips text alerts while the user is looking at the Messages tab.

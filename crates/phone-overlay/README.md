# phone-overlay

macOS-style notification banners for GeekBridge Phone, drawn in the dot-matrix
"Nothing OS" look: **incoming and active calls** (Answer / Decline / Hang up) and
**incoming texts with an inline quick reply**. Banners sit in the top-right
corner of the screen, stack newest-first (max 3 visible), slide in and out,
and cost no CPU while nothing is shown.

```rust
use phone_overlay::{spawn, Action, Banner, CallBanner, CallState, OverlayConfig, OverlayError};

match spawn(OverlayConfig::default()) {
    Ok((overlay, actions)) => {
        overlay.show(Banner::Call(CallBanner {
            id: 1, name: "Ada Lovelace".into(), number: "+1 555 0100".into(),
            state: CallState::Incoming,
        }));
        for action in actions {            // Answer / Decline / HangUp / Reply / Open / Dismiss
            println!("{action:?}");
        }
    }
    // No layer-shell compositor, no Wayland session, or not Linux:
    Err(OverlayError::Unsupported) => { /* fall back to a freedesktop notification */ }
    Err(e) => eprintln!("overlay: {e}"),
}
```

## Behaviour

* **Call banners** stay until `remove(id)`; `show` with the same id replaces in place
  (Incoming -> Active keeps the banner and swaps the buttons).
* **Text banners** auto-dismiss after `OverlayConfig::text_timeout` (default 8 s) and
  emit `Action::Dismiss`; the countdown pauses while the pointer is over the banner or
  a reply is being typed. Hovering reveals the reply field (180 ms expand) and puts
  the text cursor in it. Enter or the blue arrow emits `Action::Reply`; the banner
  then shows "Sending…", and the app answers with `reply_status(id, Sent | Failed(msg))`.
  `Sent` closes the banner 1.5 s later; `Failed` shows the message in red and keeps the text.
  Esc clears and collapses the field. Clicking the body emits `Action::Open` and closes the banner.
  The cross (visible on hover) dismisses.
* The overlay never maps a window unless there is a banner, and never takes the
  keyboard unless a reply field is focused (layer-shell `on_demand` interactivity;
  `none` otherwise). Everything outside the banners is click-through.
* Light/dark follows the `org.freedesktop.appearance color-scheme` setting of the
  settings portal (read on a helper thread, live updates included); dark when unknown.

## Architecture

The crate is a platform-neutral **core** plus thin per-OS **presenters**.

| module | role |
| --- | --- |
| `model` | public API types: `Banner`, `Action`, `ReplyStatus`, `OverlayConfig`, `OverlayHandle`, `Command` |
| `layout` | banner geometry in logical px, hit-testing, stack positions |
| `anim` | easing, tweens, the ripple loop (time is always passed in) |
| `edit` | single-line text field state machine (grapheme-safe, paste-less) |
| `state` | `OverlayState`: banners, animation, hover/focus, timers; turns input into `Action`s; tells the presenter when to wake |
| `render` | `render(&OverlayState, scale, Theme) -> tiny_skia::Pixmap` (premultiplied RGBA), `gfx`, `icons`, `dotfont`, `text` (cosmic-text, system fonts) |
| `wayland` | Linux presenter (feature `wayland`, default on Linux): layer-shell, shm, input, portal |

Everything except `wayland` builds and tests on any OS without a display:
`cargo test -p phone-overlay --no-default-features`.

### Porting to Windows / macOS

A presenter owns one native, transparent, always-on-top, click-through-by-default
window and does five things:

1. **Create/destroy the window** from `OverlayState::is_empty()`: map it when the first
   banner arrives, destroy it when the last one is gone.
2. **Size and place it** from `surface_size()` (logical px; anchored top-right, banners
   already include their 12 px edge margin and shadow room), re-evaluated after every update.
3. **Draw**: call `render(&state, scale, theme)` and blit the RGBA pixmap (convert to
   BGRA/premultiplied as the OS wants). Redraw per `next_wake()`: `Wake::Frame` means every
   display frame, `Wake::At(t)` once at `t`, `Wake::Idle` never.
4. **Feed input**: `pointer_motion`/`pointer_leave`/`pointer_button` in surface coordinates,
   `key(Key)` for the focused reply field, `keyboard_focus_lost`. Restrict hit-testing to
   `input_rects()`, and accept keyboard focus only while `wants_keyboard()`.
5. **Plumb commands and actions**: build the app's handle with
   `OverlayHandle::from_sink(|cmd| ...)` (feed `Command`s to `OverlayState::apply`, on the UI
   thread), call `tick(now)` on wake-ups, and forward returned `Action`s to the channel.

Theme: resolve `ThemePref::System` with the OS setting and `Theme::resolve`.

## Preview and demo

```sh
cargo run --example preview            # PNGs of every variant into /var/home/l1/overlay-shots/
cargo run --example demo [seconds]     # the real overlay on the real desktop, prints every Action
PHONE_OVERLAY_DEBUG=1 cargo run --example demo   # also logs scale / theme / surface size
```

## Design tokens

Ink, line and red come from GeekBridge's own palette (`theme.rs` in the desktop app):
`#F4F4F1` / `#0C0C0C` text, one red `#D71921`, dot pitches from the 5x7 `DotFont`.
The panel is the macOS banner treatment: 360 pt wide, 20 pt radius, 14 pt padding, soft
two-layer shadow, 1 px inner highlight; dark `#1C1C1E` and light `#F6F5F1`. The panel is
translucent (95 % / 96 %), a little more opaque than macOS's 88 % because there is no
background blur on wlroots-style compositors and text behind the banner would otherwise
show through. Text contrast is >= 4.5:1 on the worst desktop backdrop (checked in tests).

## Limitations

* Compositors without `zwlr_layer_shell_v1` (GNOME/Mutter): `spawn` returns `Unsupported`.
* No background blur (no standard protocol); the panel is plain translucent.
* Fractional scale uses `wp_fractional_scale_v1` + `wp_viewporter`; without them the integer
  output scale is used. The buffer is rendered at the exact preferred scale, the compositor
  maps the logical size onto it.
* Colour emoji: Noto Color Emoji ships as COLRv1, which the rasteriser cannot draw, so emoji
  render monochrome from Noto Emoji / Symbola when present. With no emoji-capable font installed at all, the primary font's missing-glyph box shows.
* No IME / compose preview in the reply field (committed text only); no paste.

// SPDX-License-Identifier: GPL-3.0-or-later

//! The presenter's state and its Wayland event handlers.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::LoopHandle;
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::{
    wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface,
};
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers};
use smithay_client_toolkit::seat::pointer::{
    CursorIcon, PointerEvent, PointerEventKind, PointerHandler, ThemeSpec, ThemedPointer,
};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm, registry_handlers,
};

use super::keys;
use super::portal::SchemeCell;
use crate::layout::{Hit, Rect};
use crate::model::{Action, Command, OverlayConfig, ThemePref};
use crate::render::render;
use crate::state::{OverlayState, Wake};
use crate::theme::Theme;
use crate::{ButtonState, Key};

/// Linux evdev code of the primary mouse button.
const BTN_LEFT: u32 = 0x110;
/// If a frame callback has not arrived after this long, draw anyway.
const FRAME_STALL: Duration = Duration::from_millis(120);

/// One layer surface and everything attached to it.
struct Surf {
    layer: LayerSurface,
    viewport: Option<WpViewport>,
    fractional: Option<WpFractionalScaleV1>,
    configured: bool,
    /// Logical size last requested.
    size: (u32, u32),
    input_rects: Vec<(i32, i32, i32, i32)>,
    keyboard_on: bool,
}

impl Surf {
    fn destroy(self) {
        if let Some(f) = &self.fractional {
            f.destroy();
        }
        if let Some(v) = &self.viewport {
            v.destroy();
        }
        // The layer surface and wl_surface are destroyed when `layer` drops.
    }
}

/// Everything the presenter thread owns.
pub struct App {
    registry: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    shm: Shm,
    layer_shell: LayerShell,
    frac_manager: Option<WpFractionalScaleManagerV1>,
    viewporter: Option<WpViewporter>,
    pool: SlotPool,
    qh: QueueHandle<App>,
    loop_handle: LoopHandle<'static, App>,

    surf: Option<Surf>,
    state: OverlayState,
    pref: ThemePref,
    scheme: SchemeCell,
    actions: Sender<Action>,

    pointer: Option<ThemedPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mods: Modifiers,
    cursor: CursorIcon,

    /// Preferred fractional scale in 120ths, 0 until announced.
    frac_scale: u32,
    int_scale: i32,

    last_logged: (u32, bool),
    need_draw: bool,
    frame_pending: Option<Instant>,
    wake_at: Option<Instant>,
    /// Set when the presenter should stop.
    pub exit: bool,
}

/// Why binding the Wayland globals failed.
pub enum BindFailure {
    /// A required global is missing.
    Missing,
    /// Something else went wrong.
    Other(String),
}

impl App {
    /// Bind globals and build the presenter state.
    pub fn new(
        globals: &GlobalList,
        qh: &QueueHandle<App>,
        loop_handle: LoopHandle<'static, App>,
        config: &OverlayConfig,
        scheme: SchemeCell,
        actions: Sender<Action>,
    ) -> Result<Self, BindFailure> {
        let compositor = CompositorState::bind(globals, qh).map_err(|_| BindFailure::Missing)?;
        let layer_shell = LayerShell::bind(globals, qh).map_err(|_| BindFailure::Missing)?;
        let shm = Shm::bind(globals, qh).map_err(|_| BindFailure::Missing)?;
        let pool = SlotPool::new(64 * 1024, &shm).map_err(|e| BindFailure::Other(e.to_string()))?;
        let frac_manager = globals
            .bind::<WpFractionalScaleManagerV1, _, _>(qh, 1..=1, ())
            .ok();
        let viewporter = globals.bind::<WpViewporter, _, _>(qh, 1..=1, ()).ok();
        Ok(Self {
            registry: RegistryState::new(globals),
            seat_state: SeatState::new(globals, qh),
            output_state: OutputState::new(globals, qh),
            compositor,
            shm,
            layer_shell,
            frac_manager,
            viewporter,
            pool,
            qh: qh.clone(),
            loop_handle,
            surf: None,
            state: OverlayState::new(config, Instant::now()),
            pref: config.theme,
            scheme,
            actions,
            pointer: None,
            keyboard: None,
            mods: Modifiers::default(),
            cursor: CursorIcon::Default,
            frac_scale: 0,
            int_scale: 1,
            last_logged: (0, false),
            need_draw: false,
            frame_pending: None,
            wake_at: None,
            exit: false,
        })
    }

    // ---- loop plumbing ---------------------------------------------------

    /// How long the event loop may sleep; `None` means until an event arrives.
    pub fn timeout(&self) -> Option<Duration> {
        let now = Instant::now();
        if let Some(t) = self.frame_pending {
            return Some(FRAME_STALL.saturating_sub(now.saturating_duration_since(t)));
        }
        self.wake_at.map(|t| t.saturating_duration_since(now))
    }

    /// An app command arrived.
    pub fn on_command(&mut self, cmd: Command) {
        self.state.apply(cmd, Instant::now());
        self.need_draw = true;
    }

    /// The portal reported a new colour scheme.
    pub fn on_theme_changed(&mut self) {
        if self.pref == ThemePref::System {
            self.need_draw = true;
        }
    }

    /// Run after every batch of events: advance time, manage the surface, draw.
    pub fn after_dispatch(&mut self) {
        let now = Instant::now();
        let actions = self.state.tick(now);
        for a in actions {
            let _ = self.actions.send(a);
        }
        if let Some(t) = self.frame_pending {
            if now.saturating_duration_since(t) >= FRAME_STALL {
                self.frame_pending = None;
                self.need_draw = true;
            }
        }
        if let Some(t) = self.wake_at {
            if now >= t {
                self.need_draw = true;
            }
        }
        if self.state.is_animating() && self.frame_pending.is_none() {
            self.need_draw = true;
        }
        self.sync_surface();
        if self.need_draw && self.frame_pending.is_none() {
            self.draw();
        }
    }

    fn sync_surface(&mut self) {
        if self.state.is_empty() {
            if let Some(s) = self.surf.take() {
                s.destroy();
                self.frame_pending = None;
                self.wake_at = None;
                self.need_draw = false;
            }
            return;
        }
        if self.surf.is_none() {
            self.create_surface();
        }
    }

    fn create_surface(&mut self) {
        let (w, h) = self.state.surface_size();
        let (w, h) = (w.ceil().max(1.0) as u32, h.ceil().max(1.0) as u32);
        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface.clone(),
            Layer::Overlay,
            Some("phone-overlay".to_owned()),
            None,
        );
        layer.set_anchor(Anchor::TOP | Anchor::RIGHT);
        layer.set_margin(0, 0, 0, 0);
        layer.set_exclusive_zone(0);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(w, h);
        let viewport = self
            .viewporter
            .as_ref()
            .map(|v| v.get_viewport(&surface, &self.qh, ()));
        let fractional = match (&self.frac_manager, &viewport) {
            (Some(m), Some(_)) => Some(m.get_fractional_scale(&surface, &self.qh, ())),
            _ => None,
        };
        // Initial commit without a buffer maps nothing yet; the compositor
        // answers with a configure and we draw then.
        layer.commit();
        self.surf = Some(Surf {
            layer,
            viewport,
            fractional,
            configured: false,
            size: (w, h),
            input_rects: Vec::new(),
            keyboard_on: false,
        });
        self.need_draw = true;
    }

    fn draw(&mut self) {
        let Some(surf) = self.surf.as_mut() else {
            return;
        };
        if !surf.configured {
            return;
        }
        self.need_draw = false;
        let (lw, lh) = self.state.surface_size();
        let (lw, lh) = (lw.ceil().max(1.0) as u32, lh.ceil().max(1.0) as u32);
        let scale = if self.frac_scale > 0 && surf.fractional.is_some() && surf.viewport.is_some() {
            self.frac_scale as f32 / 120.0
        } else {
            self.int_scale.max(1) as f32
        };
        let use_viewport =
            surf.fractional.is_some() && surf.viewport.is_some() && self.frac_scale > 0;
        let theme = Theme::resolve(self.pref, self.scheme.is_dark());
        if super::debug_enabled() && self.last_logged != (self.frac_scale, use_viewport) {
            self.last_logged = (self.frac_scale, use_viewport);
            eprintln!(
                "phone-overlay: scale {scale} (fractional preferred {}/120, int {}, viewport {use_viewport}), theme {theme:?}, surface {lw}x{lh}",
                self.frac_scale, self.int_scale
            );
        }
        let t_start = Instant::now();
        let pm = render(&self.state, scale, theme);
        let t_render = t_start.elapsed();
        let (pw, ph) = (pm.width(), pm.height());
        let stride = pw as i32 * 4;

        let created =
            self.pool
                .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888);
        let Ok((buffer, canvas)) = created else {
            // Out of shm: skip this frame, try again on the next event.
            return;
        };
        // tiny-skia is premultiplied RGBA; wl_shm ARGB8888 is BGRA bytes.
        let src_px = pm.data().as_chunks::<4>().0.iter();
        for (dst, src) in canvas.as_chunks_mut::<4>().0.iter_mut().zip(src_px) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }

        let wl = surf.layer.wl_surface().clone();
        if (lw, lh) != surf.size {
            surf.layer.set_size(lw, lh);
            surf.size = (lw, lh);
        }
        if use_viewport {
            if let Some(vp) = &surf.viewport {
                vp.set_destination(lw as i32, lh as i32);
            }
            wl.set_buffer_scale(1);
        } else {
            wl.set_buffer_scale(scale as i32);
        }

        // Click-through everywhere except the banners.
        let rects: Vec<(i32, i32, i32, i32)> = self
            .state
            .input_rects()
            .into_iter()
            .map(|r: Rect| {
                (
                    r.x.floor() as i32,
                    r.y.floor() as i32,
                    r.w.ceil() as i32,
                    r.h.ceil() as i32,
                )
            })
            .collect();
        if rects != surf.input_rects {
            match Region::new(&self.compositor) {
                Ok(region) => {
                    for &(x, y, w, h) in &rects {
                        region.add(x, y, w, h);
                    }
                    surf.layer.set_input_region(Some(region.wl_region()));
                    surf.input_rects = rects;
                }
                Err(_) => surf.layer.set_input_region(None),
            }
        }

        // Keyboard only while a reply field is focused.
        let want_kb = self.state.wants_keyboard();
        if want_kb != surf.keyboard_on {
            surf.layer.set_keyboard_interactivity(if want_kb {
                KeyboardInteractivity::OnDemand
            } else {
                KeyboardInteractivity::None
            });
            surf.keyboard_on = want_kb;
        }

        // Frame pacing: only ask for a callback while something animates.
        let wake = self.state.next_wake();
        self.wake_at = match wake {
            Wake::At(t) => Some(t),
            _ => None,
        };
        if super::debug_enabled() {
            eprintln!(
                "phone-overlay: draw {pw}x{ph} render {:.2} ms, total {:.2} ms, wake={wake:?}",
                t_render.as_secs_f32() * 1000.0,
                t_start.elapsed().as_secs_f32() * 1000.0
            );
        }
        if wake == Wake::Frame {
            wl.frame(&self.qh, wl.clone());
            self.frame_pending = Some(Instant::now());
        }
        wl.damage_buffer(0, 0, pw as i32, ph as i32);
        let _ = buffer.attach_to(&wl);
        surf.layer.commit();
    }

    // ---- input helpers -----------------------------------------------------

    fn is_ours(&self, surface: &wl_surface::WlSurface) -> bool {
        self.surf
            .as_ref()
            .is_some_and(|s| s.layer.wl_surface() == surface)
    }

    fn update_cursor(&mut self, conn: &Connection) {
        let icon = match self.state.hover() {
            Some((_, Hit::Field)) => CursorIcon::Text,
            Some((_, Hit::Body)) | None => CursorIcon::Default,
            Some(_) => CursorIcon::Pointer,
        };
        if let Some(p) = &self.pointer {
            if icon != self.cursor {
                self.cursor = icon;
            }
            let _ = p.set_cursor(conn, icon);
        }
    }

    fn handle_key(&mut self, event: &KeyEvent) {
        let Some(key) = keys::translate(
            event.keysym,
            event.utf8.as_deref(),
            self.mods.ctrl,
            self.mods.alt,
        ) else {
            return;
        };
        self.dispatch_key(key);
    }

    fn dispatch_key(&mut self, key: Key) {
        let actions = self.state.key(key, Instant::now());
        for a in actions {
            let _ = self.actions.send(a);
        }
        self.need_draw = true;
    }
}

// ---- Wayland handlers ------------------------------------------------------

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        if self.is_ours(surface) {
            self.int_scale = new_factor.max(1);
            self.need_draw = true;
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self.is_ours(surface) {
            // Always paint once more after a frame callback: the animation may
            // have just finished, and the settled state still has to be drawn.
            self.frame_pending = None;
            self.need_draw = true;
        }
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        // The compositor took the surface away (e.g. its output vanished):
        // forget it and map a new one if banners are still showing.
        if let Some(s) = self.surf.take() {
            s.destroy();
        }
        self.frame_pending = None;
        self.need_draw = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        _: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(s) = self.surf.as_mut() {
            if s.layer.wl_surface() == layer.wl_surface() {
                s.configured = true;
                self.need_draw = true;
            }
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Pointer if self.pointer.is_none() => {
                let cursor_surface = self.compositor.create_surface(qh);
                if let Ok(p) = self.seat_state.get_pointer_with_theme(
                    qh,
                    &seat,
                    self.shm.wl_shm(),
                    cursor_surface,
                    ThemeSpec::System,
                ) {
                    self.pointer = Some(p);
                }
            }
            Capability::Keyboard if self.keyboard.is_none() => {
                // Key repeat runs on the event loop; repeated keys take the
                // same path as fresh presses.
                let repeat = Box::new(|app: &mut App, _: &wl_keyboard::WlKeyboard, e: KeyEvent| {
                    app.handle_key(&e);
                });
                if let Ok(k) = self.seat_state.get_keyboard_with_repeat(
                    qh,
                    &seat,
                    None,
                    self.loop_handle.clone(),
                    repeat,
                ) {
                    self.keyboard = Some(k);
                }
            }
            _ => {}
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Pointer => {
                if let Some(p) = self.pointer.take() {
                    p.pointer().release();
                }
            }
            Capability::Keyboard => {
                if let Some(k) = self.keyboard.take() {
                    k.release();
                }
            }
            _ => {}
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self.is_ours(surface) {
            self.state.keyboard_focus_lost(Instant::now());
            self.need_draw = true;
        }
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.handle_key(&event);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: u32,
    ) {
        self.mods = modifiers;
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        conn: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let now = Instant::now();
        for event in events {
            if !self.is_ours(&event.surface) {
                continue;
            }
            let (x, y) = (event.position.0 as f32, event.position.1 as f32);
            let before = self.state.hover();
            match event.kind {
                PointerEventKind::Enter { .. } => {
                    self.state.pointer_motion(x, y, now);
                    self.update_cursor(conn);
                    self.need_draw = true;
                }
                PointerEventKind::Leave { .. } => {
                    self.state.pointer_leave(now);
                    self.need_draw = true;
                }
                PointerEventKind::Motion { .. } => {
                    self.state.pointer_motion(x, y, now);
                    if self.state.hover() != before {
                        self.update_cursor(conn);
                        self.need_draw = true;
                    }
                }
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    self.state.pointer_motion(x, y, now);
                    let actions = self.state.pointer_button(ButtonState::Down, now);
                    for a in actions {
                        let _ = self.actions.send(a);
                    }
                    self.need_draw = true;
                }
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                    self.state.pointer_motion(x, y, now);
                    let actions = self.state.pointer_button(ButtonState::Up, now);
                    for a in actions {
                        let _ = self.actions.send(a);
                    }
                    self.need_draw = true;
                }
                _ => {}
            }
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers![OutputState, SeatState];
}

impl Dispatch<WpFractionalScaleManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &WpFractionalScaleManagerV1,
        _: <WpFractionalScaleManagerV1 as smithay_client_toolkit::reexports::client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpFractionalScaleV1, ()> for App {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            if scale > 0 && scale != state.frac_scale {
                state.frac_scale = scale;
                state.need_draw = true;
            }
        }
    }
}

impl Dispatch<WpViewporter, ()> for App {
    fn event(
        _: &mut Self,
        _: &WpViewporter,
        _: <WpViewporter as smithay_client_toolkit::reexports::client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpViewport, ()> for App {
    fn event(
        _: &mut Self,
        _: &WpViewport,
        _: <WpViewport as smithay_client_toolkit::reexports::client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_keyboard!(App);
delegate_pointer!(App);
delegate_layer!(App);
delegate_registry!(App);

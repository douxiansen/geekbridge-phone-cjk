// SPDX-License-Identifier: GPL-3.0-or-later
//! Bringing our own minimised window back on Wayland.
//!
//! A Wayland program cannot un-minimise or focus its own window (winit ignores both), and a
//! minimised window is not redrawn, so the app cannot even act on a request from inside the
//! window. The desktop's taskbar does it through the `wlr-foreign-toplevel-management` protocol;
//! this does the same, for the app's own window only (matched by its `app_id`), from a short-lived
//! thread. Desktops without that protocol keep the toolkit's own attention request.

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{event_created_child, Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1};

/// Why a window could not be raised this way.
#[derive(Debug, PartialEq, Eq)]
pub enum RaiseError {
    /// Not running under Wayland.
    NoWayland,
    /// The desktop has no foreign-toplevel protocol or seat.
    Unsupported(String),
    /// No window with that app id is open.
    NotFound,
}

struct Toplevel {
    handle: ZwlrForeignToplevelHandleV1,
    app_id: String,
}

#[derive(Default)]
struct State {
    toplevels: Vec<Toplevel>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(state: &mut Self, _: &ZwlrForeignToplevelManagerV1, event: zwlr_foreign_toplevel_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            state.toplevels.push(Toplevel { handle: toplevel, app_id: String::new() });
        }
    }

    event_created_child!(State, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for State {
    fn event(state: &mut Self, handle: &ZwlrForeignToplevelHandleV1, event: zwlr_foreign_toplevel_handle_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(t) = state.toplevels.iter_mut().find(|t| &t.handle == handle) {
                    t.app_id = app_id;
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => state.toplevels.retain(|t| &t.handle != handle),
            _ => {}
        }
    }
}

/// Un-minimises and focuses every window of this app (`app_id`). Blocks for a few round trips
/// with the compositor, so call it from a helper thread.
pub fn raise(app_id: &str) -> Result<(), RaiseError> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Err(RaiseError::NoWayland);
    }
    let conn = Connection::connect_to_env().map_err(|e| RaiseError::Unsupported(e.to_string()))?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|e| RaiseError::Unsupported(e.to_string()))?;
    let qh = queue.handle();
    let _manager: ZwlrForeignToplevelManagerV1 = globals.bind(&qh, 1..=3, ()).map_err(|e| RaiseError::Unsupported(format!("no foreign-toplevel protocol: {e}")))?;
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ()).map_err(|e| RaiseError::Unsupported(format!("no seat: {e}")))?;

    let mut state = State::default();
    // The first round trip announces the windows, the second their app ids.
    for _ in 0..2 {
        queue.roundtrip(&mut state).map_err(|e| RaiseError::Unsupported(e.to_string()))?;
    }
    let mut found = false;
    for t in state.toplevels.iter().filter(|t| t.app_id == app_id) {
        t.handle.unset_minimized();
        t.handle.activate(&seat);
        found = true;
    }
    queue.roundtrip(&mut state).map_err(|e| RaiseError::Unsupported(e.to_string()))?;
    if found {
        Ok(())
    } else {
        Err(RaiseError::NotFound)
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later

//! Windows presenter: one topmost, non-activating, layered window in the top-right corner of the
//! primary monitor's work area. The platform-neutral core draws the banners to a pixmap; this
//! module only moves those pixels into the window (per-pixel alpha) and the mouse and keyboard
//! back.
//!
//! One thread owns the window and its message loop. It sleeps in `MsgWaitForMultipleObjectsEx`
//! until a command, a window message or the next timed wake-up arrives; with nothing on screen
//! the window is hidden and there is no wake-up at all.

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_BACK, VK_CONTROL,
    VK_DELETE, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetWindowLongPtrW,
    LoadCursorW, MsgWaitForMultipleObjectsEx, PeekMessageW, PostMessageW, RegisterClassW,
    SetForegroundWindow, SetWindowLongPtrW, ShowWindow, SystemParametersInfoW, TranslateMessage,
    UpdateLayeredWindow, GWLP_USERDATA, GWL_EXSTYLE, HTCLIENT, HTTRANSPARENT, IDC_ARROW,
    MA_NOACTIVATE, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, SPI_GETWORKAREA, SW_HIDE,
    SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, ULW_ALPHA, WM_APP, WM_CHAR, WM_KEYDOWN,
    WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NCHITTEST,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::layout::Rect;
use crate::{
    render, Action, ButtonState, Command, Key, OverlayConfig, OverlayError, OverlayHandle,
    OverlayState, Theme, ThemePref, Wake,
};

const WM_WAKE: u32 = WM_APP + 1;
/// Not exported by the bindings.
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Redraw rate while something animates.
const FRAME: Duration = Duration::from_millis(16);

fn presenter_err<E: std::fmt::Debug>(e: E) -> OverlayError {
    OverlayError::Presenter(format!("{e:?}"))
}

/// Whether Windows is set to dark app mode.
fn system_is_dark() -> bool {
    let mut data: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    // 1 means light. Dark when the value is missing, like the Linux presenter.
    r.is_err() || data == 0
}

struct App {
    hwnd: HWND,
    state: OverlayState,
    actions: Sender<Action>,
    pref: ThemePref,
    dark: (bool, Instant),
    scale: f32,
    /// Top-left of the window on screen, in physical pixels.
    origin: (i32, i32),
    input: Vec<Rect>,
    dib: Option<Dib>,
    visible: bool,
    keyboard_on: bool,
    tracking: bool,
    high_surrogate: Option<u16>,
    dirty: bool,
    wake_at: Option<Instant>,
    animating: bool,
}

struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    w: i32,
    h: i32,
}

impl Dib {
    fn new(w: i32, h: i32) -> Option<Self> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return None;
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap =
                CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            if bits.is_null() {
                let _ = DeleteObject(bitmap.into());
                let _ = DeleteDC(dc);
                return None;
            }
            let old = SelectObject(dc, bitmap.into());
            Some(Self {
                dc,
                bitmap,
                old,
                bits: bits as *mut u8,
                w,
                h,
            })
        }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

impl App {
    fn theme(&mut self) -> Theme {
        let dark = match self.pref {
            ThemePref::Dark => true,
            ThemePref::Light => false,
            ThemePref::System => {
                if self.dark.1.elapsed() > Duration::from_secs(2) {
                    self.dark = (system_is_dark(), Instant::now());
                }
                self.dark.0
            }
        };
        if dark {
            Theme::Dark
        } else {
            Theme::Light
        }
    }

    fn send(&self, actions: Vec<Action>) {
        for a in actions {
            let _ = self.actions.send(a);
        }
    }

    /// Advance time, then draw if something changed.
    fn after_events(&mut self) {
        let now = Instant::now();
        let actions = self.state.tick(now);
        self.send(actions);
        if self.state.is_empty() {
            if self.visible {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
                self.visible = false;
                self.dib = None;
            }
            self.wake_at = None;
            self.animating = false;
            self.dirty = false;
            return;
        }
        let due = self.wake_at.is_some_and(|t| now >= t);
        if self.dirty || self.animating || due {
            self.draw();
        }
    }

    fn timeout_ms(&self) -> u32 {
        if self.animating {
            return FRAME.as_millis() as u32;
        }
        match self.wake_at {
            Some(t) => t
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(u32::MAX as u128 - 1) as u32,
            None => u32::MAX, // INFINITE
        }
    }

    fn draw(&mut self) {
        self.dirty = false;
        self.scale = (unsafe { GetDpiForSystem() } as f32 / 96.0).max(1.0);
        let theme = self.theme();
        let pm = render(&self.state, self.scale, theme);
        let (pw, ph) = (pm.width() as i32, pm.height() as i32);
        if pw == 0 || ph == 0 {
            return;
        }
        if self.dib.as_ref().is_none_or(|d| d.w != pw || d.h != ph) {
            self.dib = Dib::new(pw, ph);
        }
        let Some(dib) = self.dib.as_ref() else { return };
        // tiny-skia is premultiplied RGBA; a 32-bit DIB with AC_SRC_ALPHA is premultiplied BGRA.
        unsafe {
            let dst = std::slice::from_raw_parts_mut(dib.bits, (pw * ph * 4) as usize);
            let src = pm.data();
            for (d, s) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<4>().0)
            {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
            }
        }
        // Top-right corner of the primary monitor's work area (clear of the taskbar).
        let mut work = RECT::default();
        unsafe {
            let _ = SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some(&mut work as *mut RECT as *mut c_void),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
        }
        self.origin = (work.right - pw, work.top);
        let dst = POINT {
            x: self.origin.0,
            y: self.origin.1,
        };
        let size = SIZE { cx: pw, cy: ph };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            let screen = GetDC(None);
            let _ = UpdateLayeredWindow(
                self.hwnd,
                Some(screen),
                Some(&dst),
                Some(&size),
                Some(dib.dc),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            ReleaseDC(None, screen);
            if !self.visible {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                self.visible = true;
            }
        }
        self.input = self.state.input_rects();
        self.sync_keyboard();
        match self.state.next_wake() {
            Wake::Frame => {
                self.animating = true;
                self.wake_at = None;
            }
            Wake::At(t) => {
                self.animating = false;
                self.wake_at = Some(t);
            }
            Wake::Idle => {
                self.animating = false;
                self.wake_at = None;
            }
        }
    }

    /// The window takes keyboard focus only while a reply field is being edited.
    fn sync_keyboard(&mut self) {
        let want = self.state.wants_keyboard();
        if want == self.keyboard_on {
            return;
        }
        self.keyboard_on = want;
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let noact = WS_EX_NOACTIVATE.0 as isize;
            if want {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex & !noact);
                let _ = SetForegroundWindow(self.hwnd);
                let _ = SetFocus(Some(self.hwnd));
            } else {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex | noact);
            }
        }
    }

    fn logical(&self, x: i32, y: i32) -> (f32, f32) {
        (x as f32 / self.scale, y as f32 / self.scale)
    }

    fn pointer(&mut self, lparam: LPARAM) {
        let x = (lparam.0 & 0xffff) as i16 as i32;
        let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
        let (lx, ly) = self.logical(x, y);
        self.state.pointer_motion(lx, ly, Instant::now());
        self.dirty = true;
    }

    fn key(&mut self, key: Key) {
        let actions = self.state.key(key, Instant::now());
        self.send(actions);
        self.dirty = true;
    }
}

fn ctrl_down() -> bool {
    unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut App;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    // SAFETY: the pointer is set once by `run` for a Box that outlives the window, and messages
    // are only dispatched from that thread, never while `run` itself holds a borrow.
    let app = unsafe { &mut *ptr };
    match msg {
        WM_MOUSEACTIVATE if !app.keyboard_on => LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST => {
            // Click-through everywhere except the banners (the shadow margin is not clickable).
            let sx = (lparam.0 & 0xffff) as i16 as i32;
            let sy = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
            let (lx, ly) = app.logical(sx - app.origin.0, sy - app.origin.1);
            if app.input.iter().any(|r| r.contains(lx, ly)) {
                LRESULT(HTCLIENT as isize)
            } else {
                LRESULT(HTTRANSPARENT as isize)
            }
        }
        WM_MOUSEMOVE => {
            if !app.tracking {
                let mut t = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = unsafe { TrackMouseEvent(&mut t) };
                app.tracking = true;
            }
            app.pointer(lparam);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            app.tracking = false;
            app.state.pointer_leave(Instant::now());
            app.dirty = true;
            LRESULT(0)
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP => {
            app.pointer(lparam);
            let st = if msg == WM_LBUTTONDOWN {
                ButtonState::Down
            } else {
                ButtonState::Up
            };
            let actions = app.state.pointer_button(st, Instant::now());
            app.send(actions);
            app.dirty = true;
            LRESULT(0)
        }
        WM_CHAR => {
            let unit = wparam.0 as u16;
            // Control characters (Enter, Backspace, ...) arrive as keys, not text.
            if unit >= 0x20 && unit != 0x7f {
                let text = if (0xD800..0xDC00).contains(&unit) {
                    app.high_surrogate = Some(unit);
                    None
                } else if let Some(hi) = app
                    .high_surrogate
                    .take()
                    .filter(|_| (0xDC00..0xE000).contains(&unit))
                {
                    String::from_utf16(&[hi, unit]).ok()
                } else {
                    String::from_utf16(&[unit]).ok()
                };
                if let Some(t) = text {
                    app.key(Key::Text(t));
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let ctrl = ctrl_down();
            let vk = wparam.0 as u16;
            let key = match vk {
                v if v == VK_BACK.0 => Some(if ctrl {
                    Key::BackspaceWord
                } else {
                    Key::Backspace
                }),
                v if v == VK_DELETE.0 => Some(Key::Delete),
                v if v == VK_LEFT.0 => Some(if ctrl { Key::WordLeft } else { Key::Left }),
                v if v == VK_RIGHT.0 => Some(if ctrl { Key::WordRight } else { Key::Right }),
                v if v == VK_HOME.0 => Some(Key::Home),
                v if v == VK_END.0 => Some(Key::End),
                v if v == VK_RETURN.0 => Some(Key::Enter),
                v if v == VK_ESCAPE.0 => Some(Key::Escape),
                _ => None,
            };
            if let Some(k) = key {
                app.key(k);
            }
            LRESULT(0)
        }
        WM_KILLFOCUS => {
            app.state.keyboard_focus_lost(Instant::now());
            app.dirty = true;
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_WAKE => LRESULT(0),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn create_window() -> Result<HWND, OverlayError> {
    unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(PCWSTR::null())
            .map_err(presenter_err)?
            .into();
        let class = w!("GeekBridgePhoneOverlay");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(presenter_err)?,
            ..Default::default()
        };
        // Registering twice (a second overlay in one process) fails harmlessly.
        RegisterClassW(&wc);
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("GeekBridge Phone"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(hinstance),
            None,
        )
        .map_err(presenter_err)
    }
}

fn run(
    config: OverlayConfig,
    actions: Sender<Action>,
    commands: Receiver<Command>,
    hwnd_cell: Arc<AtomicIsize>,
    ready: mpsc::SyncSender<Result<(), OverlayError>>,
) {
    let hwnd = match create_window() {
        Ok(h) => h,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let mut app = Box::new(App {
        hwnd,
        state: OverlayState::new(&config, Instant::now()),
        actions,
        pref: config.theme,
        dark: (true, Instant::now() - Duration::from_secs(60)),
        scale: 1.0,
        origin: (0, 0),
        input: Vec::new(),
        dib: None,
        visible: false,
        keyboard_on: false,
        tracking: false,
        high_surrogate: None,
        dirty: false,
        wake_at: None,
        animating: false,
    });
    let ptr: *mut App = &mut *app;
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
    }
    hwnd_cell.store(hwnd.0 as isize, Ordering::SeqCst);
    let _ = ready.send(Ok(()));

    'main: loop {
        // Commands from the app.
        loop {
            match commands.try_recv() {
                Ok(cmd) => {
                    let a = unsafe { &mut *ptr };
                    a.state.apply(cmd, Instant::now());
                    a.dirty = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break 'main,
            }
        }
        // Window messages (mouse, keys). The window procedure reaches `app` through its pointer.
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        unsafe { (*ptr).after_events() };
        let timeout = unsafe { (*ptr).timeout_ms() };
        unsafe {
            MsgWaitForMultipleObjectsEx(None, timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let _ = DestroyWindow(hwnd);
    }
}

/// Start the Windows presenter.
///
/// # Errors
///
/// [`OverlayError::Presenter`] when the window cannot be created.
pub fn spawn(config: OverlayConfig) -> Result<(OverlayHandle, Receiver<Action>), OverlayError> {
    // Font discovery takes a moment: do it in the background so the first banner is quick.
    let _ = std::thread::Builder::new()
        .name("phone-overlay-fonts".into())
        .spawn(crate::text::warm_up);
    let (act_tx, act_rx) = mpsc::channel();
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let hwnd_cell = Arc::new(AtomicIsize::new(0));
    let cell = hwnd_cell.clone();
    std::thread::Builder::new()
        .name("phone-overlay".into())
        .spawn(move || run(config, act_tx, cmd_rx, cell, ready_tx))
        .map_err(presenter_err)?;
    ready_rx
        .recv()
        .map_err(|_| OverlayError::Presenter("presenter thread exited during start-up".into()))??;
    let cmd_tx = std::sync::Mutex::new(cmd_tx);
    let handle = OverlayHandle::from_sink(move |cmd| {
        if let Ok(tx) = cmd_tx.lock() {
            let _ = tx.send(cmd);
        }
        let raw = hwnd_cell.load(Ordering::SeqCst);
        if raw != 0 {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(raw as *mut c_void)),
                    WM_WAKE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    });
    Ok((handle, act_rx))
}

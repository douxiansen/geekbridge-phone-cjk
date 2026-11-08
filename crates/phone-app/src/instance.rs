// SPDX-License-Identifier: GPL-3.0-or-later
//! One app per user. A second launch hands its request to the first (show the window, or open a
//! conversation) and exits, so the phone's single message session is never fought over.
//!
//! Unix: a socket in the runtime directory. TODO(windows): a named pipe
//! (`\\.\pipe\geekbridge-phone`) or a mutex plus `WM_COPYDATA`. TODO(macos): the same Unix socket
//! works; `NSApplication` reopen events can be added on top. The non-Unix [`acquire`] below
//! always reports [`Role::Primary`], so those builds run, without the guard.

use crate::action::{ActionSender, UiAction};
use std::path::PathBuf;

/// What a launch asks of a running instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Show,
    /// Show the conversation with this number.
    Open(String),
}

/// Protocol tag: lets a launch tell our socket from anything else that might sit on the path.
const HELLO: &str = "GBP1";

impl Request {
    fn encode(&self) -> String {
        match self {
            Request::Show => format!("{HELLO} show\n"),
            Request::Open(n) => format!("{HELLO} open {}\n", n.replace(['\n', '\r'], " ")),
        }
    }

    fn decode(line: &str) -> Option<Request> {
        let rest = line.trim_end().strip_prefix(HELLO)?.strip_prefix(' ')?;
        match rest.split_once(' ') {
            None if rest == "show" => Some(Request::Show),
            Some(("open", n)) if !n.trim().is_empty() => Some(Request::Open(n.trim().to_string())),
            _ => None,
        }
    }

    fn into_action(self) -> UiAction {
        match self {
            Request::Show => UiAction::Show,
            Request::Open(number) => UiAction::OpenConversation { number },
        }
    }
}

/// The outcome of [`acquire`].
pub enum Role {
    /// This is the first instance. Call [`Guard::serve`] once the window exists.
    Primary(Guard),
    /// Another instance took the request; this process should exit.
    Secondary,
    /// Something answers on the socket but is not a healthy instance.
    Unresponsive(String),
}

/// Held by the first instance for as long as it runs.
pub struct Guard {
    #[cfg(unix)]
    inner: Option<unix::Bound>,
}

impl Guard {
    /// Starts answering other launches. They become [`UiAction`]s on `actions`.
    pub fn serve(&mut self, actions: ActionSender) {
        #[cfg(unix)]
        if let Some(b) = self.inner.take() {
            unix::serve(b, actions);
        }
        #[cfg(not(unix))]
        let _ = actions;
    }
}

/// Becomes the first instance, or forwards `request` to the one that is running. `dir` is a
/// directory only this user can write (the runtime directory, or the app's data directory).
#[cfg(unix)]
pub fn acquire(dir: &std::path::Path, name: &str, request: &Request) -> Role {
    unix::acquire(dir, name, request)
}

/// Windows and others: no guard yet.
#[cfg(not(unix))]
pub fn acquire(_dir: &std::path::Path, _name: &str, _request: &Request) -> Role {
    Role::Primary(Guard {})
}

/// Where the socket lives: the runtime directory if there is one.
pub fn socket_dir(fallback: PathBuf) -> PathBuf {
    dirs_runtime().unwrap_or(fallback)
}

fn dirs_runtime() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|p| p.is_dir())
}

#[cfg(unix)]
mod unix {
    use super::{Guard, Request, Role, HELLO};
    use crate::action::ActionSender;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    const IO_TIMEOUT: Duration = Duration::from_secs(2);
    const MAX_LINE: u64 = 512;

    pub struct Bound {
        listener: UnixListener,
        path: PathBuf,
    }

    /// Removes the socket file when the first instance exits.
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn socket_path(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.sock"))
    }

    /// Sends `request` to a running instance. `Ok(())` when it said it took it.
    fn forward(path: &Path, request: &Request) -> std::io::Result<()> {
        let mut stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        stream.write_all(request.encode().as_bytes())?;
        let mut reply = String::new();
        BufReader::new(stream.take(MAX_LINE)).read_line(&mut reply)?;
        if reply.trim_end() == format!("ok {HELLO}") {
            Ok(())
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "unexpected reply"))
        }
    }

    pub fn acquire(dir: &Path, name: &str, request: &Request) -> Role {
        let path = socket_path(dir, name);
        for _ in 0..3 {
            match forward(&path, request) {
                Ok(()) => return Role::Secondary,
                // Nothing there, or a dead socket file left by a crash: take over.
                Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused) => {
                    let _ = std::fs::remove_file(&path);
                    match UnixListener::bind(&path) {
                        Ok(listener) => {
                            // Keep the file private to this user.
                            use std::os::unix::fs::PermissionsExt;
                            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
                            return Role::Primary(Guard { inner: Some(Bound { listener, path }) });
                        }
                        // Another launch won the race: ask it instead.
                        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
                        Err(e) => return Role::Unresponsive(format!("cannot create {}: {e}", path.display())),
                    }
                }
                Err(e) => return Role::Unresponsive(format!("{}: {e}", path.display())),
            }
        }
        Role::Unresponsive(format!("{} keeps changing", path.display()))
    }

    pub fn serve(bound: Bound, actions: ActionSender) {
        let Bound { listener, path } = bound;
        let spawned = std::thread::Builder::new().name("gbp-instance".into()).spawn(move || {
            let _cleanup = Cleanup(path);
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                let mut line = String::new();
                let read = BufReader::new((&stream).take(MAX_LINE)).read_line(&mut line);
                if read.is_err() {
                    continue;
                }
                if let Some(request) = Request::decode(&line) {
                    actions.send(request.into_action());
                    let _ = stream.write_all(format!("ok {HELLO}\n").as_bytes());
                }
            }
        });
        if let Err(e) = spawned {
            log::warn!("single-instance listener not started: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip() {
        for r in [Request::Show, Request::Open("+213 550 123 456".into())] {
            assert_eq!(Request::decode(&r.encode()), Some(r));
        }
    }

    #[test]
    fn garbage_is_not_a_request() {
        assert_eq!(Request::decode(""), None);
        assert_eq!(Request::decode("show\n"), None);
        assert_eq!(Request::decode("GBP1 open \n"), None);
        assert_eq!(Request::decode("GBP2 show\n"), None);
        assert_eq!(Request::decode("GBP1 format c:\n"), None);
    }

    #[test]
    fn a_number_cannot_smuggle_a_second_request() {
        let line = Request::Open("1\nGBP1 show".into()).encode();
        assert_eq!(line.matches('\n').count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn the_second_launch_reaches_the_first() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let woke = Arc::new(AtomicBool::new(false));
        let w = woke.clone();
        let (actions, rx) = ActionSender::new(Arc::new(move || w.store(true, Ordering::SeqCst)));

        let Role::Primary(mut guard) = acquire(dir.path(), "t", &Request::Show) else {
            panic!("first launch should be primary");
        };
        guard.serve(actions);

        match acquire(dir.path(), "t", &Request::Open("+1555".into())) {
            Role::Secondary => {}
            _ => panic!("second launch should hand over"),
        }
        let got = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        assert_eq!(got, UiAction::OpenConversation { number: "+1555".into() });
        assert!(woke.load(Ordering::SeqCst));
    }

    #[cfg(unix)]
    #[test]
    fn a_stale_socket_from_a_crash_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        // Bind and drop: leaves the socket file behind with nobody listening.
        drop(std::os::unix::net::UnixListener::bind(dir.path().join("t.sock")).unwrap());
        assert!(matches!(acquire(dir.path(), "t", &Request::Show), Role::Primary(_)));
    }

    #[cfg(unix)]
    #[test]
    fn something_else_on_the_path_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let l = std::os::unix::net::UnixListener::bind(dir.path().join("t.sock")).unwrap();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                use std::io::Write;
                let _ = s.write_all(b"hello stranger\n");
            }
        });
        assert!(matches!(acquire(dir.path(), "t", &Request::Show), Role::Unresponsive(_)));
    }
}

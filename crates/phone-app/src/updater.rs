// SPDX-License-Identifier: GPL-3.0-or-later
//! Keeps the app up to date from GitHub releases.
//!
//! A worker thread looks for a newer release shortly after start and then about once a day (if
//! `Settings::check_updates` is on), and on request. The UI reads [`Updater::status`] and starts
//! an install with [`Updater::install`]; the download is verified (checksum and signature) by
//! `phone-update` before anything is replaced.

use crate::action::{ActionSender, UiAction};
use phone_core::service::Handle;
use phone_update::{Http, Update, UpdateError, UreqHttp, Version};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

/// The version of the running program.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// How long after start the first automatic check waits, so startup stays quiet.
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(45);
/// How often the automatic check repeats.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    /// Checked just now and nothing newer exists.
    UpToDate,
    Available(Update),
    Installing(String),
    /// Installed; the app is about to restart.
    Installed(String),
    Failed(String),
    /// The program lives where a package manager owns it: update it there.
    ManagedElsewhere,
}

enum Cmd {
    CheckNow,
    Install,
}

struct Inner {
    status: Mutex<UpdateStatus>,
    /// A release the user dismissed the banner for.
    dismissed: Mutex<Option<Version>>,
    last_checked: Mutex<Option<Instant>>,
    tx: Mutex<Sender<Cmd>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A cloneable handle to the updater thread.
#[derive(Clone)]
pub struct Updater {
    inner: Arc<Inner>,
}

impl Updater {
    /// Starts the updater thread. `wake` repaints the window when the status changes.
    pub fn start(handle: Handle, actions: ActionSender, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = mpsc::channel();
        let inner = Arc::new(Inner {
            status: Mutex::new(UpdateStatus::Idle),
            dismissed: Mutex::new(None),
            last_checked: Mutex::new(None),
            tx: Mutex::new(tx),
        });
        let worker = inner.clone();
        let spawned = thread::Builder::new()
            .name("gbp-update".into())
            .spawn(move || {
                let http = UreqHttp::new(CURRENT_VERSION);
                let mut next_auto = Instant::now() + FIRST_CHECK_AFTER;
                loop {
                    let wait = next_auto.saturating_duration_since(Instant::now());
                    match rx.recv_timeout(wait) {
                        Ok(Cmd::CheckNow) => check(&worker, &http, &wake),
                        Ok(Cmd::Install) => install(&worker, &http, &actions, &wake),
                        Err(RecvTimeoutError::Timeout) => {
                            next_auto = Instant::now() + CHECK_EVERY;
                            if handle.state().settings.check_updates {
                                check(&worker, &http, &wake);
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            });
        if let Err(e) = spawned {
            log::warn!("cannot start the updater: {e}");
        }
        Self { inner }
    }

    pub fn status(&self) -> UpdateStatus {
        lock(&self.inner.status).clone()
    }

    /// The release to announce, unless the user dismissed this one.
    pub fn announced(&self) -> Option<Update> {
        match self.status() {
            UpdateStatus::Available(u)
                if lock(&self.inner.dismissed).as_ref() != Some(&u.version) =>
            {
                Some(u)
            }
            _ => None,
        }
    }

    pub fn dismiss(&self) {
        if let UpdateStatus::Available(u) = self.status() {
            *lock(&self.inner.dismissed) = Some(u.version);
        }
    }

    pub fn last_checked(&self) -> Option<Instant> {
        *lock(&self.inner.last_checked)
    }

    pub fn check_now(&self) {
        let _ = lock(&self.inner.tx).send(Cmd::CheckNow);
    }

    pub fn install(&self) {
        let _ = lock(&self.inner.tx).send(Cmd::Install);
    }
}

fn set(inner: &Inner, status: UpdateStatus, wake: &Arc<dyn Fn() + Send + Sync>) {
    *lock(&inner.status) = status;
    wake();
}

/// Whether the running program can be replaced where it lives.
fn replaceable() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if phone_update::install::can_replace(&exe) {
        Ok(exe)
    } else {
        Err("not writable".to_string())
    }
}

fn check(inner: &Inner, http: &dyn Http, wake: &Arc<dyn Fn() + Send + Sync>) {
    if matches!(
        *lock(&inner.status),
        UpdateStatus::Installing(_) | UpdateStatus::Installed(_)
    ) {
        return;
    }
    set(inner, UpdateStatus::Checking, wake);
    let current = Version::parse(CURRENT_VERSION).unwrap_or(Version {
        major: 0,
        minor: 0,
        patch: 0,
        pre: None,
    });
    let result = phone_update::release::check(
        http,
        phone_update::REPO,
        &current,
        &phone_update::platform_suffix(),
    );
    *lock(&inner.last_checked) = Some(Instant::now());
    let status = match result {
        Ok(Some(update)) if replaceable().is_err() => {
            log::info!(
                "version {} is available but this install is not writable",
                update.version
            );
            UpdateStatus::ManagedElsewhere
        }
        Ok(Some(update)) => UpdateStatus::Available(update),
        Ok(None) => UpdateStatus::UpToDate,
        Err(UpdateError::NoAsset(p)) => {
            log::info!("the latest release has no build for {p}");
            UpdateStatus::UpToDate
        }
        Err(e) => {
            log::info!("update check failed: {e}");
            UpdateStatus::Failed(e.to_string())
        }
    };
    set(inner, status, wake);
}

fn install(
    inner: &Inner,
    http: &dyn Http,
    actions: &ActionSender,
    wake: &Arc<dyn Fn() + Send + Sync>,
) {
    let UpdateStatus::Available(update) = lock(&inner.status).clone() else {
        return;
    };
    let version = update.version.to_string();
    let exe = match replaceable() {
        Ok(e) => e,
        Err(_) => return set(inner, UpdateStatus::ManagedElsewhere, wake),
    };
    set(inner, UpdateStatus::Installing(version.clone()), wake);
    let work = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("geekbridge-phone")
        .join("update");
    match phone_update::apply(http, &update, &work, &exe) {
        Ok(_) => {
            set(inner, UpdateStatus::Installed(version), wake);
            restart(&exe, actions);
        }
        Err(e) => {
            log::warn!("update failed: {e}");
            set(inner, UpdateStatus::Failed(e.to_string()), wake);
        }
    }
}

/// Starts the new program in place of this one.
#[cfg(unix)]
fn restart(exe: &std::path::Path, actions: &ActionSender) {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    // Under systemd, let it restart the service (it stops us and starts the new program).
    if std::env::var_os("INVOCATION_ID").is_some() {
        let ok = Command::new("systemctl")
            .args(["--user", "restart", "geekbridge-phone.service"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok();
        if ok {
            return;
        }
    }
    // Otherwise a helper waits for this program to exit, then starts the new one.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let spawned = Command::new("sh")
        .arg("-c")
        .arg("sleep 2; exec \"$0\" \"$@\"")
        .arg(exe)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
    match spawned {
        Ok(_) => actions.send(UiAction::Quit),
        Err(e) => log::warn!("cannot restart after the update: {e}"),
    }
}

#[cfg(not(unix))]
fn restart(_exe: &std::path::Path, _actions: &ActionSender) {
    // TODO(windows): replace a running .exe via a helper that waits for this process.
}

/// `geekbridge-phone --check-update`: prints whether a newer release exists.
pub fn cli_check() -> Result<String, String> {
    let http = UreqHttp::new(CURRENT_VERSION);
    let current = Version::parse(CURRENT_VERSION).ok_or("bad version")?;
    match phone_update::release::check(
        &http,
        phone_update::REPO,
        &current,
        &phone_update::platform_suffix(),
    ) {
        Ok(Some(u)) => Ok(format!(
            "version {} is available (you have {CURRENT_VERSION})",
            u.version
        )),
        Ok(None) => Ok(format!("up to date ({CURRENT_VERSION})")),
        Err(e) => Err(e.to_string()),
    }
}

/// `geekbridge-phone --update`: installs the latest release now, without the window.
pub fn cli_update() -> Result<String, String> {
    let http = UreqHttp::new(CURRENT_VERSION);
    let current = Version::parse(CURRENT_VERSION).ok_or("bad version")?;
    let exe = replaceable().map_err(|_| {
        "this program is not in a writable folder; update it with your package manager".to_string()
    })?;
    let Some(update) = phone_update::release::check(
        &http,
        phone_update::REPO,
        &current,
        &phone_update::platform_suffix(),
    )
    .map_err(|e| e.to_string())?
    else {
        return Ok(format!("already up to date ({CURRENT_VERSION})"));
    };
    let work = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("geekbridge-phone")
        .join("update");
    phone_update::apply(&http, &update, &work, &exe).map_err(|e| e.to_string())?;
    Ok(format!("updated {CURRENT_VERSION} -> {}; restart the app (systemctl --user restart geekbridge-phone.service)", update.version))
}

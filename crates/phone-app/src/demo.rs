// SPDX-License-Identifier: GPL-3.0-or-later
//! `--demo`: the app against a fake phone with invented contacts and texts, in a throwaway data
//! folder, so every screen can be shown and photographed without touching a real phone.

use phone_core::demo::DemoPlatform;
use phone_core::service::Handle;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// A contact in the demo phonebook, and one that is not.
pub const KNOWN_NUMBER: &str = "+213550123456";
pub const UNKNOWN_NUMBER: &str = "+14155550142";

/// A scripted situation to put the demo in at start-up (`--scene`), for screenshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    /// A known contact is ringing.
    Ring,
    /// An unknown number is ringing.
    RingUnknown,
    /// A call has been answered and is running.
    Active,
    /// An outgoing call is being placed.
    Dialing,
    /// A new text arrives.
    Text,
    /// The phone has not allowed access to messages and contacts.
    Denied,
    /// The phone is paired but not connected.
    Offline,
}

impl Scene {
    pub fn parse(s: &str) -> Option<Scene> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "ring" => Scene::Ring,
            "ring-unknown" => Scene::RingUnknown,
            "active" => Scene::Active,
            "dialing" => Scene::Dialing,
            "text" => Scene::Text,
            "denied" => Scene::Denied,
            "offline" => Scene::Offline,
            _ => return None,
        })
    }
}

/// Drives the fake phone from the Help menu and from scenes.
#[derive(Clone)]
pub struct DemoControl {
    platform: Arc<DemoPlatform>,
}

impl DemoControl {
    pub fn new(platform: Arc<DemoPlatform>) -> Self {
        Self { platform }
    }

    pub fn ring(&self, number: &str) {
        self.platform.ring(number);
    }

    pub fn text(&self, from: &str, body: &str) {
        self.platform.text(from, body);
    }

    /// Rings, then gives up before anyone answers: a missed call.
    pub fn miss(&self, number: &str) {
        let control = self.clone();
        let number = number.to_string();
        let spawned = std::thread::Builder::new()
            .name("gbp-demo-miss".into())
            .spawn(move || {
                control.ring(&number);
                std::thread::sleep(Duration::from_millis(3000));
                control.end_call();
            });
        if let Err(e) = spawned {
            log::warn!("cannot simulate a missed call: {e}");
        }
    }

    /// Fills Recents: a missed call, a short answered one and one placed from here.
    pub fn seed_history(&self, handle: Handle) {
        let control = self.clone();
        let spawned = std::thread::Builder::new()
            .name("gbp-demo-history".into())
            .spawn(move || {
                let pause = |ms| std::thread::sleep(Duration::from_millis(ms));
                pause(800);
                control.ring(KNOWN_NUMBER);
                pause(1500);
                control.end_call();
                pause(3200);
                control.ring(UNKNOWN_NUMBER);
                pause(1200);
                handle.answer("/demo/call1");
                pause(2500);
                control.end_call();
                pause(3200);
                handle.dial("+33612345678");
                pause(1200);
                control.end_call();
            });
        if let Err(e) = spawned {
            log::warn!("cannot seed the demo history: {e}");
        }
    }

    pub fn end_call(&self) {
        self.platform.hang_up_now();
    }

    /// Connects or disconnects the fake phone.
    pub fn set_connected(&self, connected: bool) {
        if let Ok(mut c) = self.platform.connected.lock() {
            *c = connected;
        }
    }

    pub fn is_connected(&self) -> bool {
        self.platform.connected.lock().map(|c| *c).unwrap_or(false)
    }

    /// Sets the fake phone up for `scene` before the service first talks to it.
    pub fn prepare(&self, scene: Scene) {
        if scene == Scene::Offline {
            self.set_connected(false);
        }
        if scene == Scene::Denied {
            if let Ok(mut state) = self.platform.server.state.lock() {
                // OBEX "forbidden": the phone's allow prompt was refused.
                state.refuse_with = Some(0xC1);
            }
        }
    }

    /// Plays a scene on a helper thread so the UI never waits for it.
    pub fn play(&self, scene: Scene, handle: Handle) {
        let control = self.clone();
        let spawned = std::thread::Builder::new()
            .name("gbp-demo-scene".into())
            .spawn(move || {
                // Let the service find the phone and load the data first.
                std::thread::sleep(Duration::from_millis(2500));
                match scene {
                    Scene::Ring => control.ring(KNOWN_NUMBER),
                    Scene::RingUnknown => control.ring(UNKNOWN_NUMBER),
                    Scene::Dialing => handle.dial("+33612345678"),
                    Scene::Active => {
                        control.ring(KNOWN_NUMBER);
                        std::thread::sleep(Duration::from_millis(1500));
                        handle.answer("/demo/call1");
                    }
                    Scene::Text => {
                        control.text(KNOWN_NUMBER, "Table is ready, come whenever you like")
                    }
                    Scene::Denied | Scene::Offline => {}
                }
            });
        if let Err(e) = spawned {
            log::warn!("cannot start the demo scene: {e}");
        }
    }
}

/// A throwaway data folder, removed when dropped.
pub struct DemoDir {
    path: PathBuf,
}

impl DemoDir {
    pub fn create() -> std::io::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("geekbridge-phone-demo-{}", std::process::id()));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for DemoDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenes_parse_by_name() {
        assert_eq!(Scene::parse("ring"), Some(Scene::Ring));
        assert_eq!(Scene::parse(" Active "), Some(Scene::Active));
        assert_eq!(Scene::parse("ring-unknown"), Some(Scene::RingUnknown));
        assert_eq!(Scene::parse("nope"), None);
    }

    #[test]
    fn the_throwaway_folder_goes_away() {
        let path;
        {
            let d = DemoDir::create().unwrap();
            path = d.path().to_path_buf();
            assert!(path.is_dir());
        }
        assert!(!path.exists());
    }
}

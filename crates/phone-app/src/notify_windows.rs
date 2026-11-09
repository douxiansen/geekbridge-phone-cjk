// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows toast notifications for calls and texts, through PowerShell's access to the Windows
//! notification API (no extra dependencies, no installer identity needed).
//!
//! Toasts here have no buttons: answering and declining are done in the app window. A toast
//! click opens nothing yet. Each alert is tagged, so a ringing toast is removed when the call
//! ends.

use crate::notify::{
    CallEndedNotice, CallNotice, CallUpdate, Notifier, NotifierHost, NotifyId, TextNotice,
};
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

/// Windows PowerShell's own app id. Toasts need a registered app id; this one always exists.
const APP_ID: &str =
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct WindowsNotifier {
    _host: NotifierHost,
}

impl WindowsNotifier {
    pub fn new(host: NotifierHost) -> Self {
        Self { _host: host }
    }
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Inside a PowerShell single-quoted string.
fn ps(s: &str) -> String {
    s.replace('\'', "''")
}

fn tag(id: &NotifyId) -> String {
    match id {
        NotifyId::Call(c) => format!("call-{c}"),
        NotifyId::Text(t) => format!("text-{t}"),
    }
    .chars()
    .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
    .take(60)
    .collect()
}

/// Runs a PowerShell script in the background, fed through stdin so no quoting survives a
/// command line.
fn run(script: String) {
    std::thread::spawn(move || {
        let child = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "-",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match child {
            Ok(mut c) => {
                if let Some(mut stdin) = c.stdin.take() {
                    let _ = stdin.write_all(script.as_bytes());
                }
                let _ = c.wait();
            }
            Err(e) => log::warn!("could not start PowerShell for a notification: {e}"),
        }
    });
}

fn show(id: &NotifyId, title: &str, body: &str, incoming_call: bool) {
    let scenario = if incoming_call {
        " scenario=\"incomingCall\""
    } else {
        ""
    };
    let toast = format!(
        "<toast{scenario}><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        xml(title),
        xml(body)
    );
    run(format!(
        "$null = [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]\n\
         $null = [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime]\n\
         $x = New-Object Windows.Data.Xml.Dom.XmlDocument\n\
         $x.LoadXml('{}')\n\
         $t = New-Object Windows.UI.Notifications.ToastNotification $x\n\
         $t.Tag = '{}'\n\
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{}').Show($t)\n",
        ps(&toast),
        ps(&tag(id)),
        ps(APP_ID)
    ));
}

fn who(name: &Option<String>, number: &str) -> String {
    match name {
        Some(n) if !n.is_empty() => n.clone(),
        _ if number.is_empty() => "Unknown number".to_string(),
        _ => number.to_string(),
    }
}

impl Notifier for WindowsNotifier {
    fn incoming_call(&self, c: &CallNotice) {
        show(&c.id(), "Incoming call", &who(&c.name, &c.number), true);
    }

    fn call_updated(&self, c: &CallUpdate) {
        // The ringing toast is no longer true once the call is answered or being placed.
        self.clear(&c.id());
    }

    fn call_ended(&self, c: &CallEndedNotice) {
        if c.missed {
            show(
                &c.id_for_missed(),
                "Missed call",
                &who(&c.name, &c.number),
                false,
            );
        }
    }

    fn text(&self, t: &TextNotice) {
        show(&t.id(), &who(&t.name, &t.number), &t.body, false);
    }

    fn clear(&self, id: &NotifyId) {
        run(format!(
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null\n\
             [Windows.UI.Notifications.ToastNotificationManager]::History.Remove('{}', '', '{}')\n",
            ps(&tag(id)),
            ps(APP_ID)
        ));
    }
}

impl CallEndedNotice {
    /// A missed-call toast has its own tag, so clearing the ringing one cannot remove it.
    fn id_for_missed(&self) -> NotifyId {
        NotifyId::Call(format!("missed-{}", self.call_id))
    }
}

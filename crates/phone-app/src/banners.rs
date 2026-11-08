// SPDX-License-Identifier: GPL-3.0-or-later
//! The banner area under the toolbar: sentences from the service (notices) and link problems.

use crate::model::status::{notice_is_redundant, Problem};
use crate::ui::theme::{Palette, S2, S3};
use crate::ui::widgets::{self, BannerAction, Tone};
use crate::updater::{UpdateStatus, Updater};
use eframe::egui::{Frame, Margin, Ui};
use phone_core::service::{Command, Handle};
use std::time::{Duration, Instant};

/// A notice stays this long unless dismissed.
const NOTICE_LIFETIME: Duration = Duration::from_secs(20);
const MAX_NOTICES: usize = 4;

/// Sentences the service wanted the user to see.
#[derive(Debug, Default)]
pub struct Notices {
    items: Vec<(String, Instant)>,
}

impl Notices {
    /// Adds a notice (a repeat of one still showing only refreshes it).
    pub fn push(&mut self, text: String, now: Instant) {
        if let Some(existing) = self.items.iter_mut().find(|(t, _)| *t == text) {
            existing.1 = now;
            return;
        }
        if self.items.len() >= MAX_NOTICES {
            self.items.remove(0);
        }
        self.items.push((text, now));
    }

    /// Drops notices that have been up long enough, and those that repeat an active banner.
    pub fn expire(&mut self, now: Instant, active: &[Problem]) {
        self.items.retain(|(t, at)| {
            now.saturating_duration_since(*at) < NOTICE_LIFETIME && !notice_is_redundant(t, active)
        });
    }

    pub fn dismiss(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(|(t, _)| t.as_str())
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// When the oldest notice will go, for scheduling a repaint.
    pub fn next_expiry(&self, now: Instant) -> Option<Duration> {
        self.items
            .iter()
            .map(|(_, at)| NOTICE_LIFETIME.saturating_sub(now.saturating_duration_since(*at)))
            .min()
    }
}

/// Draws link problems and notices. Does nothing, and takes no space, when there are none.
pub fn show(
    ui: &mut Ui,
    pal: &Palette,
    handle: &Handle,
    problems: &[Problem],
    notices: &mut Notices,
    updater: &Updater,
) {
    let status = updater.status();
    let update_banner = update_banner_wanted(&status, updater);
    if problems.is_empty() && notices.is_empty() && !update_banner {
        return;
    }
    Frame::new()
        .inner_margin(Margin::symmetric(S3 as i8, 0))
        .show(ui, |ui| {
            ui.add_space(S3);
            ui.spacing_mut().item_spacing.y = S2;
            show_update(ui, pal, updater, &status);
            for p in problems {
                let retry = widgets::banner(
                    ui,
                    pal,
                    Tone::Alert,
                    &p.title(),
                    &p.detail(),
                    Some("Retry"),
                    false,
                );
                if retry == Some(BannerAction::Primary) {
                    match p {
                        Problem::MessageAccess | Problem::MessagesFailed(_) => {
                            handle.retry_messages()
                        }
                        Problem::ContactsFailed(_) => handle.send(Command::ReloadContacts),
                    }
                }
            }
            let mut dismiss = None;
            for (i, text) in notices.iter().enumerate() {
                if widgets::banner(ui, pal, Tone::Info, text, "", None, true)
                    == Some(BannerAction::Dismiss)
                {
                    dismiss = Some(i);
                }
            }
            if let Some(i) = dismiss {
                notices.dismiss(i);
            }
        });
}

/// Whether the updater has something to say in a banner.
fn update_banner_wanted(status: &UpdateStatus, updater: &Updater) -> bool {
    match status {
        UpdateStatus::Available(_) => updater.announced().is_some(),
        UpdateStatus::Installing(_) | UpdateStatus::Installed(_) => true,
        UpdateStatus::Failed(_) => {
            updater.last_checked().is_some() && matches!(status, UpdateStatus::Failed(_))
        }
        _ => false,
    }
}

/// The software-update banner: a new version, progress, or a failure.
fn show_update(ui: &mut Ui, pal: &Palette, updater: &Updater, status: &UpdateStatus) {
    match status {
        UpdateStatus::Available(u) if updater.announced().is_some() => {
            let notes = u
                .notes
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('#'))
                .unwrap_or("");
            let detail: String = notes
                .trim_start_matches(['-', '*', ' '])
                .chars()
                .take(110)
                .collect();
            match widgets::banner(
                ui,
                pal,
                Tone::Info,
                &format!("Version {} is available", u.version),
                &detail,
                Some("Update now"),
                true,
            ) {
                Some(BannerAction::Primary) => updater.install(),
                Some(BannerAction::Dismiss) => updater.dismiss(),
                None => {}
            }
        }
        UpdateStatus::Installing(v) => {
            widgets::banner(
                ui,
                pal,
                Tone::Info,
                &format!("Updating to version {v}…"),
                "The app restarts when it is done.",
                None,
                false,
            );
        }
        UpdateStatus::Installed(v) => {
            widgets::banner(
                ui,
                pal,
                Tone::Info,
                &format!("Updated to version {v}"),
                "Restarting…",
                None,
                false,
            );
        }
        // The guard draws the banner and reports a press on Retry.
        UpdateStatus::Failed(e)
            if widgets::banner(
                ui,
                pal,
                Tone::Alert,
                "Could not check for updates",
                e,
                Some("Retry"),
                false,
            ) == Some(BannerAction::Primary) =>
        {
            updater.check_now();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeated_notice_is_one_banner() {
        let mut n = Notices::default();
        let t0 = Instant::now();
        n.push("No phone is connected.".into(), t0);
        n.push("No phone is connected.".into(), t0 + Duration::from_secs(5));
        assert_eq!(n.len(), 1);
    }

    #[test]
    fn notices_expire_and_a_refresh_extends_them() {
        let mut n = Notices::default();
        let t0 = Instant::now();
        n.push("a".into(), t0);
        n.push("b".into(), t0);
        n.push("a".into(), t0 + Duration::from_secs(15));
        n.expire(t0 + Duration::from_secs(25), &[]);
        assert_eq!(n.iter().collect::<Vec<_>>(), vec!["a"]);
        n.expire(t0 + Duration::from_secs(40), &[]);
        assert!(n.is_empty());
    }

    #[test]
    fn the_oldest_makes_room() {
        let mut n = Notices::default();
        let t0 = Instant::now();
        for i in 0..(MAX_NOTICES + 2) {
            n.push(format!("n{i}"), t0);
        }
        assert_eq!(n.len(), MAX_NOTICES);
        assert_eq!(n.iter().next(), Some("n2"));
    }

    #[test]
    fn a_notice_that_repeats_a_banner_is_dropped() {
        let mut n = Notices::default();
        let t0 = Instant::now();
        n.push(
            "Allow message access for this computer on your phone.".into(),
            t0,
        );
        n.push("Something else".into(), t0);
        n.expire(t0, &[Problem::MessageAccess]);
        assert_eq!(n.iter().collect::<Vec<_>>(), vec!["Something else"]);
    }

    #[test]
    fn dismissing_out_of_range_is_harmless() {
        let mut n = Notices::default();
        n.push("a".into(), Instant::now());
        n.dismiss(5);
        assert_eq!(n.len(), 1);
        n.dismiss(0);
        assert!(n.is_empty());
        assert!(n.next_expiry(Instant::now()).is_none());
    }
}

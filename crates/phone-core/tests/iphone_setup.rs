// SPDX-License-Identifier: GPL-3.0-or-later
//! The state an iPhone setup guide reads: an iPhone that has not been set up reports each missing
//! step, and the service notices each one as the user completes it on the phone.
#![cfg(feature = "demo")]

use phone_core::demo::DemoPlatform;
use phone_core::service::{LinkStatus, Options, Service};
use phone_core::store::Paths;
use std::time::{Duration, Instant};

fn eventually(mut f: impl FnMut() -> bool, what: &str) {
    let end = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < end, "never became true: {what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn each_setup_step_is_noticed_as_the_user_completes_it() {
    let root = std::env::temp_dir().join(format!("gbp-iphone-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let platform = DemoPlatform::new_iphone();
    let mut opts = Options::new(Paths::under(&root));
    opts.phones_every = Duration::from_millis(40);
    opts.calls_every = Duration::from_millis(40);
    opts.reconnect_min = Duration::from_millis(60);
    opts.reconnect_max = Duration::from_millis(200);
    opts.denied_retry = Duration::from_millis(100);
    opts.inbox_poll = Some(Duration::from_millis(100));
    let (svc, _rx) = Service::start(platform.clone(), opts);
    let h = svc.handle();

    // It is an Apple phone, the computer's Bluetooth name is known, and nothing is set up yet.
    eventually(
        || {
            h.state()
                .phone
                .as_ref()
                .is_some_and(|p| p.apple && p.connected)
        },
        "iPhone detected",
    );
    assert_eq!(h.state().computer_name.as_deref(), Some("my-laptop"));
    eventually(
        || h.state().messages_status == LinkStatus::AccessDenied,
        "messages refused",
    );
    eventually(
        || matches!(h.state().contacts_status, LinkStatus::Failed(_)),
        "contacts not shared",
    );
    assert!(!h.state().calls_ready);

    // The user turns on Show Notifications on the iPhone.
    platform.allow_messages();
    eventually(
        || h.state().messages_status == LinkStatus::Ready,
        "messages ready",
    );

    // Then Sync Contacts, and presses Check again (Reload) in the guide.
    platform.share_contacts();
    h.send(phone_core::service::Command::ReloadContacts);
    eventually(
        || h.state().contacts_status == LinkStatus::Ready && !h.state().contacts.is_empty(),
        "contacts ready",
    );

    // The iPhone opens its hands-free link.
    platform.connect_hands_free();
    eventually(|| h.state().calls_ready, "calls ready");
    svc.stop();
}

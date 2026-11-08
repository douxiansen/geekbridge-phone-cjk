// SPDX-License-Identifier: GPL-3.0-or-later
//! GeekBridge Phone core. Everything that is not tied to one operating
//! system lives here; each OS supplies a [`platform::Platform`].

pub mod calls;
pub mod contacts;
pub mod conversations;
pub mod platform;
pub mod recents;
pub mod service;
pub mod settings;
pub mod store;
pub mod vcard;

// SPDX-License-Identifier: GPL-3.0-or-later
//! A portable OBEX client with Bluetooth MAP (text messages) on top.
//!
//! Nothing here touches the operating system: the client works over any
//! `Read + Write` byte stream, which on each platform is an RFCOMM socket to
//! the phone. That is what lets one implementation serve Linux, Windows and
//! macOS, none of which share a Bluetooth message API.

pub mod bmessage;
pub mod client;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
pub mod map;
pub mod packet;
pub mod pbap;

pub use client::{ObexClient, ObexError};
pub use map::{MapClient, MessageEntry};
pub use pbap::{Book, PbapClient};

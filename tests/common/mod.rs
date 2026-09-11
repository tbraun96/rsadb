//! Shared test support: an in-process device emulator and helpers.

// Test scaffolding: panics are the failure mode and pedantic style lints add nothing here.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::pedantic
)]

pub mod fake;

use rsadb::HostKey;
use std::sync::OnceLock;

/// One 2048-bit key per test binary (generation is slow in debug builds).
pub fn host_key() -> &'static HostKey {
    static KEY: OnceLock<HostKey> = OnceLock::new();
    KEY.get_or_init(|| HostKey::generate("test@rsadb").unwrap_or_else(|e| panic!("keygen: {e}")))
}

/// A second key the device does not know.
pub fn stranger_key() -> &'static HostKey {
    static KEY: OnceLock<HostKey> = OnceLock::new();
    KEY.get_or_init(|| {
        HostKey::generate("stranger@rsadb").unwrap_or_else(|e| panic!("keygen: {e}"))
    })
}

/// A minimal valid PNG (1x1 transparent pixel).
pub const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

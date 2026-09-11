//! Pure-Rust Android Debug Bridge (ADB) client.
//!
//! `rsadb` speaks the ADB transport protocol directly to `adbd` over USB
//! (via `nusb`) or TCP, with no `adb` binary, no libusb and no C code.

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod error;
pub mod wire;

pub use error::{Error, Result};

//! Pure-Rust Android Debug Bridge (ADB) client.
//!
//! `rsadb` speaks the ADB transport protocol directly to `adbd` over USB
//! (via `nusb`) or TCP, with no `adb` binary, no libusb and no C code.

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod auth;
pub mod channel;
pub mod error;
pub mod session;
pub mod transport;
pub mod wire;

pub use auth::HostKey;
pub use channel::{Channel, Connection};
pub use error::{Error, Result};
pub use session::{Session, Stream};
pub use transport::Transport;

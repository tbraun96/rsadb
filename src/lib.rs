//! Pure-Rust Android Debug Bridge (ADB) client.
//!
//! `rsadb` speaks the ADB transport protocol directly to `adbd` over USB
//! (via [`nusb`]) or TCP, with no `adb` binary, no libusb and no C code.
//! It can also talk to a running Google adb server when something else
//! (Android Studio, say) already holds the USB interface.
//!
//! ```no_run
//! use rsadb::{Device, Session, auth, transport};
//! use std::time::Duration;
//!
//! # async fn demo() -> rsadb::Result<()> {
//! let key = auth::load_or_generate(&auth::default_key_paths()?, "me@laptop")?;
//! let usb = transport::usb::find(None).await?;
//! let session = Session::connect(transport::usb::open(&usb).await?, &key, Duration::from_secs(60)).await?;
//! let device = Device::new(session);
//! println!("{}", device.getprop("ro.product.model").await?);
//! # Ok(()) }
//! ```
//!
//! # Feature flags
//!
//! * `usb` (default) — direct USB transport through `nusb`.
//! * `tcp` (default) — direct TCP transport (`adb connect` targets, emulators).
//! * `host-client` (default) — client for a running Google adb server.
//! * `hardware` — enables ignored integration tests that need a real device.

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod auth;
pub mod channel;
pub mod device;
pub mod error;
#[cfg(feature = "host-client")]
#[cfg_attr(docsrs, doc(cfg(feature = "host-client")))]
pub mod host;
pub mod services;
pub mod session;
#[cfg(feature = "usb")]
#[cfg_attr(docsrs, doc(cfg(feature = "usb")))]
pub mod track;
pub mod transport;
pub mod wire;

pub use auth::HostKey;
pub use channel::{Channel, Connection};
pub use device::Device;
pub use error::{Error, Result};
pub use session::{Session, Stream};
pub use transport::Transport;

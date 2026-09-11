//! Protocol-level clients for the services `adbd` offers on top of streams.
//!
//! These functions take a [`Connection`] and speak
//! the service's own framing; [`crate::Device`] wraps them in a typed API.

pub mod content;
pub mod props;
pub mod shell;
pub mod shell_v2;
pub mod sync;

use crate::channel::{Connection, drain};
use crate::error::Result;

pub use shell_v2::ShellOutput;
pub use sync::{Entry, Stat, StatDetail, SyncClient, SyncFeatures};

/// Reboot targets accepted by the `reboot:` service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebootTarget {
    /// Normal reboot.
    System,
    /// Into the bootloader (fastboot).
    Bootloader,
    /// Into recovery.
    Recovery,
    /// Into sideload mode.
    Sideload,
    /// Into fastbootd (userspace fastboot).
    Fastboot,
}

impl RebootTarget {
    /// The argument sent after `reboot:`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "",
            Self::Bootloader => "bootloader",
            Self::Recovery => "recovery",
            Self::Sideload => "sideload",
            Self::Fastboot => "fastboot",
        }
    }
}

/// Ask the device to reboot. The stream closes as the device goes down.
pub async fn reboot<C: Connection>(conn: &C, target: RebootTarget) -> Result<()> {
    drain(conn.open(&format!("reboot:{}", target.as_str())).await?).await?;
    Ok(())
}

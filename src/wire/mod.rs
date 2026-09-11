//! The ADB wire format: 24-byte little-endian headers followed by an optional payload.
//!
//! See `docs/protocol.md` in the repository for a narrative description.

mod codec;
mod command;

pub use codec::{HEADER_LEN, checksum, decode_header, encode_header};
pub use command::Command;

use bytes::Bytes;

/// Protocol version this crate speaks (`A_VERSION` in AOSP).
pub const VERSION: u32 = 0x0100_0001;
/// Oldest protocol version we accept from a peer.
pub const VERSION_MIN: u32 = 0x0100_0000;
/// Peers at or above this version ignore `data_check`; we skip computing it for them.
pub const VERSION_SKIP_CHECKSUM: u32 = 0x0100_0001;
/// Largest payload we ever offer or accept (1 MiB, `MAX_PAYLOAD` in AOSP).
pub const MAX_PAYLOAD: u32 = 1024 * 1024;
/// Payload limit assumed when a legacy peer sends `arg1 == 0` in its `A_CNXN`.
pub const LEGACY_MAX_PAYLOAD: u32 = 256 * 1024;

/// `A_AUTH` sub-type: device offers a random 20-byte token to sign.
pub const AUTH_TOKEN: u32 = 1;
/// `A_AUTH` sub-type: host returns a PKCS#1 v1.5 signature of the token.
pub const AUTH_SIGNATURE: u32 = 2;
/// `A_AUTH` sub-type: host offers its public key for the user to accept.
pub const AUTH_RSAPUBLICKEY: u32 = 3;

/// A decoded header, before or after the payload has been attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// The command word.
    pub command: Command,
    /// First argument (meaning depends on the command).
    pub arg0: u32,
    /// Second argument (meaning depends on the command).
    pub arg1: u32,
    /// Length of the payload that follows this header.
    pub data_length: u32,
    /// Sum of all payload bytes (ignored by peers at [`VERSION_SKIP_CHECKSUM`] or newer).
    pub data_check: u32,
}

/// A complete message: header plus payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The command word.
    pub command: Command,
    /// First argument.
    pub arg0: u32,
    /// Second argument.
    pub arg1: u32,
    /// The payload (may be empty).
    pub payload: Bytes,
}

impl Message {
    /// Build a message with an arbitrary payload.
    pub fn new(command: Command, arg0: u32, arg1: u32, payload: impl Into<Bytes>) -> Self {
        Self {
            command,
            arg0,
            arg1,
            payload: payload.into(),
        }
    }

    /// Build a message with no payload.
    pub fn empty(command: Command, arg0: u32, arg1: u32) -> Self {
        Self::new(command, arg0, arg1, Bytes::new())
    }

    /// Header for this message, computing the checksum when `with_checksum` is set.
    ///
    /// Returns `None` if the payload is longer than `u32::MAX` bytes.
    pub fn header(&self, with_checksum: bool) -> Option<Header> {
        let data_length = u32::try_from(self.payload.len()).ok()?;
        let data_check = if with_checksum {
            checksum(&self.payload)
        } else {
            0
        };
        Some(Header {
            command: self.command,
            arg0: self.arg0,
            arg1: self.arg1,
            data_length,
            data_check,
        })
    }

    /// Whether this message's checksum matches its payload.
    pub fn checksum_matches(&self, header: &Header) -> bool {
        header.data_check == checksum(&self.payload)
    }
}

impl Header {
    /// Attach a payload, producing a [`Message`].
    pub fn with_payload(self, payload: impl Into<Bytes>) -> Message {
        Message::new(self.command, self.arg0, self.arg1, payload)
    }
}

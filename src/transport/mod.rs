//! Message transports: anything that can carry [`Message`]s to and from a device.
//!
//! A [`Transport`] is used sequentially during the handshake and then split
//! into a [`MessageSink`] and a [`MessageSource`] so the session can write and
//! read concurrently.

mod framed;
#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "usb")]
pub mod usb;

pub use framed::{FramedReader, FramedWriter, StreamTransport};

use crate::error::Result;
use crate::wire::{MAX_PAYLOAD, Message, VERSION_SKIP_CHECKSUM};
use std::future::Future;

/// Per-connection framing policy, negotiated by the `A_CNXN` exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireConfig {
    /// Fill `data_check` on outgoing messages.
    pub compute_checksum: bool,
    /// Reject incoming messages whose `data_check` does not match.
    pub verify_checksum: bool,
    /// Largest payload we will accept from the peer.
    pub max_payload: u32,
}

impl WireConfig {
    /// Policy before the peer's version is known: checksum on, do not verify.
    pub const INITIAL: Self = Self {
        compute_checksum: true,
        verify_checksum: false,
        max_payload: MAX_PAYLOAD,
    };

    /// Policy after `A_CNXN` for a peer speaking `version` with the given payload limit.
    pub fn negotiated(version: u32, max_payload: u32) -> Self {
        let checksum = version < VERSION_SKIP_CHECKSUM;
        Self {
            compute_checksum: checksum,
            verify_checksum: checksum,
            max_payload,
        }
    }
}

/// The writing half of a transport.
pub trait MessageSink: Send {
    /// Send one message.
    fn send(&mut self, msg: Message) -> impl Future<Output = Result<()>> + Send;
}

/// The reading half of a transport.
pub trait MessageSource: Send {
    /// Receive the next message, or [`crate::Error::Disconnected`] at end of stream.
    fn recv(&mut self) -> impl Future<Output = Result<Message>> + Send;
}

/// A bidirectional message channel to a device.
pub trait Transport: MessageSink + MessageSource + 'static {
    /// The writing half produced by [`Transport::split`].
    type Sink: MessageSink + 'static;
    /// The reading half produced by [`Transport::split`].
    type Source: MessageSource + 'static;

    /// Change the framing policy (the session calls this once `A_CNXN` has been exchanged).
    fn configure(&mut self, config: WireConfig);

    /// Split into independently usable halves.
    fn split(self) -> (Self::Sink, Self::Source);
}

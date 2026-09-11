//! The byte-stream abstraction every service is written against.
//!
//! A [`Channel`] is one ADB stream (an `A_OPEN`ed service). A [`Connection`]
//! can open channels: either a direct [`crate::session::Session`] or a
//! device reached through a running adb server.

use crate::error::{Error, Result};
use bytes::{Bytes, BytesMut};
use std::future::Future;

/// One bidirectional ADB stream.
pub trait Channel: Send {
    /// Next chunk from the device, or `None` once the device closed the stream.
    fn recv(&mut self) -> impl Future<Output = Result<Option<Bytes>>> + Send;

    /// Send bytes to the device (blocking on flow control as required).
    fn send(&mut self, data: Bytes) -> impl Future<Output = Result<()>> + Send;

    /// Close the stream; idempotent.
    fn close(&mut self) -> impl Future<Output = Result<()>> + Send;
}

/// Something that can open ADB service streams.
pub trait Connection: Send + Sync {
    /// The channel type this connection produces.
    type Channel: Channel + 'static;

    /// Open `service` (for example `"shell:id"`).
    fn open(&self, service: &str) -> impl Future<Output = Result<Self::Channel>> + Send;

    /// The feature list the device advertised (for example `shell_v2`).
    fn features(&self) -> impl Future<Output = Result<Vec<String>>> + Send;
}

impl<C: Connection> Connection for &C {
    type Channel = C::Channel;

    fn open(&self, service: &str) -> impl Future<Output = Result<Self::Channel>> + Send {
        C::open(self, service)
    }

    fn features(&self) -> impl Future<Output = Result<Vec<String>>> + Send {
        C::features(self)
    }
}

impl<C: Connection> Connection for std::sync::Arc<C> {
    type Channel = C::Channel;

    fn open(&self, service: &str) -> impl Future<Output = Result<Self::Channel>> + Send {
        C::open(self, service)
    }

    fn features(&self) -> impl Future<Output = Result<Vec<String>>> + Send {
        C::features(self)
    }
}

/// Buffered reading helpers over a [`Channel`].
#[derive(Debug)]
pub struct ChannelReader<C> {
    channel: C,
    buffer: BytesMut,
}

impl<C: Channel> ChannelReader<C> {
    /// Wrap `channel`.
    pub fn new(channel: C) -> Self {
        Self {
            channel,
            buffer: BytesMut::new(),
        }
    }

    /// The wrapped channel, for writes.
    pub fn channel(&mut self) -> &mut C {
        &mut self.channel
    }

    /// Consume the reader, returning the channel and any unread bytes.
    pub fn into_parts(self) -> (C, Bytes) {
        (self.channel, self.buffer.freeze())
    }

    /// Read exactly `len` bytes; end-of-stream before that is [`Error::StreamClosed`].
    pub async fn read_exact(&mut self, len: usize) -> Result<Bytes> {
        while self.buffer.len() < len {
            match self.channel.recv().await? {
                Some(chunk) => self.buffer.extend_from_slice(&chunk),
                None => return Err(Error::StreamClosed),
            }
        }
        Ok(self.buffer.split_to(len).freeze())
    }

    /// Read a little-endian `u32`.
    pub async fn read_u32(&mut self) -> Result<u32> {
        let b = self.read_exact(4).await?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Next chunk, honouring buffered bytes first; `None` at end of stream.
    pub async fn next_chunk(&mut self) -> Result<Option<Bytes>> {
        if !self.buffer.is_empty() {
            return Ok(Some(self.buffer.split().freeze()));
        }
        self.channel.recv().await
    }

    /// Everything until the device closes the stream.
    pub async fn read_to_end(&mut self) -> Result<Bytes> {
        while let Some(chunk) = self.channel.recv().await? {
            self.buffer.extend_from_slice(&chunk);
        }
        Ok(self.buffer.split().freeze())
    }
}

/// Read a whole stream to the end, then close it.
pub async fn drain<C: Channel>(channel: C) -> Result<Bytes> {
    let mut reader = ChannelReader::new(channel);
    let out = reader.read_to_end().await?;
    reader.channel().close().await?;
    Ok(out)
}

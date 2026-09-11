//! A service stream carried by one TCP connection to the adb server.

use crate::channel::Channel;
use crate::error::Result;
use bytes::{Bytes, BytesMut};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

/// Read size per `recv` call.
const CHUNK: usize = 64 * 1024;

/// A [`Channel`] over a TCP connection the adb server has bound to a service.
#[derive(Debug)]
pub struct HostChannel {
    stream: TcpStream,
    closed: bool,
}

impl HostChannel {
    pub(super) fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            closed: false,
        }
    }
}

impl Channel for HostChannel {
    async fn recv(&mut self) -> Result<Option<Bytes>> {
        let mut buf = BytesMut::with_capacity(CHUNK);
        match self.stream.read_buf(&mut buf).await {
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some(buf.freeze())),
            // The server hung up after its reply; everything it sent was
            // already delivered, so treat the reset as end of stream.
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn send(&mut self, data: Bytes) -> Result<()> {
        self.stream.write_all(&data).await?;
        self.stream.flush().await?;
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        if !self.closed {
            self.closed = true;
            // A peer that already hung up is not an error for a close.
            let _ = self.stream.shutdown().await;
        }
        Ok(())
    }
}

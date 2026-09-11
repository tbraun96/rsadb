//! Bulk-endpoint I/O for the USB transport.
//!
//! Header and payload travel as separate transfers, mirroring what `adbd`
//! does on the device. A payload that is an exact multiple of the endpoint's
//! maximum packet size is followed by a zero-length packet so the peer's read
//! terminates; zero-length packets received while waiting for a header are
//! skipped for the same reason.

use crate::error::{Error, Result};
use crate::transport::{MessageSink, MessageSource, Transport, WireConfig};
use crate::wire::{HEADER_LEN, Message, decode_header, encode_header};
use nusb::transfer::{Buffer, Bulk, Completion, In, Out, TransferError};
use nusb::{Device, Endpoint, Interface};

/// The writing half: the bulk OUT endpoint.
pub struct UsbSink {
    endpoint: Endpoint<Bulk, Out>,
    config: WireConfig,
    _interface: Interface,
    _device: Device,
}

/// The reading half: the bulk IN endpoint.
pub struct UsbSource {
    endpoint: Endpoint<Bulk, In>,
    config: WireConfig,
}

/// A claimed ADB USB interface.
pub struct UsbTransport {
    sink: UsbSink,
    source: UsbSource,
}

impl std::fmt::Debug for UsbTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UsbTransport")
            .field("in", &self.source.endpoint.endpoint_address())
            .field("out", &self.sink.endpoint.endpoint_address())
            .finish()
    }
}

impl UsbTransport {
    pub(super) fn new(
        device: Device,
        interface: Interface,
        bulk_in: Endpoint<Bulk, In>,
        bulk_out: Endpoint<Bulk, Out>,
    ) -> Self {
        Self {
            sink: UsbSink {
                endpoint: bulk_out,
                config: WireConfig::INITIAL,
                _interface: interface,
                _device: device,
            },
            source: UsbSource {
                endpoint: bulk_in,
                config: WireConfig::INITIAL,
            },
        }
    }
}

fn map_transfer(e: TransferError) -> Error {
    match e {
        TransferError::Disconnected => Error::Disconnected,
        other => Error::Usb(other.to_string()),
    }
}

fn completed(completion: Completion) -> Result<Buffer> {
    completion.into_result().map_err(map_transfer)
}

impl UsbSink {
    async fn write(&mut self, data: &[u8]) -> Result<()> {
        let mut buffer = self.endpoint.allocate(data.len());
        buffer.extend_from_slice(data);
        self.endpoint.submit(buffer);
        completed(self.endpoint.next_complete().await)?;
        Ok(())
    }
}

impl MessageSink for UsbSink {
    async fn send(&mut self, msg: Message) -> Result<()> {
        let header = msg
            .header(self.config.compute_checksum)
            .ok_or_else(|| Error::InvalidArgument("payload longer than u32::MAX".into()))?;
        self.write(&encode_header(&header)).await?;
        if msg.payload.is_empty() {
            return Ok(());
        }
        self.write(&msg.payload).await?;
        if msg.payload.len() % self.endpoint.max_packet_size() == 0 {
            self.write(&[]).await?;
        }
        Ok(())
    }
}

impl UsbSource {
    /// Read up to `len` bytes (rounded up to whole packets, as the OS requires).
    async fn read(&mut self, len: usize) -> Result<Buffer> {
        let mps = self.endpoint.max_packet_size().max(1);
        let request = len.div_ceil(mps).max(1) * mps;
        self.endpoint.submit(self.endpoint.allocate(request));
        completed(self.endpoint.next_complete().await)
    }

    async fn read_exact(&mut self, len: usize) -> Result<Buffer> {
        let mut buffer = self.read(len).await?;
        while buffer.len() < len {
            let more = self.read(len - buffer.len()).await?;
            if more.is_empty() {
                return Err(Error::protocol(format!(
                    "short payload: {} of {len} bytes",
                    buffer.len()
                )));
            }
            buffer.extend_from_slice(&more);
        }
        if buffer.len() != len {
            return Err(Error::protocol(format!(
                "expected {len} bytes, got {}",
                buffer.len()
            )));
        }
        Ok(buffer)
    }
}

impl MessageSource for UsbSource {
    async fn recv(&mut self) -> Result<Message> {
        let head = loop {
            let buffer = self.read(HEADER_LEN).await?;
            if !buffer.is_empty() {
                break buffer;
            }
        };
        let raw: [u8; HEADER_LEN] = head[..]
            .try_into()
            .map_err(|_| Error::protocol(format!("truncated header: {} bytes", head.len())))?;
        let header = decode_header(&raw, self.config.max_payload)?;
        let len = header.data_length as usize;
        let payload = if len == 0 {
            Vec::new()
        } else {
            self.read_exact(len).await?.into_vec()
        };
        let msg = header.with_payload(payload);
        if self.config.verify_checksum && !msg.checksum_matches(&header) {
            return Err(Error::protocol(format!(
                "checksum mismatch on {}",
                header.command
            )));
        }
        Ok(msg)
    }
}

impl MessageSink for UsbTransport {
    fn send(&mut self, msg: Message) -> impl Future<Output = Result<()>> + Send {
        self.sink.send(msg)
    }
}

impl MessageSource for UsbTransport {
    fn recv(&mut self) -> impl Future<Output = Result<Message>> + Send {
        self.source.recv()
    }
}

impl Transport for UsbTransport {
    type Sink = UsbSink;
    type Source = UsbSource;

    fn configure(&mut self, config: WireConfig) {
        self.sink.config = config;
        self.source.config = config;
    }

    fn split(self) -> (Self::Sink, Self::Source) {
        (self.sink, self.source)
    }
}

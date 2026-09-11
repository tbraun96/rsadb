//! Header/payload framing over any byte stream (TCP, in-memory duplex pipes, …).

use super::{MessageSink, MessageSource, Transport, WireConfig};
use crate::error::{Error, Result};
use crate::wire::{HEADER_LEN, Message, decode_header, encode_header};
use bytes::BytesMut;
use std::io::ErrorKind;
use tokio::io::{
    AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, ReadHalf, WriteHalf,
};

/// Writes messages to an [`AsyncWrite`].
#[derive(Debug)]
pub struct FramedWriter<W> {
    inner: W,
    config: WireConfig,
}

impl<W: AsyncWrite + Unpin + Send> FramedWriter<W> {
    /// Wrap a writer with the given framing policy.
    pub fn new(inner: W, config: WireConfig) -> Self {
        Self { inner, config }
    }

    /// Replace the framing policy.
    pub fn configure(&mut self, config: WireConfig) {
        self.config = config;
    }

    /// Consume the writer, returning the underlying stream.
    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: AsyncWrite + Unpin + Send> MessageSink for FramedWriter<W> {
    async fn send(&mut self, msg: Message) -> Result<()> {
        let header = msg
            .header(self.config.compute_checksum)
            .ok_or_else(|| Error::InvalidArgument("payload longer than u32::MAX".into()))?;
        self.inner.write_all(&encode_header(&header)).await?;
        if !msg.payload.is_empty() {
            self.inner.write_all(&msg.payload).await?;
        }
        self.inner.flush().await?;
        Ok(())
    }
}

/// Reads messages from an [`AsyncRead`].
#[derive(Debug)]
pub struct FramedReader<R> {
    inner: R,
    config: WireConfig,
}

impl<R: AsyncRead + Unpin + Send> FramedReader<R> {
    /// Wrap a reader with the given framing policy.
    pub fn new(inner: R, config: WireConfig) -> Self {
        Self { inner, config }
    }

    /// Replace the framing policy.
    pub fn configure(&mut self, config: WireConfig) {
        self.config = config;
    }

    /// Consume the reader, returning the underlying stream.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: AsyncRead + Unpin + Send> MessageSource for FramedReader<R> {
    async fn recv(&mut self) -> Result<Message> {
        let mut head = [0u8; HEADER_LEN];
        match self.inner.read(&mut head).await? {
            0 => return Err(Error::Disconnected),
            n if n < HEADER_LEN => {
                self.inner
                    .read_exact(&mut head[n..])
                    .await
                    .map_err(truncated("header"))?;
            }
            _ => {}
        }
        let header = decode_header(&head, self.config.max_payload)?;
        let len = header.data_length as usize;
        let mut payload = BytesMut::zeroed(len);
        if len > 0 {
            self.inner
                .read_exact(&mut payload)
                .await
                .map_err(truncated("payload"))?;
        }
        let msg = header.with_payload(payload.freeze());
        if self.config.verify_checksum && !msg.checksum_matches(&header) {
            return Err(Error::protocol(format!(
                "checksum mismatch on {}",
                header.command
            )));
        }
        Ok(msg)
    }
}

fn truncated(what: &'static str) -> impl Fn(std::io::Error) -> Error {
    move |e| {
        if e.kind() == ErrorKind::UnexpectedEof {
            Error::protocol(format!("truncated {what}"))
        } else {
            Error::Io(e)
        }
    }
}

/// A [`Transport`] over one bidirectional byte stream.
#[derive(Debug)]
pub struct StreamTransport<S> {
    writer: FramedWriter<WriteHalf<S>>,
    reader: FramedReader<ReadHalf<S>>,
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> StreamTransport<S> {
    /// Frame `stream` with the initial policy.
    pub fn new(stream: S) -> Self {
        let (read, write) = tokio::io::split(stream);
        Self {
            writer: FramedWriter::new(write, WireConfig::INITIAL),
            reader: FramedReader::new(read, WireConfig::INITIAL),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> MessageSink for StreamTransport<S> {
    fn send(&mut self, msg: Message) -> impl Future<Output = Result<()>> + Send {
        self.writer.send(msg)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> MessageSource for StreamTransport<S> {
    fn recv(&mut self) -> impl Future<Output = Result<Message>> + Send {
        self.reader.recv()
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> Transport for StreamTransport<S> {
    type Sink = FramedWriter<WriteHalf<S>>;
    type Source = FramedReader<ReadHalf<S>>;

    fn configure(&mut self, config: WireConfig) {
        self.writer.configure(config);
        self.reader.configure(config);
    }

    fn split(self) -> (Self::Sink, Self::Source) {
        (self.writer, self.reader)
    }
}

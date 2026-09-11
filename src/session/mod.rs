//! A direct session with `adbd`: handshake, then multiplexed streams.

mod banner;
mod handshake;
mod reader;
mod registry;
mod stream;

pub use banner::{Banner, HOST_FEATURES};
pub use handshake::{Negotiated, handshake};
pub use stream::Stream;

use crate::auth::HostKey;
use crate::channel::Connection;
use crate::error::{Error, Result};
use crate::transport::Transport;
use crate::wire::{Command, Message};
use registry::Registry;
use std::sync::Arc;
use std::time::Duration;
use stream::Outbox;
use tokio::sync::mpsc::unbounded_channel;
use tokio::task::JoinHandle;

/// An authenticated connection to one device.
///
/// Dropping the session stops its background tasks; streams opened from it
/// then report [`Error::StreamClosed`].
pub struct Session {
    negotiated: Negotiated,
    registry: Arc<Registry>,
    outbox: Outbox,
    reader: JoinHandle<Error>,
    writer: JoinHandle<()>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("banner", &self.negotiated.banner)
            .field("max_payload", &self.negotiated.max_payload)
            .field("streams", &self.registry.len())
            .finish_non_exhaustive()
    }
}

impl Session {
    /// Perform the handshake over `transport` and start the session.
    ///
    /// `auth_wait` is how long to wait for the user to accept our key on the
    /// device after it was offered; [`Error::Unauthorized`] is returned on expiry.
    pub async fn connect<T: Transport>(
        mut transport: T,
        key: &HostKey,
        auth_wait: Duration,
    ) -> Result<Self> {
        let negotiated = handshake(&mut transport, key, auth_wait).await?;
        Ok(Self::start(transport, negotiated))
    }

    /// Start a session on a transport whose handshake already completed.
    pub fn start<T: Transport>(transport: T, negotiated: Negotiated) -> Self {
        let (sink, source) = transport.split();
        let registry = Arc::new(Registry::new());
        let (outbox, outbox_rx) = unbounded_channel();
        let reader = tokio::spawn(reader::read_loop(
            source,
            Arc::clone(&registry),
            outbox.clone(),
        ));
        let writer = tokio::spawn(reader::write_loop(sink, outbox_rx));
        Self {
            negotiated,
            registry,
            outbox,
            reader,
            writer,
        }
    }

    /// The device's banner.
    pub fn banner(&self) -> &Banner {
        &self.negotiated.banner
    }

    /// Everything the handshake agreed on.
    pub fn negotiated(&self) -> &Negotiated {
        &self.negotiated
    }

    /// Largest payload either side may send.
    pub fn max_payload(&self) -> u32 {
        self.negotiated.max_payload
    }

    /// Open a stream to `service`; the payload is the service name plus a NUL.
    pub async fn open_stream(&self, service: &str) -> Result<Stream> {
        if service.contains('\0') {
            return Err(Error::InvalidArgument("service name contains NUL".into()));
        }
        let (state, data_rx) = self.registry.register();
        let mut payload = service.as_bytes().to_vec();
        payload.push(0);
        self.outbox
            .send(Message::new(Command::Open, state.local_id, 0, payload))
            .map_err(|_| Error::Disconnected)?;
        // A refusal is `A_CLSE` without any `A_OKAY`; an `A_OKAY` followed at
        // once by `A_CLSE` is a stream that opened and finished (e.g. `reboot:`).
        if !state.wait_ack(0).await || state.remote().is_none() {
            self.registry.remove(state.local_id);
            return Err(Error::ServiceRefused {
                service: service.to_owned(),
            });
        }
        Ok(Stream::new(
            state,
            data_rx,
            self.outbox.clone(),
            Arc::clone(&self.registry),
            self.max_payload(),
        ))
    }

    /// Whether the background tasks are still alive.
    pub fn is_alive(&self) -> bool {
        !self.reader.is_finished() && !self.writer.is_finished()
    }

    /// Stop the session, closing every stream.
    pub fn close(&self) {
        self.registry.close_all();
        self.reader.abort();
        self.writer.abort();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

impl Connection for Session {
    type Channel = Stream;

    fn open(&self, service: &str) -> impl Future<Output = Result<Stream>> + Send {
        self.open_stream(service)
    }

    fn features(&self) -> impl Future<Output = Result<Vec<String>>> + Send {
        std::future::ready(Ok(self.negotiated.banner.features.clone()))
    }
}

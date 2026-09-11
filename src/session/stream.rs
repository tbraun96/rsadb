//! One multiplexed ADB stream.

use super::registry::{Registry, StreamState};
use crate::channel::Channel;
use crate::error::{Error, Result};
use crate::wire::{Command, Message};
use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Messages queued for the writer task.
pub(super) type Outbox = UnboundedSender<Message>;

/// An open service stream on a [`super::Session`].
///
/// Writes obey ADB flow control: each `A_WRTE` waits for the device's
/// `A_OKAY` before the next one leaves. Reads acknowledge each received
/// `A_WRTE` once the caller has taken the chunk, so the device never has more
/// than one payload in flight towards us.
pub struct Stream {
    state: Arc<StreamState>,
    data_rx: UnboundedReceiver<Bytes>,
    outbox: Outbox,
    registry: Arc<Registry>,
    max_payload: usize,
}

impl std::fmt::Debug for Stream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stream")
            .field("local_id", &self.state.local_id)
            .field("remote_id", &self.state.remote())
            .field("closed", &self.state.is_closed())
            .finish_non_exhaustive()
    }
}

impl Stream {
    pub(super) fn new(
        state: Arc<StreamState>,
        data_rx: UnboundedReceiver<Bytes>,
        outbox: Outbox,
        registry: Arc<Registry>,
        max_payload: u32,
    ) -> Self {
        Self {
            state,
            data_rx,
            outbox,
            registry,
            max_payload: max_payload as usize,
        }
    }

    /// Our id for this stream.
    pub fn local_id(&self) -> u32 {
        self.state.local_id
    }

    /// The device's id for this stream.
    pub fn remote_id(&self) -> Option<u32> {
        self.state.remote()
    }

    fn ids(&self) -> Result<(u32, u32)> {
        if self.state.is_closed() {
            return Err(Error::StreamClosed);
        }
        let remote = self.state.remote().ok_or(Error::StreamClosed)?;
        Ok((self.state.local_id, remote))
    }

    fn post(&self, msg: Message) -> Result<()> {
        self.outbox.send(msg).map_err(|_| Error::Disconnected)
    }

    async fn write_chunk(&mut self, chunk: Bytes) -> Result<()> {
        let (local, remote) = self.ids()?;
        let seen = self.state.acks();
        self.post(Message::new(Command::Write, local, remote, chunk))?;
        if self.state.wait_ack(seen).await {
            Ok(())
        } else {
            Err(Error::StreamClosed)
        }
    }

    fn close_now(&mut self) {
        if self.state.is_closed() {
            return;
        }
        let remote = self.state.remote().unwrap_or(0);
        self.state.mark_closed();
        self.registry.remove(self.state.local_id);
        // Nothing to do if the writer is gone: the transport is already down.
        let _ = self
            .outbox
            .send(Message::empty(Command::Close, self.state.local_id, remote));
    }
}

impl Channel for Stream {
    async fn recv(&mut self) -> Result<Option<Bytes>> {
        let Some(chunk) = self.data_rx.recv().await else {
            return Ok(None);
        };
        if let Ok((local, remote)) = self.ids() {
            self.post(Message::empty(Command::Okay, local, remote))?;
        }
        Ok(Some(chunk))
    }

    async fn send(&mut self, data: Bytes) -> Result<()> {
        let mut rest = data;
        while !rest.is_empty() {
            let take = rest.len().min(self.max_payload);
            let chunk = rest.split_to(take);
            self.write_chunk(chunk).await?;
        }
        Ok(())
    }

    fn close(&mut self) -> impl Future<Output = Result<()>> + Send {
        self.close_now();
        std::future::ready(Ok(()))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.close_now();
    }
}

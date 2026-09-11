//! The demultiplexing reader task and the serialising writer task.

use super::registry::Registry;
use super::stream::Outbox;
use crate::error::{Error, Result};
use crate::transport::{MessageSink, MessageSource};
use crate::wire::{Command, Message};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{debug, trace, warn};

/// Pull messages off the transport and route them to streams until it fails.
pub(super) async fn read_loop<S: MessageSource>(
    mut source: S,
    registry: Arc<Registry>,
    outbox: Outbox,
) -> Error {
    let err = loop {
        match source.recv().await {
            Ok(msg) => {
                if let Err(e) = dispatch(&registry, &outbox, msg) {
                    break e;
                }
            }
            Err(e) => break e,
        }
    };
    debug!(error = %err, "session reader stopped");
    registry.close_all();
    err
}

fn dispatch(registry: &Registry, outbox: &Outbox, msg: Message) -> Result<()> {
    trace!(command = %msg.command, arg0 = msg.arg0, arg1 = msg.arg1, len = msg.payload.len(), "recv");
    let remote_id = msg.arg0;
    let local_id = msg.arg1;
    match msg.command {
        Command::Okay => {
            if let Some(stream) = registry.get(local_id) {
                stream.ack(remote_id);
            } else {
                debug!(local_id, "OKAY for unknown stream");
            }
        }
        Command::Write => match registry.get(local_id) {
            Some(stream) if stream.deliver(msg.payload) => {}
            _ => {
                // Nobody is listening: tell the device so it stops sending.
                debug!(local_id, "WRTE for closed stream");
                outbox
                    .send(Message::empty(Command::Close, local_id, remote_id))
                    .map_err(|_| Error::Disconnected)?;
            }
        },
        Command::Close => {
            if let Some(stream) = registry.remove(local_id) {
                stream.mark_closed();
            }
        }
        Command::StartTls => {
            return Err(Error::Unsupported(
                "device requested A_STLS mid-session".into(),
            ));
        }
        Command::Connect | Command::Auth | Command::Sync => {
            warn!(command = %msg.command, "ignoring handshake message on an established session");
        }
        Command::Open => {
            return Err(Error::protocol(
                "device tried to open a stream towards the host",
            ));
        }
    }
    Ok(())
}

/// Serialise queued messages onto the transport.
pub(super) async fn write_loop<S: MessageSink>(mut sink: S, mut rx: UnboundedReceiver<Message>) {
    while let Some(msg) = rx.recv().await {
        trace!(command = %msg.command, arg0 = msg.arg0, arg1 = msg.arg1, len = msg.payload.len(), "send");
        if let Err(e) = sink.send(msg).await {
            debug!(error = %e, "session writer stopped");
            rx.close();
            return;
        }
    }
}

//! Bookkeeping shared between the reader task and every open [`super::Stream`].

use bytes::Bytes;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// Per-stream state touched from both the reader task and the owner of the stream.
pub(super) struct StreamState {
    /// Our id for this stream (never zero).
    pub local_id: u32,
    /// The device's id, zero until the first `A_OKAY` arrives.
    pub remote_id: AtomicU32,
    /// Set when either side closed the stream.
    pub closed: AtomicBool,
    /// Number of `A_OKAY`s received (the first one carries the remote id).
    acks: AtomicU64,
    /// Signalled on every `A_OKAY` and on close; waiters re-check state after each wake.
    pub wake: Notify,
    /// Incoming payloads; `None` once the device closed the stream.
    data_tx: Mutex<Option<UnboundedSender<Bytes>>>,
}

impl StreamState {
    /// Whether the stream is closed.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Total acknowledgements so far.
    pub fn acks(&self) -> u64 {
        self.acks.load(Ordering::Acquire)
    }

    /// Record an `A_OKAY` (adopting `remote_id` if this is the first) and wake the owner.
    pub fn ack(&self, remote_id: u32) {
        if self.remote().is_none() {
            self.remote_id.store(remote_id, Ordering::Release);
        }
        self.acks.fetch_add(1, Ordering::AcqRel);
        self.wake.notify_one();
    }

    /// Wait until `acks()` exceeds `seen`; `false` if the stream closed first.
    pub async fn wait_ack(&self, seen: u64) -> bool {
        loop {
            if self.acks() > seen {
                return true;
            }
            if self.is_closed() {
                return false;
            }
            self.wake.notified().await;
        }
    }

    /// Remote id, or `None` while the open is pending.
    pub fn remote(&self) -> Option<u32> {
        match self.remote_id.load(Ordering::Acquire) {
            0 => None,
            id => Some(id),
        }
    }

    /// Deliver a payload from the device; returns `false` when nobody listens any more.
    pub fn deliver(&self, data: Bytes) -> bool {
        let guard = lock(&self.data_tx);
        guard.as_ref().is_some_and(|tx| tx.send(data).is_ok())
    }

    /// Mark closed and wake any waiter; idempotent.
    pub fn mark_closed(&self) {
        self.closed.store(true, Ordering::Release);
        lock(&self.data_tx).take();
        self.wake.notify_one();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// All streams of one session, keyed by local id.
#[derive(Default)]
pub(super) struct Registry {
    streams: Mutex<HashMap<u32, Arc<StreamState>>>,
    next_id: AtomicU32,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            streams: Mutex::new(HashMap::new()),
            next_id: AtomicU32::new(1),
        }
    }

    /// Allocate a local id and register a fresh stream.
    pub fn register(&self) -> (Arc<StreamState>, UnboundedReceiver<Bytes>) {
        let (tx, rx) = unbounded_channel();
        let mut local_id = self.next_id.fetch_add(1, Ordering::AcqRel);
        if local_id == 0 {
            local_id = self.next_id.fetch_add(1, Ordering::AcqRel);
        }
        let state = Arc::new(StreamState {
            local_id,
            remote_id: AtomicU32::new(0),
            closed: AtomicBool::new(false),
            acks: AtomicU64::new(0),
            wake: Notify::new(),
            data_tx: Mutex::new(Some(tx)),
        });
        lock(&self.streams).insert(local_id, Arc::clone(&state));
        (state, rx)
    }

    pub fn get(&self, local_id: u32) -> Option<Arc<StreamState>> {
        lock(&self.streams).get(&local_id).cloned()
    }

    pub fn remove(&self, local_id: u32) -> Option<Arc<StreamState>> {
        lock(&self.streams).remove(&local_id)
    }

    /// Close every stream (transport gone).
    pub fn close_all(&self) {
        let all: Vec<_> = lock(&self.streams).drain().map(|(_, s)| s).collect();
        for state in all {
            state.mark_closed();
        }
    }

    /// Number of live streams.
    pub fn len(&self) -> usize {
        lock(&self.streams).len()
    }
}

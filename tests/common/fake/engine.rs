//! The fake device's protocol engine: handshake, then stream multiplexing.

use super::Shared;
use super::services::{self, ServiceHandler};
use bytes::Bytes;
use rand::RngCore as _;
use rsadb::auth::{pubkey, verify_token};
use rsadb::transport::{Transport, WireConfig};
use rsadb::wire::{
    AUTH_RSAPUBLICKEY, AUTH_SIGNATURE, AUTH_TOKEN, Command, LEGACY_MAX_PAYLOAD, Message,
};
use rsadb::{Error, Result};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

struct DeviceStream {
    host_id: u32,
    handler: Box<dyn ServiceHandler>,
    queue: VecDeque<Bytes>,
    in_flight: bool,
}

pub async fn run<T: Transport>(shared: Arc<Shared>, mut transport: T) -> Result<()> {
    transport.configure(WireConfig::INITIAL);
    let negotiated = handshake(&shared, &mut transport).await?;
    if !negotiated {
        return Ok(());
    }
    let config = &shared.config;
    let limit = if config.max_payload == 0 {
        LEGACY_MAX_PAYLOAD
    } else {
        config.max_payload
    };
    transport.configure(WireConfig::negotiated(config.version, limit));
    let max_payload = limit as usize;
    let mut streams: HashMap<u32, DeviceStream> = HashMap::new();
    let mut next_id = 100u32;
    loop {
        let msg = match transport.recv().await {
            Ok(msg) => msg,
            Err(Error::Disconnected) => return Ok(()),
            Err(e) => return Err(e),
        };
        shared.received.lock().unwrap().push(msg.clone());
        let (host_id, dev_id) = (msg.arg0, msg.arg1);
        match msg.command {
            Command::Open => {
                let service = String::from_utf8_lossy(&msg.payload)
                    .trim_end_matches('\0')
                    .to_owned();
                match services::open(&shared, &service) {
                    Some(mut handler) => {
                        let id = next_id;
                        next_id += 1;
                        transport
                            .send(Message::empty(Command::Okay, id, host_id))
                            .await?;
                        let queue = chunked(handler.on_open(), max_payload);
                        streams.insert(
                            id,
                            DeviceStream {
                                host_id,
                                handler,
                                queue,
                                in_flight: false,
                            },
                        );
                        pump(&mut transport, &mut streams, id).await?;
                    }
                    None => {
                        transport
                            .send(Message::empty(Command::Close, 0, host_id))
                            .await?
                    }
                }
            }
            Command::Write => {
                if let Some(stream) = streams.get_mut(&dev_id) {
                    transport
                        .send(Message::empty(Command::Okay, dev_id, host_id))
                        .await?;
                    let replies = stream.handler.on_data(&msg.payload);
                    stream.queue.extend(chunked(replies, max_payload));
                    pump(&mut transport, &mut streams, dev_id).await?;
                }
            }
            Command::Okay => {
                if let Some(stream) = streams.get_mut(&dev_id) {
                    stream.in_flight = false;
                    pump(&mut transport, &mut streams, dev_id).await?;
                }
            }
            Command::Close => {
                streams.remove(&dev_id);
            }
            other => return Err(Error::protocol(format!("fake device got {other}"))),
        }
    }
}

fn chunked(replies: Vec<Bytes>, max: usize) -> VecDeque<Bytes> {
    replies
        .into_iter()
        .flat_map(|mut b| {
            let mut parts = Vec::new();
            while b.len() > max {
                parts.push(b.split_to(max));
            }
            parts.push(b);
            parts
        })
        .filter(|b| !b.is_empty())
        .collect()
}

async fn pump<T: Transport>(
    transport: &mut T,
    streams: &mut HashMap<u32, DeviceStream>,
    id: u32,
) -> Result<()> {
    let Some(stream) = streams.get_mut(&id) else {
        return Ok(());
    };
    if stream.in_flight {
        return Ok(());
    }
    if let Some(chunk) = stream.queue.pop_front() {
        stream.in_flight = true;
        return transport
            .send(Message::new(Command::Write, id, stream.host_id, chunk))
            .await;
    }
    if stream.handler.finished() {
        let host_id = stream.host_id;
        streams.remove(&id);
        transport
            .send(Message::empty(Command::Close, id, host_id))
            .await?;
    }
    Ok(())
}

/// Returns `Ok(false)` when the session must not proceed (TLS requested, key never accepted).
async fn handshake<T: Transport>(shared: &Shared, transport: &mut T) -> Result<bool> {
    let config = &shared.config;
    let cnxn = transport.recv().await?;
    shared.received.lock().unwrap().push(cnxn.clone());
    if cnxn.command != Command::Connect {
        return Err(Error::protocol("expected CNXN first"));
    }
    if config.request_tls {
        transport
            .send(Message::empty(Command::StartTls, 1, 0))
            .await?;
        return Ok(false);
    }
    if config.require_auth {
        let mut token = [0u8; 20];
        rand::rngs::OsRng.fill_bytes(&mut token);
        transport
            .send(Message::new(Command::Auth, AUTH_TOKEN, 0, token.to_vec()))
            .await?;
        let sig = transport.recv().await?;
        shared.received.lock().unwrap().push(sig.clone());
        let trusted = sig.command == Command::Auth
            && sig.arg0 == AUTH_SIGNATURE
            && config
                .trusted
                .iter()
                .any(|k| verify_token(k, &token, &sig.payload).is_ok());
        if !trusted {
            rand::rngs::OsRng.fill_bytes(&mut token);
            transport
                .send(Message::new(Command::Auth, AUTH_TOKEN, 0, token.to_vec()))
                .await?;
            let offer = transport.recv().await?;
            shared.received.lock().unwrap().push(offer.clone());
            if offer.command != Command::Auth || offer.arg0 != AUTH_RSAPUBLICKEY {
                return Err(Error::protocol("expected RSAPUBLICKEY"));
            }
            let line = String::from_utf8_lossy(&offer.payload)
                .trim_end_matches('\0')
                .to_owned();
            let key = pubkey::decode_line(&line)?;
            if !config.accept_new_keys {
                // The user never taps "Allow": stay silent until the host gives up.
                let _ = transport.recv().await;
                return Ok(false);
            }
            *shared.accepted_key.lock().unwrap() = Some(key);
        }
    }
    transport
        .send(Message::new(
            Command::Connect,
            config.version,
            config.max_payload,
            config.banner.to_wire(),
        ))
        .await?;
    Ok(true)
}

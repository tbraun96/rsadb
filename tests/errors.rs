//! Failure paths: refused opens, malformed frames, disconnects.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::fake::{self, FakeConfig};
use common::host_key;
use rsadb::Transport as _;
use rsadb::transport::{MessageSink as _, MessageSource as _, StreamTransport, WireConfig};
use rsadb::wire::{Command, HEADER_LEN, MAX_PAYLOAD, Message, encode_header};
use rsadb::{Channel as _, Device, Error, Session};
use std::time::Duration;
use tokio::io::AsyncWriteExt as _;

const WAIT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn refused_service_is_service_refused() {
    let mut config = FakeConfig::trusting(host_key());
    config.refuse_services.push("shell:secret".into());
    let (_fake, transport) = fake::spawn(config);
    let session = Session::connect(transport, host_key(), WAIT).await.unwrap();
    let err = session.open_stream("shell:secret").await.unwrap_err();
    assert!(
        matches!(err, Error::ServiceRefused { ref service } if service == "shell:secret"),
        "{err}"
    );
    let err = session.open_stream("unknown:").await.unwrap_err();
    assert!(matches!(err, Error::ServiceRefused { .. }));
    // The session is still healthy afterwards.
    let out = Device::new(session)
        .shell("echo still alive")
        .await
        .unwrap();
    assert_eq!(out.stdout_text(), "still alive\n");
}

#[tokio::test]
async fn nul_in_service_name_is_rejected_locally() {
    let (_fake, transport) = fake::spawn(FakeConfig::trusting(host_key()));
    let session = Session::connect(transport, host_key(), WAIT).await.unwrap();
    assert!(matches!(
        session.open_stream("shell:a\0b").await,
        Err(Error::InvalidArgument(_))
    ));
}

#[tokio::test]
async fn truncated_header_is_a_protocol_error() {
    let (host_end, mut device_end) = tokio::io::duplex(1024);
    let mut transport = StreamTransport::new(host_end);
    device_end.write_all(&[1, 2, 3, 4, 5]).await.unwrap();
    drop(device_end);
    let err = transport.recv().await.unwrap_err();
    assert!(
        matches!(err, Error::Protocol(ref m) if m.contains("truncated header")),
        "{err}"
    );
}

#[tokio::test]
async fn bad_magic_is_a_protocol_error() {
    let (host_end, mut device_end) = tokio::io::duplex(1024);
    let mut transport = StreamTransport::new(host_end);
    let mut raw = encode_header(&Message::empty(Command::Okay, 1, 2).header(true).unwrap());
    raw[20] ^= 0xFF;
    device_end.write_all(&raw).await.unwrap();
    let err = transport.recv().await.unwrap_err();
    assert!(
        matches!(err, Error::Protocol(ref m) if m.contains("bad magic")),
        "{err}"
    );
}

#[tokio::test]
async fn unknown_command_and_oversized_payload_are_rejected() {
    let (host_end, mut device_end) = tokio::io::duplex(1024);
    let mut transport = StreamTransport::new(host_end);
    let mut raw = [0u8; HEADER_LEN];
    raw[0..4].copy_from_slice(b"NOPE");
    let code = u32::from_le_bytes(*b"NOPE");
    raw[20..24].copy_from_slice(&(code ^ 0xFFFF_FFFF).to_le_bytes());
    device_end.write_all(&raw).await.unwrap();
    assert!(
        matches!(transport.recv().await, Err(Error::Protocol(ref m)) if m.contains("unknown command"))
    );

    let (host_end, mut device_end) = tokio::io::duplex(1024);
    let mut transport = StreamTransport::new(host_end);
    let header = rsadb::wire::Header {
        command: Command::Write,
        arg0: 1,
        arg1: 1,
        data_length: MAX_PAYLOAD + 1,
        data_check: 0,
    };
    device_end.write_all(&encode_header(&header)).await.unwrap();
    assert!(matches!(transport.recv().await, Err(Error::Protocol(ref m)) if m.contains("exceeds")));
}

#[tokio::test]
async fn truncated_payload_and_checksum_mismatch() {
    let (host_end, mut device_end) = tokio::io::duplex(1024);
    let mut transport = StreamTransport::new(host_end);
    let msg = Message::new(Command::Write, 1, 1, "hello");
    device_end
        .write_all(&encode_header(&msg.header(true).unwrap()))
        .await
        .unwrap();
    device_end.write_all(b"hel").await.unwrap();
    drop(device_end);
    assert!(
        matches!(transport.recv().await, Err(Error::Protocol(ref m)) if m.contains("truncated payload"))
    );

    let (host_end, device_end) = tokio::io::duplex(1024);
    let mut host = StreamTransport::new(host_end);
    host.configure(WireConfig::negotiated(0x0100_0000, MAX_PAYLOAD));
    let mut device = StreamTransport::new(device_end);
    device.configure(WireConfig {
        compute_checksum: false,
        verify_checksum: false,
        max_payload: MAX_PAYLOAD,
    });
    device
        .send(Message::new(Command::Write, 1, 1, "payload"))
        .await
        .unwrap();
    assert!(matches!(host.recv().await, Err(Error::Protocol(ref m)) if m.contains("checksum")));
}

#[tokio::test]
async fn device_vanishing_closes_streams() {
    let (fake, transport) = fake::spawn(FakeConfig::trusting(host_key()));
    let session = Session::connect(transport, host_key(), WAIT).await.unwrap();
    let mut stream = session.open_stream("sync:").await.unwrap();
    fake.task.abort();
    let _ = fake.task.await;
    // Reads end, and writes fail once the transport is gone.
    assert!(matches!(stream.recv().await, Ok(None) | Err(_)));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let err = stream
        .send(bytes::Bytes::from_static(b"QUIT\0\0\0\0"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::StreamClosed | Error::Disconnected),
        "{err}"
    );
    assert!(!session.is_alive());
}

#[tokio::test]
async fn dropping_a_stream_sends_close() {
    let (fake, transport) = fake::spawn(FakeConfig::trusting(host_key()));
    let session = Session::connect(transport, host_key(), WAIT).await.unwrap();
    let stream = session.open_stream("sync:").await.unwrap();
    let local = stream.local_id();
    drop(stream);
    Device::new(&session).shell("true").await.unwrap();
    let received = fake.shared.received.lock().unwrap();
    assert!(
        received
            .iter()
            .any(|m| m.command == Command::Close && m.arg0 == local)
    );
}

#[tokio::test]
async fn session_drop_stops_tasks() {
    let (_fake, transport) = fake::spawn(FakeConfig::trusting(host_key()));
    let session = Session::connect(transport, host_key(), WAIT).await.unwrap();
    let mut stream = session.open_stream("sync:").await.unwrap();
    drop(session);
    assert!(matches!(stream.recv().await, Ok(None)));
    assert!(matches!(
        stream.send(bytes::Bytes::from_static(b"x")).await,
        Err(Error::StreamClosed)
    ));
}

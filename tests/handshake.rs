//! Connection and authentication paths against the fake device.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::fake::{self, FakeConfig};
use common::{host_key, stranger_key};
use rsadb::wire::{AUTH_RSAPUBLICKEY, AUTH_SIGNATURE, Command, LEGACY_MAX_PAYLOAD, MAX_PAYLOAD};
use rsadb::{Error, Session};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(5);

fn auth_types(device: &fake::FakeDevice) -> Vec<u32> {
    device
        .shared
        .received
        .lock()
        .unwrap()
        .iter()
        .filter(|m| m.command == Command::Auth)
        .map(|m| m.arg0)
        .collect()
}

#[tokio::test]
async fn trusted_key_signs_the_token_and_connects() {
    let (device, transport) = fake::spawn(FakeConfig::trusting(host_key()));
    let session = Session::connect(transport, host_key(), WAIT).await.unwrap();
    assert_eq!(session.banner().kind, "device");
    assert_eq!(session.banner().properties["ro.product.model"], "Fake");
    assert!(session.banner().has_feature("shell_v2"));
    assert_eq!(session.max_payload(), MAX_PAYLOAD);
    assert_eq!(auth_types(&device), vec![AUTH_SIGNATURE]);
    assert!(device.shared.accepted_key.lock().unwrap().is_none());
}

#[tokio::test]
async fn unknown_key_is_offered_and_accepted() {
    let config = FakeConfig {
        accept_new_keys: true,
        ..FakeConfig::default()
    };
    let (device, transport) = fake::spawn(config);
    let session = Session::connect(transport, stranger_key(), WAIT)
        .await
        .unwrap();
    assert_eq!(session.banner().kind, "device");
    assert_eq!(auth_types(&device), vec![AUTH_SIGNATURE, AUTH_RSAPUBLICKEY]);
    let accepted = device.shared.accepted_key.lock().unwrap().clone().unwrap();
    assert_eq!(&accepted, stranger_key().public_key());
    let sent = device.shared.received.lock().unwrap();
    let offer = sent
        .iter()
        .find(|m| m.command == Command::Auth && m.arg0 == AUTH_RSAPUBLICKEY)
        .unwrap();
    assert_eq!(
        offer.payload.last(),
        Some(&0),
        "public key payload is NUL-terminated"
    );
    let line = std::str::from_utf8(&offer.payload[..offer.payload.len() - 1]).unwrap();
    assert_eq!(line, stranger_key().public_key_line().unwrap());
    assert!(line.ends_with(" stranger@rsadb"));
}

#[tokio::test]
async fn unaccepted_key_reports_unauthorized() {
    let (_device, transport) = fake::spawn(FakeConfig::default());
    let err = Session::connect(transport, stranger_key(), Duration::from_millis(200))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unauthorized), "{err}");
}

#[tokio::test]
async fn legacy_device_without_auth_uses_checksums_and_small_payloads() {
    let (device, transport) = fake::spawn(FakeConfig::legacy());
    let session = Session::connect(transport, host_key(), WAIT).await.unwrap();
    assert_eq!(session.negotiated().version, 0x0100_0000);
    assert_eq!(session.max_payload(), LEGACY_MAX_PAYLOAD);
    assert!(auth_types(&device).is_empty());
    // A round trip proves both sides compute and verify data_check.
    let out = rsadb::Device::new(session)
        .shell_v1("echo legacy")
        .await
        .unwrap();
    assert_eq!(&out[..], b"legacy\n");
}

#[tokio::test]
async fn tls_request_is_a_clear_unsupported_error() {
    let config = FakeConfig {
        request_tls: true,
        ..FakeConfig::default()
    };
    let (_device, transport) = fake::spawn(config);
    let err = Session::connect(transport, host_key(), WAIT)
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Unsupported(ref m) if m.contains("A_STLS")),
        "{err}"
    );
}

#[tokio::test]
async fn host_banner_advertises_features() {
    let (device, transport) = fake::spawn(FakeConfig::trusting(host_key()));
    let _session = Session::connect(transport, host_key(), WAIT).await.unwrap();
    let received = device.shared.received.lock().unwrap();
    let cnxn = received
        .iter()
        .find(|m| m.command == Command::Connect)
        .unwrap();
    assert_eq!(cnxn.arg0, rsadb::wire::VERSION);
    assert_eq!(cnxn.arg1, MAX_PAYLOAD);
    let banner = rsadb::session::Banner::parse(std::str::from_utf8(&cnxn.payload).unwrap());
    assert_eq!(banner.kind, "host");
    assert!(banner.has_feature("shell_v2") && banner.has_feature("stat_v2"));
}

#[tokio::test]
async fn device_that_hangs_up_mid_handshake_is_disconnected() {
    let (host_end, device_end) = tokio::io::duplex(1024);
    drop(device_end);
    let transport = rsadb::transport::StreamTransport::new(host_end);
    let err = Session::connect(transport, host_key(), WAIT)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Disconnected | Error::Io(_)), "{err}");
}

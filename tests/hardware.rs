//! Tests that need a real, USB-debugging-enabled Android device.
//!
//! Run with: `cargo test --features hardware --test hardware -- --ignored`
//! and accept the "Allow USB debugging?" prompt on the phone if asked.

#![cfg(feature = "hardware")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rsadb::auth;
use rsadb::transport::usb;
use rsadb::{Device, Session};
use std::time::Duration;

async fn connect() -> Device<Session> {
    let key = auth::load_or_generate(&auth::default_key_paths().unwrap(), "rsadb-test@ci").unwrap();
    let info = usb::find(None)
        .await
        .expect("exactly one ADB device attached");
    let transport = usb::open(&info)
        .await
        .expect("claim the ADB interface (stop adb server first)");
    let session = Session::connect(transport, &key, Duration::from_secs(120))
        .await
        .unwrap();
    Device::new(session)
}

#[tokio::test]
#[ignore = "needs an attached Android device"]
async fn real_device_answers_getprop_and_shell() {
    let device = connect().await;
    let model = device.getprop("ro.product.model").await.unwrap();
    assert!(!model.is_empty());
    let out = device.shell("echo hardware ok").await.unwrap();
    assert_eq!(out.stdout_text().trim(), "hardware ok");
    assert!(out.success());
}

#[tokio::test]
#[ignore = "needs an attached Android device"]
async fn real_device_lists_and_stats_sdcard() {
    let device = connect().await;
    assert!(device.stat("/sdcard").await.unwrap().is_dir());
    let entries = device.list_dir("/sdcard").await.unwrap();
    assert!(!entries.is_empty());
}

#[tokio::test]
#[ignore = "needs an attached Android device"]
async fn real_device_screencap_is_png() {
    let device = connect().await;
    let png = device.screencap().await.unwrap();
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
}

#[tokio::test]
#[ignore = "needs an attached Android device"]
async fn real_device_push_and_pull_roundtrip() {
    let device = connect().await;
    let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
    device
        .push_bytes(
            &payload,
            "/data/local/tmp/rsadb-roundtrip.bin",
            0o100_644,
            1_700_000_000,
        )
        .await
        .unwrap();
    assert_eq!(
        device
            .pull_bytes("/data/local/tmp/rsadb-roundtrip.bin")
            .await
            .unwrap(),
        payload
    );
    device
        .shell("rm /data/local/tmp/rsadb-roundtrip.bin")
        .await
        .unwrap();
}

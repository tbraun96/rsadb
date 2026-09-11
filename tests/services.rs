//! Shell, exec, properties, screencap, content queries and reboot.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::fake::{self, FakeConfig, FakeDevice};
use common::{PNG, host_key};
use rsadb::services::RebootTarget;
use rsadb::{Device, Error, Session};
use std::time::Duration;

async fn connect(config: FakeConfig) -> (FakeDevice, Device<Session>) {
    let (fake, transport) = fake::spawn(config);
    let session = Session::connect(transport, host_key(), Duration::from_secs(5))
        .await
        .unwrap();
    (fake, Device::new(session))
}

fn v1_only() -> FakeConfig {
    let mut config = FakeConfig::trusting(host_key());
    config.banner.features = vec!["cmd".into()];
    config
}

#[tokio::test]
async fn shell_v2_separates_streams_and_exit_code() {
    let (_fake, device) = connect(FakeConfig::trusting(host_key())).await;
    let out = device.shell("warn").await.unwrap();
    assert_eq!(out.stdout, b"out\n");
    assert_eq!(out.stderr, b"err\n");
    assert_eq!(out.exit_code, Some(3));
    assert!(!out.success());
    let ok = device.shell("echo hi there").await.unwrap();
    assert_eq!(ok.stdout_text(), "hi there\n");
    assert_eq!(ok.exit_code, Some(0));
}

#[tokio::test]
async fn shell_v2_feeds_stdin() {
    let (_fake, device) = connect(FakeConfig::trusting(host_key())).await;
    let out = device
        .shell_with_stdin("cat", b"piped input")
        .await
        .unwrap();
    assert_eq!(out.stdout, b"piped input");
    assert_eq!(out.exit_code, Some(0));
}

#[tokio::test]
async fn shell_v1_fallback_merges_output() {
    let (_fake, device) = connect(v1_only()).await;
    let out = device.shell("warn").await.unwrap();
    assert_eq!(out.stdout, b"out\nerr\n");
    assert_eq!(out.exit_code, None);
    assert!(out.success());
    let err = device.shell_with_stdin("cat", b"x").await.unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)));
}

#[tokio::test]
async fn getprop_and_getprops() {
    let (_fake, device) = connect(FakeConfig::trusting(host_key())).await;
    assert_eq!(
        device.getprop("ro.product.model").await.unwrap(),
        "Fake Phone"
    );
    assert_eq!(device.getprop("does.not.exist").await.unwrap(), "");
    let all = device.getprops().await.unwrap();
    assert_eq!(all["ro.build.version.sdk"], "34");
    assert_eq!(all["persist.multi"], "one\ntwo");
    let err = device.getprop("x; reboot").await.unwrap_err();
    assert!(matches!(err, Error::InvalidArgument(_)));
}

#[tokio::test]
async fn screencap_returns_png_bytes() {
    let (_fake, device) = connect(FakeConfig::trusting(host_key())).await;
    assert_eq!(device.screencap().await.unwrap(), PNG);
    let mut bad = FakeConfig::trusting(host_key());
    bad.screencap_png = b"Error: unable to connect to SurfaceFlinger\n".to_vec();
    let (_fake, device) = connect(bad).await;
    let err = device.screencap().await.unwrap_err();
    assert!(
        matches!(err, Error::Parse(ref m) if m.contains("SurfaceFlinger")),
        "{err}"
    );
}

#[tokio::test]
async fn exec_is_raw_stdout() {
    let (_fake, device) = connect(FakeConfig::trusting(host_key())).await;
    assert_eq!(&device.exec("warn").await.unwrap()[..], b"out\n");
}

#[tokio::test]
async fn content_query_keeps_commas_in_last_column() {
    let (fake, device) = connect(FakeConfig::trusting(host_key())).await;
    let rows = device
        .content_query("content://sms/inbox", &["_id", "address", "body"])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["_id"], "1");
    assert_eq!(rows[0]["address"], "+15550001");
    assert_eq!(rows[0]["body"], "Hello, world, with, commas");
    let opened: Vec<String> = fake
        .shared
        .received
        .lock()
        .unwrap()
        .iter()
        .filter(|m| m.command == rsadb::wire::Command::Open)
        .map(|m| {
            String::from_utf8_lossy(&m.payload)
                .trim_end_matches('\0')
                .to_owned()
        })
        .collect();
    assert!(
        opened
            .iter()
            .any(|s| s.contains("--projection _id:address:body")),
        "{opened:?}"
    );
    assert!(
        opened.iter().any(|s| s.ends_with("--projection _id")),
        "{opened:?}"
    );
}

#[tokio::test]
async fn content_query_detects_row_count_mismatch() {
    let mut config = FakeConfig::trusting(host_key());
    config.content_extra_ids = 1;
    let (_fake, device) = connect(config).await;
    let err = device
        .content_query("content://sms/inbox", &["_id", "body"])
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Parse(ref m) if m.contains("reports 2")),
        "{err}"
    );
    let err = device
        .content_query("content://sms/inbox", &[])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidArgument(_)));
}

#[tokio::test]
async fn content_query_with_no_rows() {
    let mut config = FakeConfig::trusting(host_key());
    config.content_rows.clear();
    let (_fake, device) = connect(config).await;
    assert!(
        device
            .content_query("content://x", &["_id"])
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn reboot_opens_the_reboot_service() {
    let (fake, device) = connect(FakeConfig::trusting(host_key())).await;
    device.reboot(RebootTarget::Recovery).await.unwrap();
    assert_eq!(
        *fake.shared.rebooted.lock().unwrap(),
        vec!["recovery".to_owned()]
    );
}

#[tokio::test]
async fn features_are_cached_from_the_banner() {
    let (_fake, device) = connect(FakeConfig::trusting(host_key())).await;
    assert!(device.has_feature("ls_v2").await.unwrap());
    assert!(!device.has_feature("nope").await.unwrap());
}

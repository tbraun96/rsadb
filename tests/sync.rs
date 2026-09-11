//! The `sync:` service: list, stat, pull, push, and its failure paths.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::fake::{self, FakeConfig, FakeDevice, big_file, fs::Node};
use common::host_key;
use futures::StreamExt as _;
use rsadb::services::sync::{S_IFDIR, S_IFMT, S_IFREG};
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
    config.banner.features = vec!["shell_v2".into()];
    config
}

#[tokio::test]
async fn list_v2_and_v1() {
    for config in [FakeConfig::trusting(host_key()), v1_only()] {
        let v2 = config.banner.has_feature("ls_v2");
        let (_fake, device) = connect(config).await;
        let mut entries = device.list_dir("/sdcard").await.unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Download", "big.bin", "hello.txt"], "v2={v2}");
        assert!(entries[0].is_dir());
        assert_eq!(entries[1].size, 200_000);
        assert_eq!(entries[1].mode & S_IFMT, S_IFREG);
        assert_eq!(entries[2].mtime, 1_700_000_001);
        assert!(device.list_dir("/nope").await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn stat_v2_and_v1() {
    let (_fake, device) = connect(FakeConfig::trusting(host_key())).await;
    let stat = device.stat("/sdcard/hello.txt").await.unwrap();
    assert!(stat.exists() && stat.is_file());
    assert_eq!(stat.size, 22);
    assert_eq!(stat.detail.unwrap().uid, 2000);
    let dir = device.stat("/sdcard").await.unwrap();
    assert!(dir.is_dir() && dir.mode & S_IFMT == S_IFDIR);
    let missing = device.stat("/missing").await.unwrap();
    assert!(!missing.exists());
    assert_eq!(missing.error, Some(2));

    let (_fake, device) = connect(v1_only()).await;
    let stat = device.stat("/sdcard/hello.txt").await.unwrap();
    assert!(stat.exists() && stat.detail.is_none());
    assert_eq!(stat.mtime, 1_700_000_001);
    assert!(!device.stat("/missing").await.unwrap().exists());
}

#[tokio::test]
async fn pull_streams_64k_chunks() {
    for config in [FakeConfig::trusting(host_key()), v1_only()] {
        let (_fake, device) = connect(config).await;
        let mut stream = std::pin::pin!(device.pull("/sdcard/big.bin").await.unwrap());
        let mut chunks = Vec::new();
        while let Some(chunk) = stream.next().await {
            chunks.push(chunk.unwrap());
        }
        assert!(chunks.iter().all(|c| c.len() <= 64 * 1024));
        assert!(chunks.len() >= 4);
        assert_eq!(chunks.concat(), big_file());
        assert_eq!(
            device.pull_bytes("/sdcard/hello.txt").await.unwrap(),
            b"hello from the device\n"
        );
    }
}

#[tokio::test]
async fn pull_to_file_and_missing_path() {
    let (_fake, device) = connect(FakeConfig::trusting(host_key())).await;
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("big.bin");
    assert_eq!(
        device
            .pull_to_file("/sdcard/big.bin", &local)
            .await
            .unwrap(),
        200_000
    );
    assert_eq!(std::fs::read(&local).unwrap(), big_file());
    let err = device.pull_bytes("/sdcard/nothing").await.unwrap_err();
    assert!(
        matches!(err, Error::RemoteFailure(ref m) if m.contains("No such file")),
        "{err}"
    );
}

#[tokio::test]
async fn push_bytes_and_files() {
    for config in [FakeConfig::trusting(host_key()), v1_only()] {
        let (fake, device) = connect(config).await;
        let payload = big_file();
        device
            .push_bytes(&payload, "/sdcard/pushed.bin", 0o100_640, 1_234_567)
            .await
            .unwrap();
        let fs = fake.shared.fs.lock().unwrap();
        match fs.get("/sdcard/pushed.bin").unwrap() {
            Node::File { mode, mtime, data } => {
                assert_eq!(*data, payload);
                assert_eq!(*mode & 0o777, 0o640);
                assert_eq!(*mtime, 1_234_567);
            }
            Node::Dir { .. } => panic!("expected a file"),
        }
    }
    let (fake, device) = connect(FakeConfig::trusting(host_key())).await;
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("note.txt");
    std::fs::write(&local, b"from disk").unwrap();
    device
        .push_file(&local, "/sdcard/Download/note.txt")
        .await
        .unwrap();
    assert_eq!(
        fake.shared
            .fs
            .lock()
            .unwrap()
            .read("/sdcard/Download/note.txt"),
        Some(&b"from disk"[..])
    );
    let err = device.push_file(dir.path(), "/sdcard/x").await.unwrap_err();
    assert!(matches!(err, Error::InvalidArgument(_)));
}

#[tokio::test]
async fn push_into_missing_directory_fails() {
    let (_fake, device) = connect(FakeConfig::trusting(host_key())).await;
    let err = device
        .push_bytes(b"x", "/nowhere/file", 0o644, 0)
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::RemoteFailure(ref m) if m.contains("No such file")),
        "{err}"
    );
}

#[tokio::test]
async fn one_sync_stream_serves_many_requests() {
    let (_fake, device) = connect(FakeConfig::trusting(host_key())).await;
    let mut sync = device.sync().await.unwrap();
    assert_eq!(sync.list("/sdcard").await.unwrap().len(), 3);
    assert!(sync.stat("/sdcard/big.bin").await.unwrap().exists());
    let mut total = 0;
    {
        let mut stream = std::pin::pin!(sync.pull("/sdcard/big.bin").await.unwrap());
        while let Some(chunk) = stream.next().await {
            total += chunk.unwrap().len();
        }
    }
    assert_eq!(total, 200_000);
    sync.push("/sdcard/again.txt", 0o644, 7, &b"again"[..])
        .await
        .unwrap();
    assert_eq!(sync.stat("/sdcard/again.txt").await.unwrap().size, 5);
    sync.quit().await.unwrap();
}

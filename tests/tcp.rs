//! The fake device served over a real loopback TCP socket.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::fake::{self, FakeConfig, Shared};
use common::host_key;
use rsadb::transport::{StreamTransport, tcp};
use rsadb::{Device, Error, Session};
use std::time::Duration;
use tokio::net::TcpListener;

#[tokio::test]
async fn connects_over_tcp_and_runs_services() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shared = Shared::new(FakeConfig::trusting(host_key()));
    let server = {
        let shared = std::sync::Arc::clone(&shared);
        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            fake::spawn_on(shared, StreamTransport::new(socket))
                .task
                .await
                .unwrap()
        })
    };

    let transport = tcp::connect(addr).await.unwrap();
    let session = Session::connect(transport, host_key(), Duration::from_secs(5))
        .await
        .unwrap();
    let device = Device::new(session);
    assert_eq!(
        device.getprop("ro.product.model").await.unwrap(),
        "Fake Phone"
    );
    assert_eq!(
        device.pull_bytes("/sdcard/big.bin").await.unwrap(),
        fake::big_file()
    );
    let (host, port) = tcp::parse_endpoint(&addr.to_string()).unwrap();
    assert_eq!(
        (host.as_str(), port),
        (addr.ip().to_string().as_str(), addr.port())
    );
    drop(device);
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn connection_refused_is_an_io_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let err = tcp::connect(addr).await.unwrap_err();
    assert!(matches!(err, Error::Io(_)), "{err}");
}

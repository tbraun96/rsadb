//! A fake Google adb *server* on loopback, and the host-client against it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use futures::StreamExt as _;
use rsadb::host::HostClient;
use rsadb::{Connection as _, Device, Error};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};

async fn read_request(socket: &mut TcpStream) -> Option<String> {
    let mut len = [0u8; 4];
    socket.read_exact(&mut len).await.ok()?;
    let len = usize::from_str_radix(std::str::from_utf8(&len).ok()?, 16).ok()?;
    let mut buf = vec![0u8; len];
    socket.read_exact(&mut buf).await.ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

async fn reply(socket: &mut TcpStream, text: &str) {
    socket
        .write_all(format!("OKAY{:04x}{text}", text.len()).as_bytes())
        .await
        .unwrap();
}

/// Serve one adb-server connection.
async fn serve(mut socket: TcpStream) {
    let Some(request) = read_request(&mut socket).await else {
        return;
    };
    match request.as_str() {
        "host:version" => reply(&mut socket, "0029").await,
        "host:devices-l" => {
            reply(
                &mut socket,
                "R5CT1 device product:p model:Pixel_7 transport_id:3\nemu offline transport_id:4\n",
            )
            .await;
        }
        "host:features" => reply(&mut socket, "shell_v2,cmd,stat_v2").await,
        "host-serial:R5CT1:features" => reply(&mut socket, "shell_v2,ls_v2").await,
        "host:track-devices" => {
            socket.write_all(b"OKAY").await.unwrap();
            for snapshot in ["R5CT1\tdevice\n", "", "R5CT1\tdevice\nemu\tdevice\n"] {
                socket
                    .write_all(format!("{:04x}{snapshot}", snapshot.len()).as_bytes())
                    .await
                    .unwrap();
            }
        }
        "host:transport:R5CT1" => {
            socket.write_all(b"OKAY").await.unwrap();
            let Some(service) = read_request(&mut socket).await else {
                return;
            };
            if service == "shell:echo via server" {
                socket.write_all(b"OKAYvia server\n").await.unwrap();
            } else if service == "shell,v2:id" {
                socket.write_all(b"OKAY").await.unwrap();
                socket
                    .write_all(&[1, 3, 0, 0, 0, b'u', b'i', b'd', 3, 1, 0, 0, 0, 0])
                    .await
                    .unwrap();
            } else {
                let msg = format!("unknown service {service}");
                socket
                    .write_all(format!("FAIL{:04x}{msg}", msg.len()).as_bytes())
                    .await
                    .unwrap();
            }
        }
        "host:transport:missing" => {
            socket.write_all(b"FAIL0010device not found").await.unwrap();
        }
        other => {
            let msg = format!("unknown host service {other}");
            socket
                .write_all(format!("FAIL{:04x}{msg}", msg.len()).as_bytes())
                .await
                .unwrap();
        }
    }
    // Send FIN, then drain whatever the client still writes (e.g. a shell v2
    // close-stdin frame) until it hangs up. Closing with unread bytes would
    // make the kernel send RST and the client see ECONNRESET instead of EOF.
    let _ = socket.shutdown().await;
    let mut sink = [0u8; 1024];
    while matches!(socket.read(&mut sink).await, Ok(n) if n > 0) {}
}

async fn start_server() -> HostClient {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            tokio::spawn(serve(socket));
        }
    });
    HostClient::new(addr)
}

#[tokio::test]
async fn version_devices_and_features() {
    let client = start_server().await;
    assert_eq!(client.version().await.unwrap(), 41);
    let devices = client.devices().await.unwrap();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].serial, "R5CT1");
    assert_eq!(
        devices[0].attributes[1],
        ("model".to_owned(), "Pixel_7".to_owned())
    );
    assert_eq!(
        client.host_features().await.unwrap(),
        ["shell_v2", "cmd", "stat_v2"]
    );
    let only = client.only_device().await.unwrap();
    assert_eq!(only.serial(), "R5CT1");
    assert_eq!(only.features().await.unwrap(), ["shell_v2", "ls_v2"]);
}

#[tokio::test]
async fn services_through_the_server() {
    let client = start_server().await;
    let device = Device::new(client.device("R5CT1"));
    assert_eq!(
        &device.shell_v1("echo via server").await.unwrap()[..],
        b"via server\n"
    );
    let out = device.shell("id").await.unwrap();
    assert_eq!(out.stdout, b"uid");
    assert_eq!(out.exit_code, Some(0));
    let err = device.shell_v1("nope").await.unwrap_err();
    assert!(matches!(err, Error::ServiceRefused { .. }), "{err}");
    let err = Device::new(client.device("missing"))
        .shell_v1("x")
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::RemoteFailure(ref m) if m == "device not found"),
        "{err}"
    );
}

#[tokio::test]
async fn track_devices_streams_snapshots() {
    let client = start_server().await;
    let stream = client.track_devices().await.unwrap();
    let snapshots: Vec<_> = stream.map(|s| s.unwrap()).collect().await;
    assert_eq!(snapshots.len(), 3);
    assert_eq!(snapshots[0][0].serial, "R5CT1");
    assert!(snapshots[1].is_empty());
    assert_eq!(snapshots[2].len(), 2);
}

#[tokio::test]
async fn no_server_is_an_io_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let err = HostClient::new(addr).version().await.unwrap_err();
    assert!(matches!(err, Error::Io(_)), "{err}");
}

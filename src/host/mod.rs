//! Client for a running Google adb *server* (`adb start-server`, port 5037).
//!
//! Use this when another program already owns the USB interface: the server
//! multiplexes for everyone. Each service stream is its own TCP connection,
//! bound to a device with `host:transport:<serial>`.

mod channel;
mod proto;

pub use channel::HostChannel;
pub use proto::DeviceEntry;

use crate::channel::Connection;
use crate::error::{Error, Result};
use futures::Stream;
use std::net::SocketAddr;
use tokio::net::TcpStream;

/// The adb server's default port.
pub const DEFAULT_PORT: u16 = 5037;

/// A handle to an adb server.
#[derive(Debug, Clone)]
pub struct HostClient {
    addr: SocketAddr,
}

impl HostClient {
    /// The server on `127.0.0.1:5037`.
    pub fn local() -> Self {
        Self {
            addr: SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)),
        }
    }

    /// A server at `addr`.
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr }
    }

    async fn connect(&self) -> Result<TcpStream> {
        let stream = TcpStream::connect(self.addr).await.map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!("adb server at {}: {e}", self.addr),
            ))
        })?;
        stream.set_nodelay(true)?;
        Ok(stream)
    }

    /// Send one `host:` request and return its length-prefixed reply.
    pub async fn query(&self, request: &str) -> Result<String> {
        let mut stream = self.connect().await?;
        proto::send_request(&mut stream, request).await?;
        proto::read_status(&mut stream).await?;
        proto::read_block(&mut stream).await
    }

    /// `host:version` as an integer.
    pub async fn version(&self) -> Result<u32> {
        let hex = self.query("host:version").await?;
        u32::from_str_radix(hex.trim(), 16).map_err(|_| Error::parse(format!("version {hex:?}")))
    }

    /// `host:devices-l`.
    pub async fn devices(&self) -> Result<Vec<DeviceEntry>> {
        Ok(proto::parse_devices(&self.query("host:devices-l").await?))
    }

    /// `host:features`: the features the server itself supports.
    pub async fn host_features(&self) -> Result<Vec<String>> {
        Ok(split_features(&self.query("host:features").await?))
    }

    /// `host:track-devices`: a stream of device-list snapshots, one per change.
    pub async fn track_devices(
        &self,
    ) -> Result<impl Stream<Item = Result<Vec<DeviceEntry>>> + Send> {
        let mut stream = self.connect().await?;
        proto::send_request(&mut stream, "host:track-devices").await?;
        proto::read_status(&mut stream).await?;
        Ok(futures::stream::unfold(Some(stream), |state| async move {
            let mut stream = state?;
            match proto::read_block(&mut stream).await {
                Ok(text) => Some((Ok(proto::parse_devices(&text)), Some(stream))),
                Err(Error::Disconnected) => None,
                Err(e) => Some((Err(e), None)),
            }
        }))
    }

    /// A [`Connection`] to the device with `serial`.
    pub fn device(&self, serial: impl Into<String>) -> HostDevice {
        HostDevice {
            client: self.clone(),
            serial: serial.into(),
        }
    }

    /// The single connected device, or an error naming the count.
    pub async fn only_device(&self) -> Result<HostDevice> {
        let mut devices = self.devices().await?;
        devices.retain(|d| d.state == "device");
        match devices.len() {
            1 => Ok(self.device(devices.remove(0).serial)),
            0 => Err(Error::NoDevice(Some(
                "adb server lists no online device".into(),
            ))),
            n => Err(Error::NoDevice(Some(format!(
                "adb server lists {n} devices, pass a serial"
            )))),
        }
    }
}

/// One device behind an adb server.
#[derive(Debug, Clone)]
pub struct HostDevice {
    client: HostClient,
    serial: String,
}

impl HostDevice {
    /// The serial this handle is bound to.
    pub fn serial(&self) -> &str {
        &self.serial
    }
}

fn split_features(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(str::to_owned)
        .collect()
}

impl Connection for HostDevice {
    type Channel = HostChannel;

    async fn open(&self, service: &str) -> Result<HostChannel> {
        let mut stream = self.client.connect().await?;
        proto::send_request(&mut stream, &format!("host:transport:{}", self.serial)).await?;
        proto::read_status(&mut stream).await?;
        proto::send_request(&mut stream, service).await?;
        proto::read_status(&mut stream).await.map_err(|e| match e {
            Error::RemoteFailure(_) => Error::ServiceRefused {
                service: service.to_owned(),
            },
            other => other,
        })?;
        Ok(HostChannel::new(stream))
    }

    async fn features(&self) -> Result<Vec<String>> {
        Ok(split_features(
            &self
                .client
                .query(&format!("host-serial:{}:features", self.serial))
                .await?,
        ))
    }
}

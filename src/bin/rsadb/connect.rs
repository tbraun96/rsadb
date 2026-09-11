//! Turning command-line options into a connected [`Device`].

use crate::cli::{Cli, Target};
use rsadb::auth::{self, HostKey, KeyPaths};
use rsadb::host::{HostClient, HostDevice};
use rsadb::transport::{tcp, usb};
use rsadb::{Device, Result, Session};
use std::time::Duration;

/// Either kind of device the CLI can drive.
pub enum AnyDevice {
    /// A direct USB or TCP session.
    Direct(Device<Session>),
    /// A device behind a Google adb server.
    Server(Device<HostDevice>),
}

/// Key comment for keys this CLI generates: `user@host` when known.
pub fn key_comment() -> String {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "rsadb".into());
    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "localhost".into());
    format!("{user}@{host}")
}

/// Where the key lives for these options.
pub fn key_paths(cli: &Cli) -> Result<KeyPaths> {
    match &cli.key {
        Some(private) => Ok(KeyPaths {
            public: private.with_extension("pub"),
            private: private.clone(),
        }),
        None => auth::default_key_paths(),
    }
}

fn load_key(cli: &Cli) -> Result<HostKey> {
    auth::load_or_generate(&key_paths(cli)?, &key_comment())
}

/// Connect according to the target options.
pub async fn connect(cli: &Cli) -> Result<AnyDevice> {
    let Target {
        serial,
        tcp,
        server,
    } = &cli.target;
    if *server {
        let client = HostClient::local();
        let device = match serial {
            Some(s) => client.device(s.clone()),
            None => client.only_device().await?,
        };
        return Ok(AnyDevice::Server(Device::new(device)));
    }
    let key = load_key(cli)?;
    let auth_wait = Duration::from_secs(cli.auth_wait);
    let session = if let Some(spec) = tcp {
        let (host, port) = tcp::parse_endpoint(spec)?;
        Session::connect(tcp::connect((host.as_str(), port)).await?, &key, auth_wait).await?
    } else {
        let info = usb::find(serial.as_deref()).await?;
        Session::connect(usb::open(&info).await?, &key, auth_wait).await?
    };
    Ok(AnyDevice::Direct(Device::new(session)))
}

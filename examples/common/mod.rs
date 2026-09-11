//! Shared connection helper for the examples: first USB device, default host key.

use rsadb::{Device, Session, auth, transport::usb};
use std::time::Duration;

/// Connect to the only attached USB device, waiting up to a minute for the
/// "Allow USB debugging?" tap if this host key is new to the phone.
pub async fn connect() -> rsadb::Result<Device<Session>> {
    let key = auth::load_or_generate(&auth::default_key_paths()?, "rsadb-example@host")?;
    let info = usb::find(None).await?;
    let transport = usb::open(&info).await?;
    let session = Session::connect(transport, &key, Duration::from_secs(60)).await?;
    Ok(Device::new(session))
}

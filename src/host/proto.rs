//! The adb *server* (host) protocol: `%04x`-length-prefixed requests,
//! `OKAY`/`FAIL` status words, and length-prefixed replies.

use crate::error::{Error, Result};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

/// Send one request (`<hex4 length><text>`).
pub async fn send_request<W: AsyncWrite + Unpin>(w: &mut W, request: &str) -> Result<()> {
    if request.len() > 0xFFFF {
        return Err(Error::InvalidArgument(
            "host request longer than 65535 bytes".into(),
        ));
    }
    w.write_all(format!("{:04x}{request}", request.len()).as_bytes())
        .await?;
    w.flush().await?;
    Ok(())
}

/// Read a four-hex-digit length.
pub async fn read_len<R: AsyncRead + Unpin>(r: &mut R) -> Result<usize> {
    let mut hex = [0u8; 4];
    r.read_exact(&mut hex).await.map_err(eof)?;
    let text = std::str::from_utf8(&hex).map_err(|_| Error::protocol("length is not ASCII hex"))?;
    usize::from_str_radix(text, 16).map_err(|_| Error::protocol(format!("bad length {text:?}")))
}

/// Read a length-prefixed block as text.
pub async fn read_block<R: AsyncRead + Unpin>(r: &mut R) -> Result<String> {
    let len = read_len(r).await?;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await.map_err(eof)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Read `OKAY` or `FAIL` (+ message).
pub async fn read_status<R: AsyncRead + Unpin>(r: &mut R) -> Result<()> {
    let mut word = [0u8; 4];
    r.read_exact(&mut word).await.map_err(eof)?;
    match &word {
        b"OKAY" => Ok(()),
        b"FAIL" => Err(Error::RemoteFailure(read_block(r).await?)),
        other => Err(Error::protocol(format!(
            "unexpected status {:?}",
            String::from_utf8_lossy(other)
        ))),
    }
}

fn eof(e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::UnexpectedEof {
        Error::Disconnected
    } else {
        Error::Io(e)
    }
}

/// A line of `host:devices-l` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEntry {
    /// Serial number (or `host:port` for TCP devices).
    pub serial: String,
    /// `device`, `offline`, `unauthorized`, `recovery`, …
    pub state: String,
    /// Extra `key:value` fields (`product`, `model`, `device`, `transport_id`).
    pub attributes: Vec<(String, String)>,
}

/// Parse `host:devices` / `host:devices-l` output.
pub fn parse_devices(text: &str) -> Vec<DeviceEntry> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?.to_owned();
            let state = parts.next()?.to_owned();
            let attributes = parts
                .filter_map(|p| p.split_once(':').map(|(k, v)| (k.to_owned(), v.to_owned())))
                .collect();
            Some(DeviceEntry {
                serial,
                state,
                attributes,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_devices_l() {
        let text = "emulator-5554          device product:sdk_gphone64 model:Pixel_7 device:emu64a transport_id:1\nR5CT1  unauthorized usb:1-1 transport_id:2\n";
        let devs = parse_devices(text);
        assert_eq!(devs.len(), 2);
        assert_eq!(devs[0].serial, "emulator-5554");
        assert_eq!(devs[0].state, "device");
        assert_eq!(devs[0].attributes[1], ("model".into(), "Pixel_7".into()));
        assert_eq!(devs[1].state, "unauthorized");
        assert!(parse_devices("").is_empty());
    }

    #[tokio::test]
    async fn framing_roundtrip() {
        let mut buf = Vec::new();
        send_request(&mut buf, "host:version")
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(buf, b"000chost:version");
        let mut reply: &[u8] = b"OKAY00040029";
        read_status(&mut reply)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(read_block(&mut reply).await.ok().as_deref(), Some("0029"));
        let mut fail: &[u8] = b"FAIL0005oops!";
        assert!(
            matches!(read_status(&mut fail).await, Err(Error::RemoteFailure(m)) if m == "oops!")
        );
        let mut short: &[u8] = b"OK";
        assert!(matches!(
            read_status(&mut short).await,
            Err(Error::Disconnected)
        ));
    }
}

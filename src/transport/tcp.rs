//! TCP transport (`adb connect`-style devices, emulators, and Wi-Fi debugging).

use super::StreamTransport;
use crate::error::{Error, Result};
use tokio::net::{TcpStream, ToSocketAddrs};

/// The port `adbd` listens on when TCP debugging is enabled.
pub const DEFAULT_PORT: u16 = 5555;

/// A transport over a TCP connection to `adbd`.
pub type TcpTransport = StreamTransport<TcpStream>;

/// Connect to `addr` (for example `"192.168.1.20:5555"`).
pub async fn connect(addr: impl ToSocketAddrs) -> Result<TcpTransport> {
    let stream = TcpStream::connect(addr).await?;
    stream.set_nodelay(true)?;
    Ok(StreamTransport::new(stream))
}

/// Split `host[:port]` into an address, applying [`DEFAULT_PORT`] when the port is absent.
pub fn parse_endpoint(spec: &str) -> Result<(String, u16)> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(Error::InvalidArgument("empty address".into()));
    }
    if let Some(rest) = spec.strip_prefix('[') {
        let (host, tail) = rest
            .split_once(']')
            .ok_or_else(|| Error::InvalidArgument(format!("unterminated IPv6 literal {spec:?}")))?;
        let port = match tail.strip_prefix(':') {
            Some(p) => parse_port(p)?,
            None if tail.is_empty() => DEFAULT_PORT,
            None => {
                return Err(Error::InvalidArgument(format!(
                    "unexpected {tail:?} after host"
                )));
            }
        };
        return Ok((host.to_owned(), port));
    }
    match spec.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => Ok((host.to_owned(), parse_port(port)?)),
        _ => Ok((spec.to_owned(), DEFAULT_PORT)),
    }
}

fn parse_port(text: &str) -> Result<u16> {
    text.parse()
        .map_err(|_| Error::InvalidArgument(format!("bad port {text:?}")))
}

#[cfg(test)]
mod tests {
    use super::parse_endpoint;

    #[test]
    fn endpoints() {
        assert_eq!(
            parse_endpoint("10.0.0.2").ok(),
            Some(("10.0.0.2".into(), 5555))
        );
        assert_eq!(
            parse_endpoint("10.0.0.2:5556").ok(),
            Some(("10.0.0.2".into(), 5556))
        );
        assert_eq!(parse_endpoint("[::1]:7").ok(), Some(("::1".into(), 7)));
        assert_eq!(parse_endpoint("[::1]").ok(), Some(("::1".into(), 5555)));
        assert!(parse_endpoint("host:notaport").is_err());
        assert!(parse_endpoint("").is_err());
    }
}

//! `A_CNXN` / `A_AUTH` exchange.
//!
//! ```text
//! host                                  device
//!  | CNXN(version, max, "host::…")  --->  |
//!  |  <---  AUTH(TOKEN, 20 random bytes)  |   (absent on devices without auth)
//!  | AUTH(SIGNATURE, sign(token))   --->  |
//!  |  <---  CNXN(version, max, "device::…")   if the key is known, else:
//!  |  <---  AUTH(TOKEN, token')            |
//!  | AUTH(RSAPUBLICKEY, "base64 user@host\0") ---> |
//!  |            … user taps "Allow" …      |
//!  |  <---  CNXN(version, max, "device::…")|
//! ```

use super::banner::Banner;
use crate::auth::HostKey;
use crate::error::{Error, Result};
use crate::transport::{Transport, WireConfig};
use crate::wire::{
    AUTH_RSAPUBLICKEY, AUTH_SIGNATURE, AUTH_TOKEN, Command, LEGACY_MAX_PAYLOAD, MAX_PAYLOAD,
    Message, VERSION, VERSION_MIN,
};
use std::time::Duration;

/// What the handshake negotiated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Negotiated {
    /// The device's banner.
    pub banner: Banner,
    /// Protocol version both sides speak.
    pub version: u32,
    /// Largest payload either side may send.
    pub max_payload: u32,
}

/// Run the handshake on `transport`, leaving it configured for the negotiated policy.
///
/// `auth_wait` bounds how long we wait for the user to accept our key after we
/// have offered it; on expiry the result is [`Error::Unauthorized`].
pub async fn handshake<T: Transport>(
    transport: &mut T,
    key: &HostKey,
    auth_wait: Duration,
) -> Result<Negotiated> {
    /// How many stale stream frames a device may send before the handshake gives up on it.
    const MAX_STALE: u32 = 64;

    transport.configure(WireConfig::INITIAL);
    transport
        .send(Message::new(
            Command::Connect,
            VERSION,
            MAX_PAYLOAD,
            Banner::host(),
        ))
        .await?;

    let mut sent_signature = false;
    let mut sent_pubkey = false;
    // A device that was talking to a previous session is still tearing its streams down, and the
    // bulk endpoint keeps those frames across our open: a fresh handshake can therefore read a
    // CLSE (or a late WRTE/OKAY) addressed to a session that no longer exists. Found on the first
    // real phone -- the emulator is reached over TCP, where a new socket cannot inherit old
    // traffic. Such frames are skipped rather than treated as protocol errors, but only so many,
    // so a device sending nothing else can never spin here.
    let mut skipped = 0_u32;
    loop {
        let msg = if sent_pubkey {
            tokio::time::timeout(auth_wait, transport.recv())
                .await
                .map_err(|_| Error::Unauthorized)??
        } else {
            transport.recv().await?
        };
        match msg.command {
            Command::Connect => {
                let negotiated = negotiate(&msg)?;
                transport.configure(WireConfig::negotiated(
                    negotiated.version,
                    negotiated.max_payload,
                ));
                return Ok(negotiated);
            }
            Command::Auth if msg.arg0 == AUTH_TOKEN => {
                if !sent_signature {
                    let signature = key.sign_token(&msg.payload)?;
                    transport
                        .send(Message::new(Command::Auth, AUTH_SIGNATURE, 0, signature))
                        .await?;
                    sent_signature = true;
                } else if !sent_pubkey {
                    let pubkey = key.public_key_payload()?;
                    transport
                        .send(Message::new(Command::Auth, AUTH_RSAPUBLICKEY, 0, pubkey))
                        .await?;
                    sent_pubkey = true;
                } else {
                    return Err(Error::Unauthorized);
                }
            }
            Command::Auth => {
                return Err(Error::protocol(format!(
                    "unexpected AUTH type {}",
                    msg.arg0
                )));
            }
            Command::StartTls => {
                return Err(Error::Unsupported(
                    "device requested TLS authentication (A_STLS); rsadb 0.1 only speaks RSA auth"
                        .into(),
                ));
            }
            Command::Close | Command::Write | Command::Okay | Command::Sync => {
                skipped += 1;
                if skipped > MAX_STALE {
                    return Err(Error::protocol(format!(
                        "device sent {skipped} stream frames and no banner during the handshake"
                    )));
                }
            }
            other @ Command::Open => {
                return Err(Error::protocol(format!(
                    "unexpected {other} during handshake"
                )));
            }
        }
    }
}

fn negotiate(msg: &Message) -> Result<Negotiated> {
    let version = msg.arg0;
    if version < VERSION_MIN {
        return Err(Error::protocol(format!(
            "peer protocol version {version:#010x} too old"
        )));
    }
    let offered = if msg.arg1 == 0 {
        LEGACY_MAX_PAYLOAD
    } else {
        msg.arg1
    };
    let banner = Banner::parse(&String::from_utf8_lossy(&msg.payload));
    Ok(Negotiated {
        banner,
        version: version.min(VERSION),
        max_payload: offered.min(MAX_PAYLOAD),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation_clamps_to_our_limits() {
        let msg = Message::new(
            Command::Connect,
            0x0100_0002,
            8 * 1024 * 1024,
            "device::features=cmd",
        );
        let n = negotiate(&msg).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(n.version, VERSION);
        assert_eq!(n.max_payload, MAX_PAYLOAD);
        assert_eq!(n.banner.features, vec!["cmd"]);
        let legacy = Message::new(Command::Connect, VERSION_MIN, 0, "device::");
        assert_eq!(
            negotiate(&legacy).map(|n| n.max_payload).ok(),
            Some(LEGACY_MAX_PAYLOAD)
        );
        assert!(negotiate(&Message::new(Command::Connect, 0, 0, "")).is_err());
    }
}

//! Host authentication: RSA key handling and token signing.
//!
//! ADB authenticates the *host* to the device. The device sends a random
//! 20-byte token; the host signs it with its RSA key (PKCS#1 v1.5 with the
//! SHA-1 `DigestInfo` prefix, exactly what `RSA_sign(NID_sha1, …)` produces).
//! If the device does not know the key it sends another token, the host
//! answers with its public key, and the user is asked to allow the host.

mod keyfile;
pub mod pubkey;

pub use keyfile::{KeyPaths, default_key_paths, load_or_generate, read_key, write_key};

use crate::error::{Error, Result};
use rsa::pkcs1::{DecodeRsaPrivateKey as _, EncodeRsaPrivateKey as _};
use rsa::pkcs1v15::Pkcs1v15Sign;
use rsa::pkcs8::DecodePrivateKey as _;
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha1::Sha1;

/// Length of the token a device sends in `A_AUTH(AUTH_TOKEN)`.
pub const TOKEN_LEN: usize = 20;
/// Key size Android requires.
pub const KEY_BITS: usize = 2048;

/// The host's RSA identity.
#[derive(Clone)]
pub struct HostKey {
    private: RsaPrivateKey,
    public: RsaPublicKey,
    comment: String,
}

impl std::fmt::Debug for HostKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostKey")
            .field("comment", &self.comment)
            .finish_non_exhaustive()
    }
}

impl HostKey {
    /// Generate a fresh 2048-bit key. `comment` is the `user@host` label stored in `adbkey.pub`.
    pub fn generate(comment: impl Into<String>) -> Result<Self> {
        let private = RsaPrivateKey::new(&mut rand::rngs::OsRng, KEY_BITS)?;
        Self::from_private(private, comment.into())
    }

    /// Wrap an existing private key.
    pub fn from_private(private: RsaPrivateKey, comment: String) -> Result<Self> {
        let public = private.to_public_key();
        pubkey::encode(&public)?;
        Ok(Self {
            private,
            public,
            comment,
        })
    }

    /// Parse a PEM private key: PKCS#8 (`BEGIN PRIVATE KEY`, what current adb
    /// writes) or PKCS#1 (`BEGIN RSA PRIVATE KEY`, what older adb wrote).
    pub fn from_pem(pem: &str, comment: impl Into<String>) -> Result<Self> {
        let private = match RsaPrivateKey::from_pkcs8_pem(pem) {
            Ok(key) => key,
            Err(pkcs8_err) => RsaPrivateKey::from_pkcs1_pem(pem).map_err(|pkcs1_err| {
                Error::Key(format!("not PKCS#8 ({pkcs8_err}) nor PKCS#1 ({pkcs1_err})"))
            })?,
        };
        Self::from_private(private, comment.into())
    }

    /// Serialise the private key as PKCS#1 PEM (`BEGIN RSA PRIVATE KEY`), which every adb reads.
    pub fn to_pem(&self) -> Result<String> {
        Ok(self
            .private
            .to_pkcs1_pem(rsa::pkcs8::LineEnding::LF)?
            .to_string())
    }

    /// The public half.
    pub fn public_key(&self) -> &RsaPublicKey {
        &self.public
    }

    /// The `user@host` label.
    pub fn comment(&self) -> &str {
        &self.comment
    }

    /// The `adbkey.pub` line for this key.
    pub fn public_key_line(&self) -> Result<String> {
        pubkey::encode_line(&self.public, &self.comment)
    }

    /// The payload of `A_AUTH(AUTH_RSAPUBLICKEY)`: the `adbkey.pub` line plus a NUL.
    pub fn public_key_payload(&self) -> Result<Vec<u8>> {
        let mut bytes = self.public_key_line()?.into_bytes();
        bytes.push(0);
        Ok(bytes)
    }

    /// Sign a 20-byte token the way `adb` does.
    pub fn sign_token(&self, token: &[u8]) -> Result<Vec<u8>> {
        if token.len() != TOKEN_LEN {
            return Err(Error::Protocol(format!(
                "auth token must be {TOKEN_LEN} bytes, got {}",
                token.len()
            )));
        }
        Ok(self.private.sign(Pkcs1v15Sign::new::<Sha1>(), token)?)
    }
}

/// Verify a token signature against a public key (what the device does).
pub fn verify_token(key: &RsaPublicKey, token: &[u8], signature: &[u8]) -> Result<()> {
    if token.len() != TOKEN_LEN {
        return Err(Error::Protocol(format!(
            "auth token must be {TOKEN_LEN} bytes, got {}",
            token.len()
        )));
    }
    key.verify(Pkcs1v15Sign::new::<Sha1>(), token, signature)
        .map_err(|e| Error::Key(format!("signature rejected: {e}")))
}

//! Android's public-key serialisation (`android_pubkey.c`).
//!
//! The device stores keys as base64 of a fixed 524-byte structure:
//!
//! ```text
//! len: u32      = 64 (modulus size in 32-bit words)
//! n0inv: u32    = -1 / n[0] mod 2^32   (Montgomery constant)
//! n: [u32; 64]  = modulus, little-endian words
//! rr: [u32; 64] = R^2 mod n where R = 2^2048, little-endian words
//! exponent: u32 = public exponent (65537)
//! ```

use crate::error::{Error, Result};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use rsa::traits::PublicKeyParts as _;
use rsa::{BigUint, RsaPublicKey};

/// Modulus size in bytes for the only key size Android accepts.
pub const MODULUS_BYTES: usize = 256;
/// Modulus size in 32-bit words (the `len` field).
pub const MODULUS_WORDS: u32 = 64;
/// Size of the encoded structure.
pub const ENCODED_LEN: usize = 4 + 4 + MODULUS_BYTES + MODULUS_BYTES + 4;

/// Compute `-n0^-1 mod 2^32` for an odd `n0` by Newton iteration.
///
/// Five doublings of precision reach 32 bits from the trivial 1-bit inverse.
pub fn n0inv(n0: u32) -> u32 {
    let mut inv = n0;
    for _ in 0..5 {
        inv = inv.wrapping_mul(2u32.wrapping_sub(n0.wrapping_mul(inv)));
    }
    0u32.wrapping_sub(inv)
}

fn to_le_padded(value: &BigUint) -> Result<[u8; MODULUS_BYTES]> {
    let bytes = value.to_bytes_le();
    if bytes.len() > MODULUS_BYTES {
        return Err(Error::Key(format!(
            "value needs {} bytes, limit is {MODULUS_BYTES}",
            bytes.len()
        )));
    }
    let mut out = [0u8; MODULUS_BYTES];
    out[..bytes.len()].copy_from_slice(&bytes);
    Ok(out)
}

/// Encode a 2048-bit RSA public key into Android's binary structure.
pub fn encode(key: &RsaPublicKey) -> Result<Vec<u8>> {
    let n = key.n();
    if n.bits() != MODULUS_BYTES * 8 {
        return Err(Error::Key(format!(
            "Android requires a 2048-bit modulus, got {} bits",
            n.bits()
        )));
    }
    if n.to_bytes_le().first().is_none_or(|b| b & 1 == 0) {
        return Err(Error::Key("modulus must be odd".into()));
    }
    let n_le = to_le_padded(n)?;
    let n0 = u32::from_le_bytes([n_le[0], n_le[1], n_le[2], n_le[3]]);
    let r = BigUint::from(1u8) << (MODULUS_BYTES * 8);
    let rr = to_le_padded(&((&r * &r) % n))?;
    let e_bytes = key.e().to_bytes_le();
    if e_bytes.len() > 4 {
        return Err(Error::Key("public exponent does not fit in 32 bits".into()));
    }
    let mut e_le = [0u8; 4];
    e_le[..e_bytes.len()].copy_from_slice(&e_bytes);
    let exponent = u32::from_le_bytes(e_le);

    let mut out = Vec::with_capacity(ENCODED_LEN);
    out.extend_from_slice(&MODULUS_WORDS.to_le_bytes());
    out.extend_from_slice(&n0inv(n0).to_le_bytes());
    out.extend_from_slice(&n_le);
    out.extend_from_slice(&rr);
    out.extend_from_slice(&exponent.to_le_bytes());
    Ok(out)
}

/// Decode Android's binary structure back into a public key, validating every field.
pub fn decode(bytes: &[u8]) -> Result<RsaPublicKey> {
    if bytes.len() != ENCODED_LEN {
        return Err(Error::Key(format!(
            "encoded key must be {ENCODED_LEN} bytes, got {}",
            bytes.len()
        )));
    }
    let words = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if words != MODULUS_WORDS {
        return Err(Error::Key(format!(
            "unexpected modulus length {words} words"
        )));
    }
    let n = BigUint::from_bytes_le(&bytes[8..8 + MODULUS_BYTES]);
    let e_at = 8 + 2 * MODULUS_BYTES;
    let e = u32::from_le_bytes([
        bytes[e_at],
        bytes[e_at + 1],
        bytes[e_at + 2],
        bytes[e_at + 3],
    ]);
    let key = RsaPublicKey::new(n, BigUint::from(e))?;
    let expected = encode(&key)?;
    if expected != bytes {
        return Err(Error::Key("n0inv or rr do not match the modulus".into()));
    }
    Ok(key)
}

/// The `adbkey.pub` line: base64 structure, a space, and a `user@host` comment.
pub fn encode_line(key: &RsaPublicKey, comment: &str) -> Result<String> {
    Ok(format!("{} {comment}", BASE64.encode(encode(key)?)))
}

/// Parse an `adbkey.pub` line (comment optional).
pub fn decode_line(line: &str) -> Result<RsaPublicKey> {
    let b64 = line
        .split_whitespace()
        .next()
        .ok_or_else(|| Error::Key("empty key line".into()))?;
    decode(&BASE64.decode(b64)?)
}

#[cfg(test)]
mod tests {
    use super::n0inv;

    #[test]
    fn n0inv_is_negative_inverse_mod_2_32() {
        for n0 in [1u32, 3, 0xFFFF_FFFF, 0x1234_5679, 0xDEAD_BEEF | 1] {
            let inv = n0inv(n0);
            assert_eq!(n0.wrapping_mul(inv), 0u32.wrapping_sub(1), "{n0:#x}");
        }
    }
}

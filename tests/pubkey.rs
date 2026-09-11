//! Android public-key encoding, cross-checked with an independent big-integer implementation.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use base64::Engine as _;
use common::host_key;
use num_bigint::BigUint;
use rsa::traits::PublicKeyParts as _;
use rsadb::auth::{HostKey, pubkey, verify_token};

/// `-n^-1 mod 2^32` by the extended Euclidean algorithm on `num-bigint`.
fn reference_n0inv(n: &BigUint) -> u32 {
    let modulus = BigUint::from(1u64) << 32usize;
    let n0 = n % &modulus;
    let inv = n0
        .modinv(&modulus)
        .expect("odd modulus is invertible mod 2^32");
    let neg = (&modulus - inv) % &modulus;
    neg.to_u32_digits().first().copied().unwrap_or(0)
}

/// `(2^2048)^2 mod n`, computed by `num-bigint`.
fn reference_rr(n: &BigUint) -> Vec<u8> {
    let r = BigUint::from(1u8) << 2048usize;
    let mut rr = ((&r * &r) % n).to_bytes_le();
    rr.resize(256, 0);
    rr
}

fn modulus_of(key: &HostKey) -> BigUint {
    BigUint::from_bytes_be(&key.public_key().n().to_bytes_be())
}

#[test]
fn encoding_matches_android_pubkey_layout() {
    let key = host_key();
    let n = modulus_of(key);
    let encoded = pubkey::encode(key.public_key()).unwrap();
    assert_eq!(encoded.len(), pubkey::ENCODED_LEN);
    assert_eq!(encoded.len(), 524);

    assert_eq!(
        &encoded[0..4],
        &64u32.to_le_bytes(),
        "len field is 64 words"
    );
    assert_eq!(&encoded[4..8], &reference_n0inv(&n).to_le_bytes(), "n0inv");
    let mut n_le = n.to_bytes_le();
    n_le.resize(256, 0);
    assert_eq!(&encoded[8..264], &n_le[..], "modulus little-endian");
    assert_eq!(&encoded[264..520], &reference_rr(&n)[..], "rr = R^2 mod n");
    assert_eq!(&encoded[520..524], &65537u32.to_le_bytes(), "exponent");

    let line = key.public_key_line().unwrap();
    let (b64, comment) = line.split_once(' ').unwrap();
    assert_eq!(b64.len(), 700, "524 bytes base64-encode to 700 characters");
    assert_eq!(comment, "test@rsadb");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap(),
        encoded
    );
}

#[test]
fn n0inv_agrees_with_reference_on_many_moduli() {
    for k in 0u32..2000 {
        let n0 = k.wrapping_mul(0x9E37_79B9) | 1;
        let n = BigUint::from(n0);
        assert_eq!(pubkey::n0inv(n0), reference_n0inv(&n), "{n0:#x}");
    }
}

#[test]
fn decode_roundtrips_and_validates() {
    let key = host_key();
    let encoded = pubkey::encode(key.public_key()).unwrap();
    assert_eq!(&pubkey::decode(&encoded).unwrap(), key.public_key());
    assert_eq!(
        &pubkey::decode_line(&key.public_key_line().unwrap()).unwrap(),
        key.public_key()
    );

    let mut wrong_rr = encoded.clone();
    wrong_rr[300] ^= 1;
    assert!(
        pubkey::decode(&wrong_rr).is_err(),
        "corrupted rr must not decode"
    );
    let mut wrong_len = encoded.clone();
    wrong_len[0] = 63;
    assert!(pubkey::decode(&wrong_len).is_err());
    assert!(pubkey::decode(&encoded[..523]).is_err());
    assert!(pubkey::decode_line("").is_err());
    assert!(pubkey::decode_line("not base64 at all!").is_err());
}

#[test]
fn token_signatures_verify_with_the_public_key() {
    let key = host_key();
    let token = [7u8; 20];
    let sig = key.sign_token(&token).unwrap();
    assert_eq!(sig.len(), 256);
    verify_token(key.public_key(), &token, &sig).unwrap();
    assert!(verify_token(key.public_key(), &[8u8; 20], &sig).is_err());
    assert!(verify_token(common::stranger_key().public_key(), &token, &sig).is_err());
    assert!(
        key.sign_token(&[0u8; 19]).is_err(),
        "token must be exactly 20 bytes"
    );
}

#[test]
fn pem_roundtrip_and_key_files() {
    let key = host_key();
    let pem = key.to_pem().unwrap();
    assert!(pem.starts_with("-----BEGIN RSA PRIVATE KEY-----"));
    let again = HostKey::from_pem(&pem, "x@y").unwrap();
    assert_eq!(again.public_key(), key.public_key());
    assert!(HostKey::from_pem("garbage", "x").is_err());

    let dir = tempfile::tempdir().unwrap();
    let paths = rsadb::auth::KeyPaths::in_dir(&dir.path().join("nested"));
    rsadb::auth::write_key(&paths, key).unwrap();
    let loaded = rsadb::auth::load_or_generate(&paths, "other@host").unwrap();
    assert_eq!(
        loaded.public_key(),
        key.public_key(),
        "existing key is reused"
    );
    let pub_line = std::fs::read_to_string(&paths.public).unwrap();
    assert_eq!(pub_line.trim_end(), key.public_key_line().unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&paths.private)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

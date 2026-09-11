//! Header encoding and decoding.

use super::{Command, Header, MAX_PAYLOAD};
use crate::error::{Error, Result};

/// Size of the fixed header in bytes.
pub const HEADER_LEN: usize = 24;

/// The payload checksum: the wrapping sum of all payload bytes.
pub fn checksum(payload: &[u8]) -> u32 {
    payload
        .iter()
        .fold(0u32, |acc, &b| acc.wrapping_add(u32::from(b)))
}

/// Serialise a header into its 24-byte wire form.
pub fn encode_header(header: &Header) -> [u8; HEADER_LEN] {
    let command = header.command.code();
    let mut out = [0u8; HEADER_LEN];
    out[0..4].copy_from_slice(&command.to_le_bytes());
    out[4..8].copy_from_slice(&header.arg0.to_le_bytes());
    out[8..12].copy_from_slice(&header.arg1.to_le_bytes());
    out[12..16].copy_from_slice(&header.data_length.to_le_bytes());
    out[16..20].copy_from_slice(&header.data_check.to_le_bytes());
    out[20..24].copy_from_slice(&(command ^ 0xFFFF_FFFF).to_le_bytes());
    out
}

fn word(bytes: &[u8; HEADER_LEN], at: usize) -> u32 {
    let mut w = [0u8; 4];
    w.copy_from_slice(&bytes[at..at + 4]);
    u32::from_le_bytes(w)
}

/// Parse a 24-byte header, checking the magic and the payload bound.
///
/// `max_payload` is the negotiated limit; anything larger is rejected before a
/// single payload byte is read so a hostile peer cannot make us allocate freely.
pub fn decode_header(bytes: &[u8; HEADER_LEN], max_payload: u32) -> Result<Header> {
    let command_code = word(bytes, 0);
    let magic = word(bytes, 20);
    if magic != command_code ^ 0xFFFF_FFFF {
        return Err(Error::protocol(format!(
            "bad magic {magic:#010x} for command {command_code:#010x}"
        )));
    }
    let command = Command::from_code(command_code)
        .ok_or_else(|| Error::protocol(format!("unknown command {command_code:#010x}")))?;
    let data_length = word(bytes, 12);
    let bound = max_payload.min(MAX_PAYLOAD);
    if data_length > bound {
        return Err(Error::protocol(format!(
            "payload length {data_length} exceeds limit {bound}"
        )));
    }
    Ok(Header {
        command,
        arg0: word(bytes, 4),
        arg1: word(bytes, 8),
        data_length,
        data_check: word(bytes, 16),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_magic() {
        let h = Header {
            command: Command::Open,
            arg0: 1,
            arg1: 0,
            data_length: 6,
            data_check: 99,
        };
        let bytes = encode_header(&h);
        assert_eq!(&bytes[0..4], b"OPEN");
        assert_eq!(decode_header(&bytes, MAX_PAYLOAD).ok(), Some(h));
        let mut bad = bytes;
        bad[23] ^= 1;
        assert!(matches!(
            decode_header(&bad, MAX_PAYLOAD),
            Err(Error::Protocol(_))
        ));
    }

    #[test]
    fn rejects_unknown_command_and_oversize() {
        let mut bytes = [0u8; HEADER_LEN];
        bytes[0..4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&(0x1234_5678u32 ^ 0xFFFF_FFFF).to_le_bytes());
        assert!(decode_header(&bytes, MAX_PAYLOAD).is_err());
        let big = Header {
            command: Command::Write,
            arg0: 0,
            arg1: 0,
            data_length: 4096,
            data_check: 0,
        };
        assert!(decode_header(&encode_header(&big), 1024).is_err());
        assert!(decode_header(&encode_header(&big), 4096).is_ok());
    }

    #[test]
    fn checksum_is_byte_sum() {
        assert_eq!(checksum(b""), 0);
        assert_eq!(checksum(&[1, 2, 3]), 6);
        assert_eq!(checksum(&[255; 4]), 1020);
    }
}

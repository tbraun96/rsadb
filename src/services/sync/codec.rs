//! Byte layouts of the `sync:` sub-protocol (`file_sync_protocol.h`).
//!
//! Every message starts with a four-character id. Requests are
//! `id, path_len: u32, path`; replies have fixed layouts described per id.

use super::entry::{Entry, Stat, StatDetail};
use crate::error::{Error, Result};
use bytes::{Bytes, BytesMut};

/// Four-character sync ids as little-endian words.
pub mod id {
    /// `LIST` — list a directory (v1 entries).
    pub const LIST: &[u8; 4] = b"LIST";
    /// `LIS2` — list a directory (v2 entries).
    pub const LIS2: &[u8; 4] = b"LIS2";
    /// `DENT` — v1 directory entry.
    pub const DENT: &[u8; 4] = b"DENT";
    /// `DNT2` — v2 directory entry.
    pub const DNT2: &[u8; 4] = b"DNT2";
    /// `STAT` — v1 lstat.
    pub const STAT: &[u8; 4] = b"STAT";
    /// `STA2` — v2 stat (follows symlinks).
    pub const STA2: &[u8; 4] = b"STA2";
    /// `LST2` — v2 lstat.
    pub const LST2: &[u8; 4] = b"LST2";
    /// `RECV` — pull a file (v1).
    pub const RECV: &[u8; 4] = b"RECV";
    /// `RCV2` — pull a file (v2, with flags).
    pub const RCV2: &[u8; 4] = b"RCV2";
    /// `SEND` — push a file (v1, `path,mode`).
    pub const SEND: &[u8; 4] = b"SEND";
    /// `SND2` — push a file (v2, with flags).
    pub const SND2: &[u8; 4] = b"SND2";
    /// `DATA` — a chunk of file content.
    pub const DATA: &[u8; 4] = b"DATA";
    /// `DONE` — end of transfer / listing.
    pub const DONE: &[u8; 4] = b"DONE";
    /// `OKAY` — success status.
    pub const OKAY: &[u8; 4] = b"OKAY";
    /// `FAIL` — failure status followed by a message.
    pub const FAIL: &[u8; 4] = b"FAIL";
    /// `QUIT` — end the sync session.
    pub const QUIT: &[u8; 4] = b"QUIT";
}

/// Largest `DATA` chunk either side sends.
pub const DATA_MAX: usize = 64 * 1024;
/// Size of a v1 stat reply.
pub const STAT_V1_LEN: usize = 16;
/// Size of a v2 stat reply.
pub const STAT_V2_LEN: usize = 72;
/// Size of a v1 directory entry header (also the size of the terminating `DONE`).
pub const DENT_V1_LEN: usize = 20;
/// Size of a v2 directory entry header (also the size of the terminating `DONE`).
pub const DENT_V2_LEN: usize = 76;

/// Encode `id, len, path`.
pub fn request(id: &[u8; 4], path: &str) -> Result<Bytes> {
    let len = u32::try_from(path.len())
        .map_err(|_| Error::InvalidArgument("path longer than u32::MAX".into()))?;
    let mut out = BytesMut::with_capacity(8 + path.len());
    out.extend_from_slice(id);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(path.as_bytes());
    Ok(out.freeze())
}

/// Encode an `id, u32` pair (`DATA` length, `DONE` mtime, `RCV2`/`SND2` flags, `QUIT`).
pub fn word(id: &[u8; 4], value: u32) -> Bytes {
    let mut out = BytesMut::with_capacity(8);
    out.extend_from_slice(id);
    out.extend_from_slice(&value.to_le_bytes());
    out.freeze()
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut w = [0u8; 8];
    w.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(w)
}

fn i64_at(b: &[u8], at: usize) -> i64 {
    i64::from_le_bytes(u64_at(b, at).to_le_bytes())
}

/// Parse the 12 bytes after `id` in a v1 stat or dent: `mode, size, mtime`.
pub fn stat_v1(b: &[u8]) -> Stat {
    Stat {
        mode: u32_at(b, 4),
        size: u64::from(u32_at(b, 8)),
        mtime: i64::from(u32_at(b, 12)),
        error: None,
        detail: None,
    }
}

/// Parse a 72-byte `STA2` reply (or the first 72 bytes of a `DNT2` entry).
pub fn stat_v2(b: &[u8]) -> Stat {
    let detail = StatDetail {
        dev: u64_at(b, 8),
        ino: u64_at(b, 16),
        nlink: u32_at(b, 28),
        uid: u32_at(b, 32),
        gid: u32_at(b, 36),
        atime: i64_at(b, 48),
        ctime: i64_at(b, 64),
    };
    Stat {
        mode: u32_at(b, 24),
        size: u64_at(b, 40),
        mtime: i64_at(b, 56),
        error: Some(u32_at(b, 4)),
        detail: Some(detail),
    }
}

/// Name length field of a `DENT` header.
pub fn dent_v1_namelen(b: &[u8]) -> usize {
    u32_at(b, 16) as usize
}

/// Name length field of a `DNT2` header.
pub fn dent_v2_namelen(b: &[u8]) -> usize {
    u32_at(b, 72) as usize
}

/// Build an [`Entry`] from a parsed stat and its name bytes.
pub fn entry(stat: &Stat, name: &[u8]) -> Entry {
    Entry {
        name: String::from_utf8_lossy(name).into_owned(),
        mode: stat.mode,
        size: stat.size,
        mtime: stat.mtime,
    }
}

/// Encode a v1 stat reply body (used by device emulators).
pub fn encode_stat_v1(id: &[u8; 4], stat: &Stat) -> Bytes {
    let mut out = BytesMut::with_capacity(STAT_V1_LEN);
    out.extend_from_slice(id);
    out.extend_from_slice(&stat.mode.to_le_bytes());
    out.extend_from_slice(&u32::try_from(stat.size).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(&u32::try_from(stat.mtime).unwrap_or(0).to_le_bytes());
    out.freeze()
}

/// Encode a v2 stat reply body (used by device emulators).
pub fn encode_stat_v2(id: &[u8; 4], stat: &Stat) -> Bytes {
    let d = stat.detail.unwrap_or_default();
    let mut out = BytesMut::with_capacity(STAT_V2_LEN);
    out.extend_from_slice(id);
    out.extend_from_slice(&stat.error.unwrap_or(0).to_le_bytes());
    out.extend_from_slice(&d.dev.to_le_bytes());
    out.extend_from_slice(&d.ino.to_le_bytes());
    out.extend_from_slice(&stat.mode.to_le_bytes());
    out.extend_from_slice(&d.nlink.to_le_bytes());
    out.extend_from_slice(&d.uid.to_le_bytes());
    out.extend_from_slice(&d.gid.to_le_bytes());
    out.extend_from_slice(&stat.size.to_le_bytes());
    out.extend_from_slice(&d.atime.to_le_bytes());
    out.extend_from_slice(&stat.mtime.to_le_bytes());
    out.extend_from_slice(&d.ctime.to_le_bytes());
    out.freeze()
}

//! Types returned by the `sync:` service.

/// A directory entry from `LIST`/`LIS2`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// File name (no directory component).
    pub name: String,
    /// Unix mode bits (`S_IFDIR`, permissions, …).
    pub mode: u32,
    /// Size in bytes (32-bit on `LIST`, 64-bit on `LIS2`).
    pub size: u64,
    /// Modification time, seconds since the epoch.
    pub mtime: i64,
}

/// Extra fields only `STA2`/`LIS2` provide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatDetail {
    /// Device number.
    pub dev: u64,
    /// Inode number.
    pub ino: u64,
    /// Hard-link count.
    pub nlink: u32,
    /// Owner uid.
    pub uid: u32,
    /// Owner gid.
    pub gid: u32,
    /// Access time, seconds since the epoch.
    pub atime: i64,
    /// Status-change time, seconds since the epoch.
    pub ctime: i64,
}

/// Result of `STAT`/`STA2`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    /// Unix mode bits; zero when the path does not exist (v1 has no error field).
    pub mode: u32,
    /// Size in bytes.
    pub size: u64,
    /// Modification time, seconds since the epoch.
    pub mtime: i64,
    /// `errno` from the device (`STA2` only); `Some(0)` means success.
    pub error: Option<u32>,
    /// Fields only `STA2` reports.
    pub detail: Option<StatDetail>,
}

/// File-type bits of a mode word.
pub const S_IFMT: u32 = 0o170_000;
/// Directory.
pub const S_IFDIR: u32 = 0o040_000;
/// Regular file.
pub const S_IFREG: u32 = 0o100_000;
/// Symbolic link.
pub const S_IFLNK: u32 = 0o120_000;

impl Stat {
    /// Whether the path exists (mode non-zero and no error reported).
    pub fn exists(&self) -> bool {
        self.mode != 0 && self.error.is_none_or(|e| e == 0)
    }

    /// Whether the entry is a directory.
    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }

    /// Whether the entry is a regular file.
    pub fn is_file(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }
}

impl Entry {
    /// Whether the entry is a directory.
    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }

    /// Whether the entry is a symbolic link.
    pub fn is_symlink(&self) -> bool {
        self.mode & S_IFMT == S_IFLNK
    }
}

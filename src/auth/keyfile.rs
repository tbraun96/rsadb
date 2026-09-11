//! Reading and writing `~/.android/adbkey` and `adbkey.pub`.
//!
//! This is the only file in `auth` that touches the filesystem.

use super::HostKey;
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// Where the private and public key files live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPaths {
    /// The PEM private key (`adbkey`).
    pub private: PathBuf,
    /// The Android-format public key (`adbkey.pub`).
    pub public: PathBuf,
}

impl KeyPaths {
    /// Both files inside `dir`, named as adb names them.
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            private: dir.join("adbkey"),
            public: dir.join("adbkey.pub"),
        }
    }
}

/// The location Google's adb uses: `$HOME/.android` (or `%USERPROFILE%\.android`).
///
/// Fails when neither variable is set rather than guessing.
pub fn default_key_paths() -> Result<KeyPaths> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| Error::Key("neither HOME nor USERPROFILE is set".into()))?;
    Ok(KeyPaths::in_dir(&PathBuf::from(home).join(".android")))
}

/// Load the key at `paths.private`.
pub fn read_key(paths: &KeyPaths, comment: &str) -> Result<HostKey> {
    let pem = std::fs::read_to_string(&paths.private)?;
    HostKey::from_pem(&pem, comment)
}

/// Write both files, creating the directory and restricting the private key to the owner.
pub fn write_key(paths: &KeyPaths, key: &HostKey) -> Result<()> {
    if let Some(dir) = paths.private.parent() {
        std::fs::create_dir_all(dir)?;
    }
    write_private(&paths.private, key.to_pem()?.as_bytes())?;
    std::fs::write(&paths.public, format!("{}\n", key.public_key_line()?))?;
    Ok(())
}

#[cfg(unix)]
fn write_private(path: &Path, pem: &[u8]) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(pem)?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &Path, pem: &[u8]) -> Result<()> {
    std::fs::write(path, pem)?;
    Ok(())
}

/// Reuse the key at `paths` if it exists, otherwise generate one and store it.
///
/// A device that has already accepted Google's `adbkey` therefore accepts us too.
pub fn load_or_generate(paths: &KeyPaths, comment: &str) -> Result<HostKey> {
    if paths.private.exists() {
        return read_key(paths, comment);
    }
    let key = HostKey::generate(comment)?;
    write_key(paths, &key)?;
    Ok(key)
}

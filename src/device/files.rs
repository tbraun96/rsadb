//! File-transfer helpers on [`Device`], built on the `sync:` service.

use super::Device;
use crate::channel::Connection;
use crate::error::{Error, Result};
use crate::services::sync::{Entry, Stat, SyncClient, SyncFeatures};
use bytes::Bytes;
use futures::{Stream, StreamExt as _};
use std::path::Path;
use std::time::UNIX_EPOCH;
use tokio::io::AsyncWriteExt as _;

/// Mode bits used for pushed files on platforms without Unix permission bits: `-rw-r--r--`.
pub const FALLBACK_PUSH_MODE: u32 = 0o100_644;

impl<C: Connection> Device<C> {
    /// Open a `sync:` client on this device.
    pub async fn sync(&self) -> Result<SyncClient<C::Channel>> {
        let features = SyncFeatures::from_features(self.features().await?);
        Ok(SyncClient::new(
            self.connection().open("sync:").await?,
            features,
        ))
    }

    /// List a directory.
    pub async fn list_dir(&self, path: &str) -> Result<Vec<Entry>> {
        let mut sync = self.sync().await?;
        let entries = sync.list(path).await?;
        sync.quit().await?;
        Ok(entries)
    }

    /// Stat a path.
    pub async fn stat(&self, path: &str) -> Result<Stat> {
        let mut sync = self.sync().await?;
        let stat = sync.stat(path).await?;
        sync.quit().await?;
        Ok(stat)
    }

    /// Pull a file as a stream of chunks (each at most 64 KiB).
    pub async fn pull(
        &self,
        remote: &str,
    ) -> Result<impl Stream<Item = Result<Bytes>> + Send + use<C>> {
        let mut sync = self.sync().await?;
        sync.start_pull(remote).await?;
        Ok(futures::stream::unfold(
            (sync, false),
            |(mut sync, done)| async move {
                if done {
                    return None;
                }
                match sync.next_data().await {
                    Ok(Some(chunk)) => Some((Ok(chunk), (sync, false))),
                    Ok(None) => None,
                    Err(e) => Some((Err(e), (sync, true))),
                }
            },
        ))
    }

    /// Pull a whole file into memory.
    pub async fn pull_bytes(&self, remote: &str) -> Result<Vec<u8>> {
        let mut stream = std::pin::pin!(self.pull(remote).await?);
        let mut out = Vec::new();
        while let Some(chunk) = stream.next().await {
            out.extend_from_slice(&chunk?);
        }
        Ok(out)
    }

    /// Pull a file to `local`, creating or truncating it. Returns the byte count.
    pub async fn pull_to_file(&self, remote: &str, local: &Path) -> Result<u64> {
        let mut stream = std::pin::pin!(self.pull(remote).await?);
        let mut file = tokio::fs::File::create(local).await?;
        let mut total = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            total += chunk.len() as u64;
        }
        file.flush().await?;
        Ok(total)
    }

    /// Push in-memory bytes to `remote` with the given mode and mtime.
    pub async fn push_bytes(&self, data: &[u8], remote: &str, mode: u32, mtime: u32) -> Result<()> {
        let mut sync = self.sync().await?;
        sync.push(remote, mode, mtime, data).await?;
        sync.quit().await
    }

    /// Push the file at `local` to `remote`, keeping its mtime and permission bits.
    pub async fn push_file(&self, local: &Path, remote: &str) -> Result<()> {
        let meta = tokio::fs::metadata(local).await?;
        if !meta.is_file() {
            return Err(Error::InvalidArgument(format!(
                "{} is not a regular file",
                local.display()
            )));
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .and_then(|d| u32::try_from(d.as_secs()).ok())
            .ok_or_else(|| Error::InvalidArgument("file mtime is not representable".into()))?;
        let file = tokio::fs::File::open(local).await?;
        let mut sync = self.sync().await?;
        sync.push(remote, mode_of(&meta), mtime, file).await?;
        sync.quit().await
    }
}

#[cfg(unix)]
fn mode_of(meta: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    meta.permissions().mode()
}

#[cfg(not(unix))]
fn mode_of(meta: &std::fs::Metadata) -> u32 {
    if meta.permissions().readonly() {
        0o100_444
    } else {
        FALLBACK_PUSH_MODE
    }
}

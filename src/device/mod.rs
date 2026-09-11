//! A typed, high-level view of one device.

mod files;

pub use files::FALLBACK_PUSH_MODE;

use crate::channel::Connection;
use crate::error::{Error, Result};
use crate::services::content::{self, Row};
use crate::services::{RebootTarget, ShellOutput, props, shell};
use bytes::Bytes;
use std::collections::BTreeMap;
use tokio::sync::OnceCell;

/// The eight-byte PNG signature.
const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];

/// Typed helpers over any [`Connection`] (a direct session or an adb-server client).
#[derive(Debug)]
pub struct Device<C> {
    conn: C,
    features: OnceCell<Vec<String>>,
}

impl<C: Connection> Device<C> {
    /// Wrap a connection.
    pub fn new(conn: C) -> Self {
        Self {
            conn,
            features: OnceCell::new(),
        }
    }

    /// The underlying connection, for services this type does not wrap.
    pub fn connection(&self) -> &C {
        &self.conn
    }

    /// Give the connection back.
    pub fn into_connection(self) -> C {
        self.conn
    }

    /// The device's advertised features (cached).
    pub async fn features(&self) -> Result<&[String]> {
        self.features
            .get_or_try_init(|| self.conn.features())
            .await
            .map(Vec::as_slice)
    }

    /// Whether the device advertised `feature`.
    pub async fn has_feature(&self, feature: &str) -> Result<bool> {
        Ok(self.features().await?.iter().any(|f| f == feature))
    }

    /// Run a shell command, using `shell,v2:` when available.
    pub async fn shell(&self, command: &str) -> Result<ShellOutput> {
        shell::shell(&self.conn, self.features().await?, command).await
    }

    /// Run a shell command with data on its stdin (`shell,v2:` only).
    pub async fn shell_with_stdin(&self, command: &str, stdin: &[u8]) -> Result<ShellOutput> {
        if !self.has_feature(shell::SHELL_V2_FEATURE).await? {
            return Err(Error::Unsupported(
                "device lacks shell_v2; stdin needs it".into(),
            ));
        }
        shell::shell_v2(&self.conn, command, stdin).await
    }

    /// Run a command through the legacy `shell:` service (merged output, no exit code).
    pub async fn shell_v1(&self, command: &str) -> Result<Bytes> {
        shell::shell_v1(&self.conn, command).await
    }

    /// Run a command through `exec:` and return its raw stdout.
    pub async fn exec(&self, command: &str) -> Result<Bytes> {
        shell::exec(&self.conn, command).await
    }

    /// Run a command and fail unless it exited 0 (when an exit code is available).
    async fn shell_checked(&self, command: &str) -> Result<ShellOutput> {
        let out = self.shell(command).await?;
        if out.success() {
            Ok(out)
        } else {
            Err(Error::RemoteFailure(format!(
                "{command:?} exited {}: {}",
                out.exit_code.unwrap_or(0),
                out.stderr_text().trim()
            )))
        }
    }

    /// Read one system property.
    pub async fn getprop(&self, name: &str) -> Result<String> {
        props::validate_name(name)?;
        let out = self.shell_checked(&format!("getprop {name}")).await?;
        Ok(out.stdout_text().trim_end_matches(['\r', '\n']).to_owned())
    }

    /// Read every system property.
    pub async fn getprops(&self) -> Result<BTreeMap<String, String>> {
        let out = self.shell_checked("getprop").await?;
        Ok(props::parse_all(&out.stdout_text()))
    }

    /// Take a screenshot as PNG bytes (`exec:screencap -p`).
    pub async fn screencap(&self) -> Result<Vec<u8>> {
        let png = self.exec("screencap -p").await?;
        if png.len() < PNG_MAGIC.len() || png[..PNG_MAGIC.len()] != PNG_MAGIC {
            return Err(Error::parse(format!(
                "screencap did not return a PNG ({} bytes: {:?})",
                png.len(),
                String::from_utf8_lossy(&png[..png.len().min(64)])
            )));
        }
        Ok(png.to_vec())
    }

    /// Query a content provider.
    ///
    /// `projection` must be non-empty; put free-text columns last because the
    /// `content` tool does not escape `, ` inside values. The row count is
    /// cross-checked against an `_id`-only query so a mis-parse surfaces as an
    /// error instead of silently dropped rows.
    pub async fn content_query(&self, uri: &str, projection: &[&str]) -> Result<Vec<Row>> {
        self.content_query_with(uri, projection, &[]).await
    }

    /// [`Self::content_query`] with extra `content query` arguments (`--where`, `--sort`, …).
    pub async fn content_query_with(
        &self,
        uri: &str,
        projection: &[&str],
        extra: &[&str],
    ) -> Result<Vec<Row>> {
        if projection.is_empty() {
            return Err(Error::InvalidArgument(
                "projection must name at least one column".into(),
            ));
        }
        let out = self
            .shell_checked(&content::command(uri, projection, extra))
            .await?;
        let rows = content::parse_rows(&out.stdout_text(), projection)?;
        let check = self
            .shell_checked(&content::command(uri, &["_id"], extra))
            .await?;
        let expected = content::count_rows(&check.stdout_text());
        if expected != rows.len() {
            return Err(Error::parse(format!(
                "parsed {} rows but the _id-only query reports {expected}",
                rows.len()
            )));
        }
        Ok(rows)
    }

    /// Reboot the device.
    pub async fn reboot(&self, target: RebootTarget) -> Result<()> {
        crate::services::reboot(&self.conn, target).await
    }
}

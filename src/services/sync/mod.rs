//! The `sync:` service: file listing, stat, pull and push.

pub mod codec;
mod entry;

pub use entry::{Entry, S_IFDIR, S_IFLNK, S_IFMT, S_IFREG, Stat, StatDetail};

use crate::channel::{Channel, ChannelReader, Connection};
use crate::error::{Error, Result};
use bytes::Bytes;
use codec::{DATA_MAX, DENT_V1_LEN, DENT_V2_LEN, STAT_V1_LEN, STAT_V2_LEN, id};
use futures::Stream;
use tokio::io::{AsyncRead, AsyncReadExt as _};

/// Which v2 variants the device supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncFeatures {
    /// `ls_v2`: 64-bit sizes and full stat in listings.
    pub ls_v2: bool,
    /// `stat_v2`: full stat with error codes.
    pub stat_v2: bool,
    /// `sendrecv_v2`: flag words on `RCV2`/`SND2`.
    pub sendrecv_v2: bool,
}

impl SyncFeatures {
    /// Derive from a device's advertised feature list.
    pub fn from_features(features: &[String]) -> Self {
        let has = |name: &str| features.iter().any(|f| f == name);
        Self {
            ls_v2: has("ls_v2"),
            stat_v2: has("stat_v2"),
            sendrecv_v2: has("sendrecv_v2"),
        }
    }
}

/// A client for one `sync:` stream. Requests are sequential.
pub struct SyncClient<C> {
    reader: ChannelReader<C>,
    features: SyncFeatures,
}

impl<C: Channel> SyncClient<C> {
    /// Wrap an already-open `sync:` channel.
    pub fn new(channel: C, features: SyncFeatures) -> Self {
        Self {
            reader: ChannelReader::new(channel),
            features,
        }
    }

    /// Open `sync:` on `conn`, choosing variants from the device's features.
    pub async fn open<K: Connection<Channel = C>>(conn: &K) -> Result<Self> {
        let features = SyncFeatures::from_features(&conn.features().await?);
        Ok(Self::new(conn.open("sync:").await?, features))
    }

    async fn send(&mut self, bytes: Bytes) -> Result<()> {
        self.reader.channel().send(bytes).await
    }

    async fn read_id(&mut self) -> Result<[u8; 4]> {
        let b = self.reader.read_exact(4).await?;
        Ok([b[0], b[1], b[2], b[3]])
    }

    async fn fail(&mut self) -> Error {
        match self.reader.read_u32().await {
            Ok(len) => match self.reader.read_exact(len as usize).await {
                Ok(msg) => Error::RemoteFailure(String::from_utf8_lossy(&msg).into_owned()),
                Err(e) => e,
            },
            Err(e) => e,
        }
    }

    /// Read an `OKAY`/`FAIL` status word.
    async fn read_status(&mut self) -> Result<()> {
        match &self.read_id().await? {
            id::OKAY => {
                self.reader.read_u32().await?;
                Ok(())
            }
            id::FAIL => Err(self.fail().await),
            other => Err(Error::protocol(format!(
                "unexpected sync reply {}",
                String::from_utf8_lossy(other)
            ))),
        }
    }

    /// `STAT`/`STA2` on `path`.
    pub async fn stat(&mut self, path: &str) -> Result<Stat> {
        if self.features.stat_v2 {
            self.send(codec::request(id::STA2, path)?).await?;
            let b = self.reader.read_exact(STAT_V2_LEN).await?;
            expect_id(&b, *id::STA2)?;
            return Ok(codec::stat_v2(&b));
        }
        self.send(codec::request(id::STAT, path)?).await?;
        let b = self.reader.read_exact(STAT_V1_LEN).await?;
        expect_id(&b, *id::STAT)?;
        Ok(codec::stat_v1(&b))
    }

    /// `LIST`/`LIS2` on `path`.
    pub async fn list(&mut self, path: &str) -> Result<Vec<Entry>> {
        let (req, dent, header_len) = if self.features.ls_v2 {
            (id::LIS2, id::DNT2, DENT_V2_LEN)
        } else {
            (id::LIST, id::DENT, DENT_V1_LEN)
        };
        self.send(codec::request(req, path)?).await?;
        let mut entries = Vec::new();
        loop {
            let b = self.reader.read_exact(header_len).await?;
            if &b[..4] == id::DONE {
                return Ok(entries);
            }
            if &b[..4] == id::FAIL {
                return Err(Error::RemoteFailure(
                    String::from_utf8_lossy(&b[8..])
                        .trim_end_matches('\0')
                        .to_owned(),
                ));
            }
            expect_id(&b, *dent)?;
            let (stat, namelen) = if self.features.ls_v2 {
                (codec::stat_v2(&b), codec::dent_v2_namelen(&b))
            } else {
                (codec::stat_v1(&b), codec::dent_v1_namelen(&b))
            };
            let name = self.reader.read_exact(namelen).await?;
            entries.push(codec::entry(&stat, &name));
        }
    }

    /// Next `DATA` chunk of a running pull, `None` on `DONE`.
    pub async fn next_data(&mut self) -> Result<Option<Bytes>> {
        match &self.read_id().await? {
            id::DATA => {
                let len = self.reader.read_u32().await? as usize;
                if len > DATA_MAX {
                    return Err(Error::protocol(format!(
                        "DATA chunk of {len} bytes exceeds {DATA_MAX}"
                    )));
                }
                Ok(Some(self.reader.read_exact(len).await?))
            }
            id::DONE => {
                self.reader.read_u32().await?;
                Ok(None)
            }
            id::FAIL => Err(self.fail().await),
            other => Err(Error::protocol(format!(
                "unexpected sync reply {}",
                String::from_utf8_lossy(other)
            ))),
        }
    }

    /// Send the request that starts a pull; follow with [`Self::next_data`] until `None`.
    pub async fn start_pull(&mut self, path: &str) -> Result<()> {
        if self.features.sendrecv_v2 {
            self.send(codec::request(id::RCV2, path)?).await?;
            self.send(codec::word(id::RCV2, 0)).await
        } else {
            self.send(codec::request(id::RECV, path)?).await
        }
    }

    /// Pull `path` as a stream of chunks.
    pub async fn pull(&mut self, path: &str) -> Result<impl Stream<Item = Result<Bytes>> + '_> {
        self.start_pull(path).await?;
        Ok(futures::stream::unfold(
            (self, false),
            |(client, done)| async move {
                if done {
                    return None;
                }
                match client.next_data().await {
                    Ok(Some(chunk)) => Some((Ok(chunk), (client, false))),
                    Ok(None) => None,
                    Err(e) => Some((Err(e), (client, true))),
                }
            },
        ))
    }

    /// Push `source` to `path` with the given mode bits and mtime.
    pub async fn push<R: AsyncRead + Unpin + Send>(
        &mut self,
        path: &str,
        mode: u32,
        mtime: u32,
        mut source: R,
    ) -> Result<()> {
        if self.features.sendrecv_v2 {
            self.send(codec::request(id::SND2, path)?).await?;
            let mut flags = codec::word(id::SND2, mode).to_vec();
            flags.extend_from_slice(&0u32.to_le_bytes());
            self.send(Bytes::from(flags)).await?;
        } else {
            self.send(codec::request(id::SEND, &format!("{path},{mode}"))?)
                .await?;
        }
        let mut buf = vec![0u8; DATA_MAX];
        loop {
            let n = source.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            let len = u32::try_from(n).map_err(|_| Error::protocol("chunk too large"))?;
            let mut chunk = codec::word(id::DATA, len).to_vec();
            chunk.extend_from_slice(&buf[..n]);
            self.send(Bytes::from(chunk)).await?;
        }
        self.send(codec::word(id::DONE, mtime)).await?;
        self.read_status().await
    }

    /// End the sync session politely.
    pub async fn quit(mut self) -> Result<()> {
        self.send(codec::word(id::QUIT, 0)).await?;
        self.reader.channel().close().await
    }
}

fn expect_id(b: &[u8], want: [u8; 4]) -> Result<()> {
    if b[..4] == want {
        Ok(())
    } else {
        Err(Error::protocol(format!(
            "expected {} got {}",
            String::from_utf8_lossy(&want),
            String::from_utf8_lossy(&b[..4])
        )))
    }
}

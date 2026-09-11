//! An in-memory filesystem for the fake device's `sync:` service.

use rsadb::services::sync::{S_IFDIR, S_IFREG};
use rsadb::services::{Stat, StatDetail};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    File {
        mode: u32,
        mtime: i64,
        data: Vec<u8>,
    },
    Dir {
        mode: u32,
        mtime: i64,
    },
}

#[derive(Debug, Clone, Default)]
pub struct Fs {
    nodes: BTreeMap<String, Node>,
}

impl Fs {
    pub fn new() -> Self {
        let mut fs = Self::default();
        fs.mkdir("/", 0o755, 0);
        fs.mkdir("/sdcard", 0o771, 1_700_000_000);
        fs
    }

    pub fn mkdir(&mut self, path: &str, perm: u32, mtime: i64) {
        self.nodes.insert(
            path.to_owned(),
            Node::Dir {
                mode: S_IFDIR | perm,
                mtime,
            },
        );
    }

    pub fn add_file(&mut self, path: &str, perm: u32, mtime: i64, data: &[u8]) {
        self.nodes.insert(
            path.to_owned(),
            Node::File {
                mode: S_IFREG | perm,
                mtime,
                data: data.to_vec(),
            },
        );
    }

    pub fn get(&self, path: &str) -> Option<&Node> {
        self.nodes.get(path.trim_end_matches('/').max("/"))
    }

    pub fn stat(&self, path: &str) -> Option<Stat> {
        let (mode, mtime, size) = match self.get(path)? {
            Node::File { mode, mtime, data } => (*mode, *mtime, data.len() as u64),
            Node::Dir { mode, mtime } => (*mode, *mtime, 4096),
        };
        Some(Stat {
            mode,
            size,
            mtime,
            error: Some(0),
            detail: Some(StatDetail {
                dev: 1,
                ino: 7,
                nlink: 1,
                uid: 2000,
                gid: 2000,
                atime: mtime,
                ctime: mtime,
            }),
        })
    }

    /// Direct children of `dir` as `(name, stat)`.
    pub fn list(&self, dir: &str) -> Option<Vec<(String, Stat)>> {
        let dir = dir.trim_end_matches('/');
        match self.get(if dir.is_empty() { "/" } else { dir })? {
            Node::Dir { .. } => {}
            Node::File { .. } => return None,
        }
        let prefix = format!("{dir}/");
        let mut out = Vec::new();
        for path in self.nodes.keys() {
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            if rest.is_empty() || rest.contains('/') {
                continue;
            }
            if let Some(stat) = self.stat(path) {
                out.push((rest.to_owned(), stat));
            }
        }
        Some(out)
    }

    pub fn read(&self, path: &str) -> Option<&[u8]> {
        match self.get(path)? {
            Node::File { data, .. } => Some(data),
            Node::Dir { .. } => None,
        }
    }

    /// Write a file; fails when the parent directory does not exist.
    pub fn write(
        &mut self,
        path: &str,
        mode: u32,
        mtime: i64,
        data: Vec<u8>,
    ) -> Result<(), String> {
        let parent = path
            .rsplit_once('/')
            .map(|(p, _)| if p.is_empty() { "/" } else { p });
        match parent.and_then(|p| self.get(p)) {
            Some(Node::Dir { .. }) => {}
            _ => {
                return Err(format!(
                    "couldn't create file: No such file or directory ({path})"
                ));
            }
        }
        self.nodes.insert(
            path.to_owned(),
            Node::File {
                mode: S_IFREG | (mode & 0o777),
                mtime,
                data,
            },
        );
        Ok(())
    }
}

//! Device side of the `sync:` service over the in-memory filesystem.

use super::fs::Fs;
use super::services::ServiceHandler;
use bytes::{Bytes, BytesMut};
use rsadb::services::sync::codec::{self, DATA_MAX, id};
use std::sync::{Arc, Mutex};

enum State {
    Idle,
    Receiving {
        path: String,
        mode: u32,
        data: Vec<u8>,
    },
    AwaitRecvSetup {
        path: String,
    },
    AwaitSendSetup {
        path: String,
    },
}

pub struct SyncHandler {
    fs: Arc<Mutex<Fs>>,
    buffer: BytesMut,
    state: State,
    done: bool,
}

impl SyncHandler {
    pub fn new(fs: Arc<Mutex<Fs>>) -> Self {
        Self {
            fs,
            buffer: BytesMut::new(),
            state: State::Idle,
            done: false,
        }
    }

    fn fail(msg: &str) -> Bytes {
        let mut out = codec::word(id::FAIL, msg.len() as u32).to_vec();
        out.extend_from_slice(msg.as_bytes());
        Bytes::from(out)
    }

    fn u32_at(&self, at: usize) -> u32 {
        u32::from_le_bytes([
            self.buffer[at],
            self.buffer[at + 1],
            self.buffer[at + 2],
            self.buffer[at + 3],
        ])
    }

    /// Try to consume one request; returns replies and whether progress was made.
    fn step(&mut self) -> (Vec<Bytes>, bool) {
        if self.buffer.len() < 8 {
            return (Vec::new(), false);
        }
        let head: [u8; 4] = [
            self.buffer[0],
            self.buffer[1],
            self.buffer[2],
            self.buffer[3],
        ];
        let arg = self.u32_at(4) as usize;
        match std::mem::replace(&mut self.state, State::Idle) {
            State::AwaitRecvSetup { path } => {
                let _ = self.buffer.split_to(8);
                (self.recv(&path), true)
            }
            State::AwaitSendSetup { path } => {
                if self.buffer.len() < 12 {
                    self.state = State::AwaitSendSetup { path };
                    return (Vec::new(), false);
                }
                let mode = self.u32_at(4);
                let _ = self.buffer.split_to(12);
                self.state = State::Receiving {
                    path,
                    mode,
                    data: Vec::new(),
                };
                (Vec::new(), true)
            }
            State::Receiving {
                path,
                mode,
                mut data,
            } => {
                if &head == id::DONE {
                    let _ = self.buffer.split_to(8);
                    let reply = match self.fs.lock().unwrap().write(&path, mode, arg as i64, data) {
                        Ok(()) => codec::word(id::OKAY, 0),
                        Err(e) => Self::fail(&e),
                    };
                    return (vec![reply], true);
                }
                if &head != id::DATA || self.buffer.len() < 8 + arg {
                    self.state = State::Receiving { path, mode, data };
                    return (Vec::new(), false);
                }
                let _ = self.buffer.split_to(8);
                data.extend_from_slice(&self.buffer.split_to(arg));
                self.state = State::Receiving { path, mode, data };
                (Vec::new(), true)
            }
            State::Idle => {
                if &head == id::QUIT {
                    self.done = true;
                    return (Vec::new(), false);
                }
                if self.buffer.len() < 8 + arg {
                    return (Vec::new(), false);
                }
                let _ = self.buffer.split_to(8);
                let path = String::from_utf8_lossy(&self.buffer.split_to(arg)).into_owned();
                (self.request(&head, path), true)
            }
        }
    }

    fn request(&mut self, head: &[u8; 4], path: String) -> Vec<Bytes> {
        let fs = self.fs.lock().unwrap();
        match head {
            id::STAT | id::STA2 | id::LST2 => {
                let stat = fs.stat(&path);
                let v2 = head != id::STAT;
                vec![match (v2, stat) {
                    (false, Some(s)) => codec::encode_stat_v1(id::STAT, &s),
                    (false, None) => codec::encode_stat_v1(id::STAT, &empty_stat()),
                    (true, Some(s)) => codec::encode_stat_v2(head, &s),
                    (true, None) => codec::encode_stat_v2(head, &missing_stat()),
                }]
            }
            id::LIST | id::LIS2 => {
                let v2 = head == id::LIS2;
                let (dent, done_len) = if v2 {
                    (id::DNT2, codec::DENT_V2_LEN)
                } else {
                    (id::DENT, codec::DENT_V1_LEN)
                };
                let mut out = Vec::new();
                for (name, stat) in fs.list(&path).unwrap_or_default() {
                    let mut msg = if v2 {
                        codec::encode_stat_v2(dent, &stat)
                    } else {
                        codec::encode_stat_v1(dent, &stat)
                    }
                    .to_vec();
                    msg.extend_from_slice(&(name.len() as u32).to_le_bytes());
                    msg.extend_from_slice(name.as_bytes());
                    out.push(Bytes::from(msg));
                }
                let mut done = id::DONE.to_vec();
                done.resize(done_len, 0);
                out.push(Bytes::from(done));
                out
            }
            id::RECV => {
                drop(fs);
                self.recv(&path)
            }
            id::RCV2 => {
                self.state = State::AwaitRecvSetup { path };
                Vec::new()
            }
            id::SEND => {
                let (file, mode) = path.rsplit_once(',').unwrap_or((&path, "420"));
                let mode = mode.parse().unwrap_or(0o644);
                self.state = State::Receiving {
                    path: file.to_owned(),
                    mode,
                    data: Vec::new(),
                };
                Vec::new()
            }
            id::SND2 => {
                self.state = State::AwaitSendSetup { path };
                Vec::new()
            }
            other => vec![Self::fail(&format!(
                "unknown request {}",
                String::from_utf8_lossy(other)
            ))],
        }
    }

    fn recv(&self, path: &str) -> Vec<Bytes> {
        let fs = self.fs.lock().unwrap();
        let Some(data) = fs.read(path) else {
            return vec![Self::fail(&format!(
                "open failed: No such file or directory ({path})"
            ))];
        };
        let mut out: Vec<Bytes> = data
            .chunks(DATA_MAX)
            .map(|c| {
                let mut m = codec::word(id::DATA, c.len() as u32).to_vec();
                m.extend_from_slice(c);
                Bytes::from(m)
            })
            .collect();
        out.push(codec::word(id::DONE, 0));
        out
    }
}

fn empty_stat() -> rsadb::services::Stat {
    rsadb::services::Stat {
        mode: 0,
        size: 0,
        mtime: 0,
        error: None,
        detail: None,
    }
}

fn missing_stat() -> rsadb::services::Stat {
    rsadb::services::Stat {
        mode: 0,
        size: 0,
        mtime: 0,
        error: Some(2),
        detail: None,
    }
}

impl ServiceHandler for SyncHandler {
    fn on_open(&mut self) -> Vec<Bytes> {
        Vec::new()
    }

    fn on_data(&mut self, data: &[u8]) -> Vec<Bytes> {
        self.buffer.extend_from_slice(data);
        let mut out = Vec::new();
        loop {
            let (replies, progressed) = self.step();
            out.extend(replies);
            if !progressed {
                return out;
            }
        }
    }

    fn finished(&self) -> bool {
        self.done
    }
}

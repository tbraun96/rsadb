//! `FakeDevice`: the device side of the ADB protocol, in process.
//!
//! It is driven through the public [`Transport`] trait, so the same engine
//! runs over an in-memory duplex pipe or a real TCP socket.

pub mod engine;
pub mod fs;
pub mod services;
pub mod shell;
pub mod sync;

use rsa::RsaPublicKey;
use rsadb::Transport;
use rsadb::session::Banner;
use rsadb::transport::StreamTransport;
use rsadb::wire::{MAX_PAYLOAD, Message, VERSION};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::io::DuplexStream;
use tokio::task::JoinHandle;

/// How the emulated device behaves.
pub struct FakeConfig {
    pub trusted: Vec<RsaPublicKey>,
    pub accept_new_keys: bool,
    pub require_auth: bool,
    pub request_tls: bool,
    pub version: u32,
    pub max_payload: u32,
    pub banner: Banner,
    pub props: BTreeMap<String, String>,
    pub screencap_png: Vec<u8>,
    pub content_rows: Vec<BTreeMap<String, String>>,
    pub content_extra_ids: usize,
    pub refuse_services: Vec<String>,
}

impl Default for FakeConfig {
    fn default() -> Self {
        let mut props = BTreeMap::new();
        props.insert("ro.product.model".into(), "Fake Phone".into());
        props.insert("ro.build.version.sdk".into(), "34".into());
        props.insert("persist.multi".into(), "one\ntwo".into());
        let mut banner = Banner::parse(
            "device::ro.product.name=fake;ro.product.model=Fake;ro.product.device=fake",
        );
        banner.features = ["shell_v2", "cmd", "stat_v2", "ls_v2", "sendrecv_v2"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let mut row = BTreeMap::new();
        row.insert("_id".into(), "1".into());
        row.insert("address".into(), "+15550001".into());
        row.insert("body".into(), "Hello, world, with, commas".into());
        Self {
            trusted: Vec::new(),
            accept_new_keys: false,
            require_auth: true,
            request_tls: false,
            version: VERSION,
            max_payload: MAX_PAYLOAD,
            banner,
            props,
            screencap_png: super::PNG.to_vec(),
            content_rows: vec![row],
            content_extra_ids: 0,
            refuse_services: Vec::new(),
        }
    }
}

impl FakeConfig {
    pub fn trusting(key: &rsadb::HostKey) -> Self {
        Self {
            trusted: vec![key.public_key().clone()],
            ..Self::default()
        }
    }

    pub fn legacy() -> Self {
        let mut config = Self {
            require_auth: false,
            version: 0x0100_0000,
            max_payload: 0,
            ..Self::default()
        };
        config.banner.features = vec![];
        config
    }
}

/// State the tests can inspect while the device runs.
pub struct Shared {
    pub config: Arc<FakeConfig>,
    pub fs: Arc<Mutex<fs::Fs>>,
    pub received: Mutex<Vec<Message>>,
    pub accepted_key: Mutex<Option<RsaPublicKey>>,
    pub rebooted: Mutex<Vec<String>>,
}

impl Shared {
    pub fn new(config: FakeConfig) -> Arc<Self> {
        let mut fs = fs::Fs::new();
        fs.add_file(
            "/sdcard/hello.txt",
            0o644,
            1_700_000_001,
            b"hello from the device\n",
        );
        fs.add_file("/sdcard/big.bin", 0o600, 1_700_000_002, &big_file());
        fs.mkdir("/sdcard/Download", 0o771, 1_700_000_003);
        Arc::new(Self {
            config: Arc::new(config),
            fs: Arc::new(Mutex::new(fs)),
            received: Mutex::new(Vec::new()),
            accepted_key: Mutex::new(None),
            rebooted: Mutex::new(Vec::new()),
        })
    }
}

/// 200 000 bytes with a recognisable pattern (spans several 64 KiB chunks).
pub fn big_file() -> Vec<u8> {
    (0..200_000u32)
        .map(|i| (i.wrapping_mul(31) % 251) as u8)
        .collect()
}

/// A running fake device and the host end of its transport.
pub struct FakeDevice {
    pub shared: Arc<Shared>,
    pub task: JoinHandle<rsadb::Result<()>>,
}

/// Start the device on an in-memory pipe; returns the host-side transport.
pub fn spawn(config: FakeConfig) -> (FakeDevice, StreamTransport<DuplexStream>) {
    let (host_end, device_end) = tokio::io::duplex(256 * 1024);
    let shared = Shared::new(config);
    let device = spawn_on(Arc::clone(&shared), StreamTransport::new(device_end));
    (device, StreamTransport::new(host_end))
}

/// Start the device on any transport.
pub fn spawn_on<T: Transport>(shared: Arc<Shared>, transport: T) -> FakeDevice {
    let task = tokio::spawn(engine::run(Arc::clone(&shared), transport));
    FakeDevice { shared, task }
}

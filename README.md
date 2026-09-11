# rsadb

[![CI](https://github.com/tbraun96/rsadb/actions/workflows/ci.yml/badge.svg)](https://github.com/tbraun96/rsadb/actions/workflows/ci.yml)
[![docs.rs](https://img.shields.io/docsrs/rsadb)](https://docs.rs/rsadb)
[![crates.io](https://img.shields.io/crates/v/rsadb)](https://crates.io/crates/rsadb)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#licence)
![MSRV 1.85](https://img.shields.io/badge/MSRV-1.85-informational)

A pure-Rust implementation of the Android Debug Bridge (ADB) client
protocol. It talks to `adbd` on the phone directly over USB or TCP: no
`adb` binary, no adb server, no libusb, no C toolchain.

## Why pure Rust

* **No `adb` binary or daemon.** Your program is the ADB host. Nothing to
  install, nothing on port 5037 to fight over, no version skew between your
  code and platform-tools.
* **No libusb, no C.** USB goes through [`nusb`](https://crates.io/crates/nusb),
  which speaks IOKit, usbfs and WinUSB in Rust. The crate is
  `#![forbid(unsafe_code)]` and every dependency is Rust under MIT/Apache/BSD
  licences.
* **Async, typed, small.** Tokio throughout; `Result`s everywhere; the
  library core has no platform code and compiles for targets without USB.
* **Still cooperative.** If Android Studio or Google's adb server already
  holds the USB interface, the `host-client` feature talks to that server
  with the same `Device` API.

## Comparison

| | rsadb | Google `adb` | `adb_client` | `mozdevice` | `forensic-adb` |
|---|---|---|---|---|---|
| Needs an adb server running | no | is one | no (USB) / yes (TCP) | yes | yes |
| Direct USB to the phone | yes (`nusb`) | yes | yes (`rusb`/libusb) | no | no |
| Pure Rust, no C linked | yes | no | no (libusb) | yes | yes |
| Async | tokio | n/a | blocking | blocking | tokio |
| RSA host-key auth | yes | yes | yes | via server | via server |
| Shell v2 (stderr + exit code) | yes | yes | not documented | no | no |
| `sync:` pull/push/list/stat | yes | yes | yes | yes | yes |
| Licence | MIT/Apache-2.0 | Apache-2.0 | MIT | MPL-2.0 | MPL-2.0 |

Rows for other crates reflect their public documentation at the time of
writing; corrections are welcome.

## Quick start

### Library

```toml
[dependencies]
rsadb = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

```rust,no_run
use rsadb::{auth, transport::usb, Device, Session};
use std::time::Duration;

#[tokio::main]
async fn main() -> rsadb::Result<()> {
    // Reuse ~/.android/adbkey if Google's adb already made one, else create it.
    let key = auth::load_or_generate(&auth::default_key_paths()?, "me@laptop")?;

    // First (or only) ADB device on the bus; pass Some(serial) to choose.
    let info = usb::find(None).await?;
    let transport = usb::open(&info).await?;

    // CNXN + AUTH; waits up to 60 s for the "Allow USB debugging?" tap.
    let session = Session::connect(transport, &key, Duration::from_secs(60)).await?;
    let device = Device::new(session);

    println!("{}", device.getprop("ro.product.model").await?);
    let out = device.shell("ls -l /sdcard").await?;
    print!("{}", out.stdout_text());
    std::fs::write("screen.png", device.screencap().await?)?;
    Ok(())
}
```

Over TCP (`adb tcpip 5555` on the phone, or an emulator):

```rust,no_run
# async fn demo(key: rsadb::HostKey) -> rsadb::Result<()> {
let transport = rsadb::transport::tcp::connect(("192.168.1.20", 5555)).await?;
let session = rsadb::Session::connect(transport, &key, std::time::Duration::from_secs(60)).await?;
# Ok(()) }
```

Through a running Google adb server (when something else owns the USB
interface):

```rust,no_run
# async fn demo() -> rsadb::Result<()> {
let server = rsadb::host::HostClient::local();
let device = rsadb::Device::new(server.only_device().await?);
println!("{:?}", device.list_dir("/sdcard").await?);
# Ok(()) }
```

### CLI

```sh
cargo install rsadb
rsadb devices
rsadb shell getprop ro.build.version.release
rsadb pull /sdcard/DCIM/Camera/IMG_0001.jpg .
rsadb push ./app-debug.apk /data/local/tmp/
rsadb screencap screen.png
rsadb -t 192.168.1.20 shell id       # TCP
rsadb --server devices               # via Google's adb server
rsadb keygen                         # print ~/.android/adbkey.pub, creating it if needed
```

Exit codes: 0 ok, 1 failure, 2 usage, 3 no device, 4 device has not
authorised this key, 5 USB interface held by another process, 6 unsupported
by the device, 7 the device reported a failure.

## API tour

| Type / module | What it does |
|---|---|
| `wire` | `Message`, `Header`, `Command`, header encode/decode, checksum |
| `auth::HostKey` | generate / load PEM, sign tokens, `adbkey.pub` line |
| `auth::pubkey` | Android's `RSAPublicKey` layout (`n0inv`, `rr`) encode/decode |
| `transport::usb` | `list()`, `find(serial)`, `open()` → `UsbTransport` |
| `transport::tcp` | `connect(addr)` → `TcpTransport`; `parse_endpoint("host[:port]")` |
| `transport::StreamTransport` | frame any `AsyncRead + AsyncWrite` (used by the tests) |
| `Session` | handshake, `open_stream(service)`, banner, negotiated limits |
| `Stream` | one multiplexed ADB stream implementing `Channel` |
| `Channel`, `Connection` | the traits services are written against |
| `services::shell` | `shell_v1`, `shell_v2`, `exec`, `quote` |
| `services::sync::SyncClient` | `list`, `stat`, `pull` (stream of chunks), `push`, `quit` |
| `services::content` | `content query` command building and row parsing |
| `Device<C>` | `shell`, `exec`, `getprop(s)`, `screencap`, `list_dir`, `stat`, `pull*`, `push*`, `content_query`, `reboot` |
| `host::HostClient` | `version`, `devices`, `track_devices`, `device(serial)` → `Connection` |
| `track` | `watch_usb()` (hotplug) and `poll_usb(interval)` attach/detach streams |

Feature flags: `usb`, `tcp`, `host-client`, `cli` (all default), `hardware`
(enables the ignored real-device tests).

## Protocol notes

The wire format, handshake, flow control, `shell,v2` framing, the `sync:`
sub-protocol and the `adbkey.pub` layout are written up in
[docs/protocol.md](docs/protocol.md); the module structure in
[docs/architecture.md](docs/architecture.md). Highlights:

* Version `0x01000001`, 1 MiB payloads, checksums only for legacy peers.
* One `WRTE` in flight per stream; incoming payloads are acknowledged after
  the caller consumes them, so back-pressure reaches the phone.
* The public key is encoded exactly as `android_pubkey.c` does; the tests
  recompute `n0inv` and `R² mod n` with `num-bigint`, and the encoding of an
  existing Google-generated `adbkey` matches `adbkey.pub` byte for byte.

## Security notes

* **Host key.** `~/.android/adbkey` is a 2048-bit RSA private key; rsadb
  creates it with mode 0600 and never transmits it. Anyone holding it can
  act as your computer towards every phone that trusted it, so treat it
  like an SSH key. Pass `auth::KeyPaths` to use a different location.
* **The prompt.** A phone that does not know the key shows "Allow USB
  debugging?" with the key's fingerprint. rsadb sends the key and waits for
  the time you give it; `Error::Unauthorized` means nobody tapped Allow.
  Only accept prompts you expected.
* **Command lines.** `Device::shell` runs whatever string you pass through
  `/system/bin/sh` on the phone. Property names are validated, and
  `content_query` quotes its arguments, but for your own commands use
  `services::shell::quote` on untrusted pieces.
* **Bounds.** Payload lengths are checked against the negotiated limit
  before allocation; sync `DATA` chunks are capped at 64 KiB.
* **Timing side channel in `rsa`.** [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071)
  affects the `rsa` crate. rsadb performs one signature per connection with
  a local key and exposes no repeated-timing oracle; see `deny.toml` and
  `SECURITY.md`.

## Limitations and roadmap

* **`A_STLS` / TLS authentication** (Android 11+ wireless debugging with
  pairing) is detected and reported as `Error::Unsupported`. USB and
  `adb tcpip` connections on those devices use RSA and work.
* **`abb` / `abb_exec`** (binder-level `cmd` transport) is advertised in the
  banner but not wrapped; use `shell`.
* **Port forwarding / reverse** (`forward:`, `reverse:`) are not implemented.
* **Hotplug on Windows** goes through `nusb::watch_devices`; `poll_usb` is
  the fallback everywhere.
* **No background server.** `rsadb connect` verifies a TCP device and
  exits; each process owns its own session.
* Compression flags on `RCV2`/`SND2` are always zero (uncompressed).

## FAQ

**Do I need to stop Google's adb server?** For direct USB, yes: only one
process can claim the interface (`Error::ClaimFailed` tells you). Or keep
it running and use `--server` / `HostClient` instead.

**Do I need udev rules on Linux?** The same ones adb needs: your user must
be able to open the USB device node.

**Will my phone ask again?** Not if you reuse `~/.android/adbkey`; that is
the default and the encoding matches adb's exactly.

**Can I use it from a blocking program?** Wrap calls in a tokio runtime
(`Runtime::block_on`).

**Does it work with emulators?** Yes, over TCP (`-t 127.0.0.1:5555`) or
through the adb server.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Tests need no hardware: an
in-process device emulator speaks the phone side of the protocol. Real-device
tests run with `cargo test --features hardware --test hardware -- --ignored`.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. Unless you explicitly state
otherwise, any contribution intentionally submitted for inclusion in the
work by you shall be dual licensed as above, without any additional terms
or conditions.

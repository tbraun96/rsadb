# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2026-09-12

### Fixed

- USB transfers inside a Tokio runtime panicked with "Awaiting blocking syscall without an async
  runtime": `nusb`'s `tokio` feature is now enabled. Nothing that opened a device over USB from an
  async context worked before this; the TCP path was unaffected, which is why the emulator never
  showed it.
- A fresh handshake could fail with "unexpected CLSE during handshake" or report the host key as
  unauthorised, intermittently. A device still tearing down a previous session's streams leaves
  those frames in the bulk endpoint, and they arrive before the banner. Stream frames are now
  skipped during the handshake, up to a bound, instead of being treated as protocol errors.

Both were found on the first real phone (a Samsung Galaxy S10, Android 12) and neither is
reachable over TCP.

## [Unreleased]

## [0.1.0] - 2026-09-11

### Added

- `wire`: 24-byte ADB message headers, command words, checksum, bounds
  checking before payload allocation.
- `auth`: RSA host keys (`~/.android/adbkey`, PKCS#8 or PKCS#1 PEM),
  generation with 0600 permissions, Android `adbkey.pub` encoding
  (`n0inv`, `R² mod n`) byte-compatible with Google's adb, token signing
  (PKCS#1 v1.5 + SHA-1 DigestInfo).
- `transport`: `Transport` trait; USB over `nusb` (interface
  0xFF/0x42/0x01, bulk endpoints, zero-length packet handling,
  `ClaimFailed` when another adb holds the interface); TCP; a framed
  transport over any `AsyncRead + AsyncWrite`.
- `session`: `CNXN`/`AUTH` handshake with signature and public-key
  paths, `Unauthorized` after a caller-chosen wait, `A_STLS` reported as
  `Unsupported`, version and payload negotiation, multiplexed streams with
  `OKAY` flow control and consumer-driven acknowledgement.
- `services`: `shell:`, `shell,v2:` (framed stdin/stdout/stderr/exit),
  `exec:`, `sync:` (`LIST`/`LIS2`, `STAT`/`STA2`, `RECV`/`RCV2`,
  `SEND`/`SND2`, 64 KiB `DATA` chunks), `reboot:`, `getprop` parsing,
  `content query` row parsing with a row-count cross-check.
- `Device`: typed helpers (`getprop`, `getprops`, `shell`, `exec`,
  `screencap`, `list_dir`, `stat`, `pull`, `pull_to_file`, `push_file`,
  `push_bytes`, `content_query`, `reboot`).
- `host`: client for a running Google adb server (`host:version`,
  `host:devices-l`, `host:track-devices`, `host:transport:<serial>`).
- `track`: USB attach/detach streams via hotplug or polling.
- CLI `rsadb`: `devices`, `shell`, `exec`, `pull`, `push`, `ls`,
  `screencap`, `getprop`, `connect`, `keygen`.
- Tests: in-process device emulator, property tests for the codec,
  ignored hardware tests behind the `hardware` feature.

### Known limitations

- TLS authentication (`A_STLS`), `abb`, `forward:`/`reverse:` and sync
  compression are not implemented.

[Unreleased]: https://github.com/tbraun96/rsadb/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tbraun96/rsadb/releases/tag/v0.1.0

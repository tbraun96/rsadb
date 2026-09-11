# Security Policy

## Supported versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes       |

Only the latest 0.1.x release receives security fixes.

## Reporting a vulnerability

Please do not open a public issue for security problems.

Report privately through GitHub Security Advisories using "Report a
vulnerability" at:

https://github.com/tbraun96/rsadb/security/advisories/new

Include the affected version, the transport involved (USB, TCP, or adb
server), a description of the impact, and reproduction steps or a proof of
concept where possible.

## What counts

Reports in the following areas are in scope:

- Authentication bypass or weakening of the RSA host-key handshake.
- Memory-safety issues. The crate is `#![forbid(unsafe_code)]`, so these are
  most likely to surface through dependencies or through unbounded
  allocation.
- Private key handling: exposure, unintended disclosure, or writing the key
  with permissive modes.
- Command injection through the CLI or library, including shell argument
  quoting and property-name validation.
- Denial of service through malformed device responses (for example
  oversized payload lengths).

Problems in the Android platform, in `adbd`, or in Google's `adb` server are
out of scope unless rsadb makes them exploitable.

## Response expectations

- Acknowledgement within 7 days of the report.
- An assessment of severity and an expected fix timeline after triage.
- Credit in the advisory and changelog unless you prefer otherwise.
- A patched release and a published advisory once a fix is available.

## Security notes

- The host private key lives at `~/.android/adbkey` and is created with
  mode 0600. rsadb reads it locally to sign the device's 20-byte
  authentication token (PKCS#1 v1.5 with SHA-1, as required by the ADB
  protocol). The private key is never transmitted; only the public key, in
  Android's `adbkey.pub` encoding, is sent to the device.
- A device shows "Allow USB debugging?" for a public key it has not seen. Do
  not accept that prompt for a host you do not control.
- Property names are validated before they are placed in a shell command
  line, and all shell arguments are single-quoted.
- Payload lengths announced by the device are bounded to 1 MiB before any
  allocation takes place.
- TLS authentication (`A_STLS`, Android 11+ wireless pairing) is not
  implemented in 0.1 and returns `Error::Unsupported` rather than falling
  back to a weaker path.

## Known advisories

- [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071)
  (Marvin timing side channel in the `rsa` crate) is acknowledged in
  `deny.toml`. rsadb performs a single PKCS#1 v1.5 signature per connection
  with a key that never leaves the host, and the peer cannot request
  repeated signatures of chosen inputs, so no timing oracle is exposed. The
  dependency will move to the constant-time `rsa` line once it is stable.

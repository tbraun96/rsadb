# Contributing to rsadb

Thank you for your interest in rsadb, a pure-Rust Android Debug Bridge (ADB)
client library and CLI. This document explains how to build, test, and submit
changes.

## Prerequisites

- Rust 1.85 or newer (edition 2024). The MSRV is checked in CI.
- `cargo-deny` for the licence and advisory gate: `cargo install cargo-deny`.
- No Android SDK, `adb` binary, or libusb is required. USB access goes through
  the `nusb` crate; TCP access speaks directly to `adbd`.

## Building and testing

The full local gate is the same set of commands CI runs:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo deny check
```

All five must pass before a pull request is reviewed.

### Tests without hardware

Protocol and behaviour tests run against `FakeDevice` in `tests/common/fake`,
an in-process implementation of the `adbd` side of the wire protocol. Use it
for new tests wherever possible; it exercises the real transport, framing,
authentication, and service code without a device.

### Hardware tests

Tests that need a physical or emulated Android device are gated behind the
`hardware` feature and marked `#[ignore]`:

```sh
cargo test --features hardware --test hardware -- --ignored
```

Connect a device with USB debugging enabled first. The device will prompt
"Allow USB debugging?" the first time it sees the host key.

## Coding rules

- `#![forbid(unsafe_code)]` applies to the whole crate. Do not add `unsafe`.
- No `unwrap()` or `expect()` in library code. Return `Error` instead.
- Every source file must stay at or under 250 lines. Split files rather than
  growing them.
- No implicit defaults in production paths: require explicit configuration or
  fail fast.
- Keep business logic free of direct I/O so it can be tested against
  `FakeDevice`.
- Validate untrusted input at the boundary. Property names are checked before
  they reach a shell command line, shell arguments are single-quoted, and
  payload lengths are bounded (1 MiB) before allocation. Preserve these
  invariants when touching the affected code.
- Public items need rustdoc; `cargo doc` runs with `-D warnings`.

## Commit messages

Use Conventional Commits. The scope is the module or subsystem touched:

```
feat(sync): implement push with mtime preservation
fix(auth): reject tokens that are not exactly 20 bytes
docs(readme): document TCP connect
test(shell): cover v2 exit-code framing
chore(deps): bump nusb
```

Keep the subject line under 72 characters and explain the "why" in the body
when it is not obvious.

## Pull requests

- Open an issue first for anything larger than a bug fix so the design can be
  discussed.
- One logical change per pull request.
- Add or update tests using `FakeDevice`. Hardware-only coverage is not a
  substitute.
- Update `CHANGELOG.md` under "Unreleased".
- Use a Conventional Commit style title; the PR is squash-merged with it.
- Fill in the pull request template checklist.

## Scope of 0.1

The following are intentionally not implemented and return
`Error::Unsupported`: TLS authentication (`A_STLS`, Android 11+ wireless
pairing), `abb`, and port forward/reverse. Contributions in these areas are
welcome; please open an issue to coordinate.

## Releasing

Releases are cut by maintainers:

1. Bump `version` in `Cargo.toml` and move the "Unreleased" section of
   `CHANGELOG.md` under the new version.
2. Commit with `chore(release): vX.Y.Z` and merge to `main`.
3. Push a tag `vX.Y.Z` that exactly matches the `Cargo.toml` version.

The release workflow verifies the tag against `Cargo.toml`, builds CLI
binaries for Linux, macOS, and Windows, attaches them to a GitHub release,
and runs `cargo publish`. Publishing currently authenticates with a
`CARGO_REGISTRY_TOKEN` repository secret. The intended replacement is
crates.io Trusted Publishing (OIDC), which removes the long-lived token; the
workflow will be switched once the crate is configured for it on crates.io.

## Licence

rsadb is dual-licensed under MIT OR Apache-2.0. Unless you explicitly state
otherwise, any contribution intentionally submitted for inclusion in the work
by you shall be dual-licensed as above, without any additional terms or
conditions, following the Rust project convention.

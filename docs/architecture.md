# Architecture

rsadb is a stack of small layers; each one is testable without the one
below it.

```
┌───────────────────────────────────────────────────────────────┐
│ rsadb CLI (src/bin/rsadb)          examples/                  │
├───────────────────────────────────────────────────────────────┤
│ Device<C: Connection>              typed helpers: getprop,    │
│ (src/device)                       screencap, pull/push, …    │
├───────────────────────────────────────────────────────────────┤
│ services (src/services)            shell v1/v2, exec, sync,   │
│                                    reboot, content, props     │
├──────────────────────────┬────────────────────────────────────┤
│ Session (src/session)    │ HostClient / HostDevice (src/host) │
│ handshake + multiplexing │ adb-server text protocol           │
├──────────────────────────┼────────────────────────────────────┤
│ Transport (src/transport)│ one TCP socket per stream          │
│ usb (nusb) │ tcp │ framed│                                    │
├──────────────────────────┴────────────────────────────────────┤
│ wire (headers, commands)     auth (keys, signing, pubkey)     │
└───────────────────────────────────────────────────────────────┘
```

## The two traits that hold it together

* `Transport` (`src/transport/mod.rs`) moves whole `Message`s. It is used
  sequentially during the handshake and then `split()` into a
  `MessageSink` and a `MessageSource` so the session can read and write
  concurrently. USB splits into the two bulk endpoints; byte streams split
  with `tokio::io::split`.
* `Channel` / `Connection` (`src/channel.rs`) abstract "a service stream"
  and "something that opens service streams". `Session` implements
  `Connection` with multiplexed `Stream`s; `HostDevice` implements it with
  one TCP connection per stream. Every service and every `Device` method is
  written once against these traits.

## Session internals

```
                 ┌───────────── Session ─────────────┐
 open("sync:") ──► registry: local id → StreamState   │
                 │   remote id, closed, acks, Notify  │
                 │                                    │
   Stream ──WRTE──► outbox (mpsc) ──► writer task ──► MessageSink
   Stream ◄─data──┐                                   │
                  └── reader task ◄──────────────── MessageSource
                        OKAY → ack() / adopt remote id
                        WRTE → deliver to the stream's queue
                        CLSE → mark closed, drop the queue
```

* `Stream::send` posts a `WRTE` and waits until the acknowledgement counter
  moves past the value it saw before posting. Counting rather than
  signalling matters: an `OKAY` immediately followed by `CLSE` (what
  `sync:` does after `QUIT`) must read as "written, then closed", not as
  "closed while writing".
* `Stream::recv` sends the `OKAY` for a received payload only after the
  caller has taken it, so back-pressure reaches the device.
* Dropping a `Stream` sends `CLSE`; dropping the `Session` aborts both
  tasks and closes every stream.

## Error policy

One `Error` enum (`src/error.rs`) with variants callers can act on:
`Unauthorized` (tap Allow on the phone), `ClaimFailed` (stop the other adb
server), `ServiceRefused`, `RemoteFailure` (a `FAIL` message), `Unsupported`
(`A_STLS`), `Protocol` (malformed data), `Disconnected`. Library code never
panics on input; `unwrap`/`expect` are denied by clippy.

## I/O boundaries

Parsing and encoding are pure functions (`wire::codec`, `auth::pubkey`,
`services::content`, `services::props`, `services::shell_v2`,
`services::sync::codec`) with unit tests. I/O lives in the transports, in
`auth::keyfile`, and in `Device`'s file helpers. Everything in between is
generic over `Channel`/`Connection`, which is what lets the integration
tests run a complete device emulator in process (`tests/common/fake`)
over an in-memory duplex pipe or a loopback TCP socket.

## File layout rules

Every source file stays at or under 250 lines (CI enforces this); modules
are split by concern rather than by size when they approach the limit.

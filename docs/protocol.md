# The ADB wire protocol, as rsadb speaks it

This is an independent description written from the public AOSP
`protocol.txt`, `adb.h`, `adb_auth_host.cpp`, `android_pubkey.c`,
`shell_protocol.h` and `file_sync_protocol.h`. Nothing here is copied from
those files; if the two disagree, the phone is right and this is a bug.

## 1. Messages

Every message is a 24-byte little-endian header, optionally followed by a
payload of `data_length` bytes.

```
offset  size  field        meaning
0       4     command      one of the words below
4       4     arg0         command specific
8       4     arg1         command specific
12      4     data_length  payload bytes that follow
16      4     data_check   wrapping sum of all payload bytes (see 1.2)
20      4     magic        command XOR 0xFFFFFFFF
```

| word   | value        | arg0                 | arg1               | payload                     |
|--------|--------------|----------------------|--------------------|-----------------------------|
| `CNXN` | `0x4e584e43` | protocol version     | max payload        | identity banner             |
| `AUTH` | `0x48545541` | 1 token / 2 sig / 3 key | 0               | token, signature or key     |
| `OPEN` | `0x4e45504f` | local id             | 0                  | service name + NUL          |
| `OKAY` | `0x59414b4f` | sender's id          | receiver's id      | none                        |
| `WRTE` | `0x45545257` | sender's id          | receiver's id      | data                        |
| `CLSE` | `0x45534c43` | sender's id (0 = refusal) | receiver's id | none                        |
| `STLS` | `0x534c5453` | TLS version          | 0                  | none                        |
| `SYNC` | `0x434e5953` | obsolete             |                    |                             |

The magic is a cheap integrity check: a header whose last word is not the
complement of its first is rejected before any payload is read, as is any
`data_length` above the negotiated limit.

### 1.1 Versions and payload size

We announce `0x01000001` and 1 MiB. The device answers with its own
version and limit; each side uses the minimum. A device that sends `0` as
its limit is treated as a legacy 256 KiB peer.

### 1.2 The checksum

Peers at version `0x01000001` or newer ignore `data_check`. rsadb
therefore fills it in only until the peer's version is known and, after
`CNXN`, only when the negotiated version is older than `0x01000001`.
Incoming checksums are verified under the same rule. Both behaviours are
exercised by the tests with a legacy fake device.

## 2. Connecting

```
host                                            device
 | CNXN(0x01000001, 1 MiB, "host::features=…") ---> |
 |                                                  |
 |  <--- AUTH(TOKEN=1, 20 random bytes)             |   (skipped by devices
 | AUTH(SIGNATURE=2, RSA-sign(token)) --->          |    without auth)
 |                                                  |
 |  <--- CNXN(version, max, "device::…")            |   key known: done
 |                                                  |
 |  <--- AUTH(TOKEN=1, new token)                   |   key unknown:
 | AUTH(RSAPUBLICKEY=3, "<base64> user@host\0") --> |
 |       … user taps "Allow USB debugging" …        |
 |  <--- CNXN(version, max, "device::…")            |
```

* The signature is PKCS#1 v1.5 over the 20-byte token with the SHA-1
  `DigestInfo` prefix, that is `RSA_sign(NID_sha1, token, 20)`. The token
  is treated as if it were already a SHA-1 digest; it is not hashed again.
* If the user never taps Allow the device stays silent. rsadb waits for a
  caller-provided duration and then reports `Error::Unauthorized`.
* A device that answers `CNXN` with `STLS` wants TLS-based authentication
  (Android 11 wireless debugging). rsadb 0.1 reports `Error::Unsupported`.

### 2.1 The banner

`<kind>::<key>=<value>;<key>=<value>;…` where `kind` is `host`, `device`,
`recovery`, `sideload` or `bootloader`. Devices send `ro.product.name`,
`ro.product.model`, `ro.product.device` and `features=` (comma separated).
rsadb announces `shell_v2,cmd,stat_v2,ls_v2,apex,abb,abb_exec,
fixed_push_mkdir,fixed_push_symlink_timestamp` and adapts to whatever the
device advertises back.

### 2.2 The public key (`adbkey.pub`)

Android stores RSA public keys in a Montgomery-friendly layout so the
kernel-side verifier does not need a bignum library:

```
u32      len       modulus size in 32-bit words, always 64
u32      n0inv     -n^-1 mod 2^32, where n is the modulus
u32[64]  n         modulus, little-endian words
u32[64]  rr        R^2 mod n with R = 2^2048, little-endian words
u32      exponent  65537
```

The 524 bytes are base64-encoded (700 characters), followed by a space and
a `user@host` comment. `rsadb` computes `n0inv` by Newton iteration on
`u32` and `rr` with the `rsa` crate's bignum; the tests recompute both with
`num-bigint` and compare byte for byte. The encoding of an existing
`~/.android/adbkey` written by Google's adb is identical, so a phone that
already trusts that key trusts rsadb.

## 3. Streams

After `CNXN` either side may open streams. Ids are per side; the host
allocates local ids from 1 upwards.

```
host                                  device
 | OPEN(local=7, 0, "shell:id\0") ---> |
 |  <--- OKAY(remote=42, local=7)      |   opened; or CLSE(0, 7) = refused
 |  <--- WRTE(42, 7, "uid=2000…")      |
 | OKAY(7, 42) --->                    |   acknowledge before the next WRTE
 |  <--- CLSE(42, 7)                   |   end of stream
```

Flow control is one payload in flight per direction: a sender must not
issue another `WRTE` on a stream until the previous one was `OKAY`ed. rsadb
enforces this on writes and, on reads, acknowledges a payload only once the
caller has taken it, so a slow consumer bounds the device's memory use to a
single payload per stream.

`CLSE` needs no reply. A `CLSE` with `arg0 == 0` in reply to `OPEN` means
the service was refused (`Error::ServiceRefused`).

## 4. Services

The `OPEN` payload names a service. rsadb implements:

| service              | shape                                                        |
|----------------------|--------------------------------------------------------------|
| `shell:<cmd>`        | raw bytes until `CLSE`; stdout and stderr merged; no exit code |
| `shell,v2:<cmd>`     | framed, see 4.1                                              |
| `exec:<cmd>`         | raw stdout without a pty; used for `screencap -p`            |
| `sync:`              | the file-transfer sub-protocol, see 4.2                      |
| `reboot:<target>`    | `CLSE` follows as the device goes down                       |

### 4.1 Shell protocol v2

Each frame is `id: u8`, `length: u32 LE`, `length` bytes.

| id | name         | direction | payload                         |
|----|--------------|-----------|---------------------------------|
| 0  | stdin        | host → device | bytes for the process's stdin |
| 1  | stdout       | device → host | bytes                        |
| 2  | stderr       | device → host | bytes                        |
| 3  | exit         | device → host | one byte: the exit status    |
| 4  | close stdin  | host → device | empty                        |
| 5  | window size  | host → device | `rows x cols , xpix x ypix`  |

### 4.2 The sync sub-protocol

Requests are `id: [u8; 4]`, `length: u32 LE`, then `length` bytes of path.
Replies have fixed layouts; all integers little-endian.

| request | reply                                                                 |
|---------|-----------------------------------------------------------------------|
| `STAT`  | `STAT` mode:u32 size:u32 mtime:u32 (16 bytes; mode 0 = not found)     |
| `STA2`  | `STA2` error:u32 dev:u64 ino:u64 mode nlink uid gid:u32 size:u64 atime mtime ctime:i64 (72 bytes) |
| `LIST`  | repeated `DENT` mode size mtime:u32 namelen:u32 name; then `DONE` padded to 20 bytes |
| `LIS2`  | repeated `DNT2` (the `STA2` body + namelen + name); then `DONE` padded to 76 bytes |
| `RECV`  | repeated `DATA` len:u32 bytes (≤ 64 KiB), then `DONE` 0, or `FAIL` len msg |
| `RCV2`  | as `RECV` after an extra `RCV2` flags:u32 word from the host (0 = uncompressed) |
| `SEND`  | path is `remote,mode`; host sends `DATA` chunks then `DONE` mtime; device replies `OKAY` 0 or `FAIL` len msg |
| `SND2`  | path, then `SND2` mode:u32 flags:u32, then as `SEND`                  |
| `QUIT`  | `QUIT` 0 ends the session                                             |

`v2` variants are used when the device advertises `stat_v2`, `ls_v2` or
`sendrecv_v2`.

## 5. Talking to an adb server instead

Google's `adb` daemon on port 5037 uses a different, text-framed protocol:
each request is `%04x` hex length + text, answered by `OKAY` or `FAIL` + a
length-prefixed message. `host:transport:<serial>` binds the connection to
a device, after which a service name is sent the same way and the socket
becomes the raw stream of that service. rsadb's `host` module implements
this so the same `Device` API works when another program owns the USB
interface.

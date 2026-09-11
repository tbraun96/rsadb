//! Property tests for the header codec and the framed transport.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use rsadb::Transport as _;
use rsadb::transport::{MessageSink as _, MessageSource as _, StreamTransport, WireConfig};
use rsadb::wire::{
    Command, HEADER_LEN, Header, MAX_PAYLOAD, Message, checksum, decode_header, encode_header,
};

fn any_command() -> impl Strategy<Value = Command> {
    prop::sample::select(Command::ALL.to_vec())
}

fn any_header() -> impl Strategy<Value = Header> {
    (
        any_command(),
        any::<u32>(),
        any::<u32>(),
        0..=MAX_PAYLOAD,
        any::<u32>(),
    )
        .prop_map(|(command, arg0, arg1, data_length, data_check)| Header {
            command,
            arg0,
            arg1,
            data_length,
            data_check,
        })
}

proptest! {
    #[test]
    fn header_roundtrips(header in any_header()) {
        let bytes = encode_header(&header);
        prop_assert_eq!(decode_header(&bytes, MAX_PAYLOAD).ok(), Some(header));
        let magic = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        prop_assert_eq!(magic, header.command.code() ^ 0xFFFF_FFFF);
    }

    #[test]
    fn corrupting_the_magic_is_always_detected(header in any_header(), bit in 0u32..32) {
        let mut bytes = encode_header(&header);
        let magic = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]) ^ (1 << bit);
        bytes[20..24].copy_from_slice(&magic.to_le_bytes());
        prop_assert!(decode_header(&bytes, MAX_PAYLOAD).is_err());
    }

    #[test]
    fn random_bytes_never_panic(bytes in prop::array::uniform24(any::<u8>())) {
        let _ = decode_header(&bytes, MAX_PAYLOAD);
        prop_assert_eq!(bytes.len(), HEADER_LEN);
    }

    #[test]
    fn checksum_matches_reference(payload in prop::collection::vec(any::<u8>(), 0..4096)) {
        let reference = payload.iter().map(|&b| u32::from(b)).fold(0u32, u32::wrapping_add);
        prop_assert_eq!(checksum(&payload), reference);
    }

    #[test]
    fn messages_survive_the_framed_transport(
        command in any_command(),
        arg0 in any::<u32>(),
        arg1 in any::<u32>(),
        payload in prop::collection::vec(any::<u8>(), 0..70_000),
        legacy in any::<bool>(),
    ) {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        rt.block_on(async {
            let (a, b) = tokio::io::duplex(128 * 1024);
            let mut host = StreamTransport::new(a);
            let mut device = StreamTransport::new(b);
            let version = if legacy { 0x0100_0000 } else { 0x0100_0001 };
            host.configure(WireConfig::negotiated(version, MAX_PAYLOAD));
            device.configure(WireConfig::negotiated(version, MAX_PAYLOAD));
            let msg = Message::new(command, arg0, arg1, payload.clone());
            let sent = msg.clone();
            let writer = tokio::spawn(async move { host.send(sent).await });
            let got = device.recv().await.unwrap();
            writer.await.unwrap().unwrap();
            prop_assert_eq!(got, msg);
            Ok(())
        })?;
    }
}

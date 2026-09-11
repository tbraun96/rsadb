//! The command words of the ADB transport protocol.

use std::fmt;

/// A transport-level command word.
///
/// Each value is the little-endian encoding of a four-character mnemonic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Command {
    /// `SYNC` — obsolete; kept for completeness.
    Sync = 0x434e_5953,
    /// `CNXN` — connection banner (version, max payload, identity string).
    Connect = 0x4e58_4e43,
    /// `AUTH` — authentication exchange.
    Auth = 0x4854_5541,
    /// `OPEN` — open a stream to a named service.
    Open = 0x4e45_504f,
    /// `OKAY` — stream ready / write acknowledged.
    Okay = 0x5941_4b4f,
    /// `CLSE` — close a stream.
    Close = 0x4553_4c43,
    /// `WRTE` — data on a stream.
    Write = 0x4554_5257,
    /// `STLS` — switch to TLS-based authentication (Android 11+).
    StartTls = 0x534c_5453,
}

impl Command {
    /// The numeric command word.
    pub const fn code(self) -> u32 {
        self as u32
    }

    /// The four-character mnemonic.
    pub const fn mnemonic(self) -> &'static str {
        match self {
            Self::Sync => "SYNC",
            Self::Connect => "CNXN",
            Self::Auth => "AUTH",
            Self::Open => "OPEN",
            Self::Okay => "OKAY",
            Self::Close => "CLSE",
            Self::Write => "WRTE",
            Self::StartTls => "STLS",
        }
    }

    /// Decode a command word.
    pub const fn from_code(code: u32) -> Option<Self> {
        Some(match code {
            0x434e_5953 => Self::Sync,
            0x4e58_4e43 => Self::Connect,
            0x4854_5541 => Self::Auth,
            0x4e45_504f => Self::Open,
            0x5941_4b4f => Self::Okay,
            0x4553_4c43 => Self::Close,
            0x4554_5257 => Self::Write,
            0x534c_5453 => Self::StartTls,
            _ => return None,
        })
    }

    /// All command words, for exhaustive tests.
    pub const ALL: [Self; 8] = [
        Self::Sync,
        Self::Connect,
        Self::Auth,
        Self::Open,
        Self::Okay,
        Self::Close,
        Self::Write,
        Self::StartTls,
    ];
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.mnemonic())
    }
}

#[cfg(test)]
mod tests {
    use super::Command;

    #[test]
    fn mnemonic_bytes_match_codes() {
        for cmd in Command::ALL {
            let bytes: [u8; 4] = cmd
                .mnemonic()
                .as_bytes()
                .try_into()
                .unwrap_or_else(|_| unreachable!("mnemonics are four bytes"));
            assert_eq!(u32::from_le_bytes(bytes), cmd.code(), "{cmd}");
            assert_eq!(Command::from_code(cmd.code()), Some(cmd));
        }
        assert_eq!(Command::from_code(0), None);
    }
}

//! Crate-wide error type.

use std::fmt;

/// Result alias used throughout the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Every failure `rsadb` can report.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A message header was malformed (bad magic, unknown command, oversized payload).
    #[error("malformed ADB message: {0}")]
    Protocol(String),

    /// The peer closed the connection or the transport hit end-of-stream.
    #[error("connection closed by peer")]
    Disconnected,

    /// The device wants a "Allow USB debugging?" confirmation that has not been given.
    #[error("device has not authorised this host key (accept the prompt on the device)")]
    Unauthorized,

    /// The device asked for a mechanism this version does not implement.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// The device refused to open a service (`A_CLSE` in reply to `A_OPEN`).
    #[error("device refused service {service:?}")]
    ServiceRefused {
        /// The service string that was rejected.
        service: String,
    },

    /// A `sync:` or host-server request answered with `FAIL`.
    #[error("remote failure: {0}")]
    RemoteFailure(String),

    /// The stream was closed before the operation finished.
    #[error("stream closed")]
    StreamClosed,

    /// The USB interface is held by another process (usually the Google adb server).
    #[error("could not claim the ADB USB interface: {0} (is another adb server running?)")]
    ClaimFailed(String),

    /// No device matched the request.
    #[error("no ADB device found{}", fmt_hint(.0.as_deref()))]
    NoDevice(Option<String>),

    /// A USB-level failure.
    #[error("usb: {0}")]
    Usb(String),

    /// An operating-system I/O failure.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// A host-key failure (parse, generate, sign).
    #[error("host key: {0}")]
    Key(String),

    /// A reply could not be parsed into the requested shape.
    #[error("could not parse device output: {0}")]
    Parse(String),

    /// The caller supplied an argument the protocol cannot carry.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// The operation did not finish in the allotted time.
    #[error("timed out: {0}")]
    Timeout(String),
}

fn fmt_hint(hint: Option<&str>) -> String {
    hint.map(|h| format!(" ({h})")).unwrap_or_default()
}

impl Error {
    /// Build a [`Error::Protocol`] from anything displayable.
    pub fn protocol(msg: impl fmt::Display) -> Self {
        Self::Protocol(msg.to_string())
    }

    /// Build a [`Error::Parse`] from anything displayable.
    pub fn parse(msg: impl fmt::Display) -> Self {
        Self::Parse(msg.to_string())
    }
}

impl From<rsa::Error> for Error {
    fn from(e: rsa::Error) -> Self {
        Self::Key(e.to_string())
    }
}

impl From<rsa::pkcs8::Error> for Error {
    fn from(e: rsa::pkcs8::Error) -> Self {
        Self::Key(e.to_string())
    }
}

impl From<rsa::pkcs1::Error> for Error {
    fn from(e: rsa::pkcs1::Error) -> Self {
        Self::Key(e.to_string())
    }
}

impl From<base64::DecodeError> for Error {
    fn from(e: base64::DecodeError) -> Self {
        Self::Key(format!("base64: {e}"))
    }
}

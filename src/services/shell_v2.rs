//! Framing for the `shell,v2:` protocol.
//!
//! Every frame is `id: u8`, `len: u32` little-endian, then `len` bytes.

use crate::error::{Error, Result};
use bytes::{Bytes, BytesMut};

/// Frame identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameId {
    /// Data for the command's stdin (host → device).
    Stdin = 0,
    /// Data from the command's stdout.
    Stdout = 1,
    /// Data from the command's stderr.
    Stderr = 2,
    /// One byte: the exit code.
    Exit = 3,
    /// Host closed the command's stdin.
    CloseStdin = 4,
    /// Terminal size `rows x cols , xpixels x ypixels` as text.
    WindowSize = 5,
}

impl FrameId {
    /// Decode an id byte.
    pub const fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Stdin,
            1 => Self::Stdout,
            2 => Self::Stderr,
            3 => Self::Exit,
            4 => Self::CloseStdin,
            5 => Self::WindowSize,
            _ => return None,
        })
    }
}

/// A decoded frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Which stream the payload belongs to.
    pub id: FrameId,
    /// The payload.
    pub data: Bytes,
}

/// Encode one frame.
pub fn encode(id: FrameId, data: &[u8]) -> Result<Bytes> {
    let len = u32::try_from(data.len())
        .map_err(|_| Error::InvalidArgument("shell v2 frame longer than u32::MAX".into()))?;
    let mut out = BytesMut::with_capacity(5 + data.len());
    out.extend_from_slice(&[id as u8]);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(data);
    Ok(out.freeze())
}

/// Incremental frame parser.
#[derive(Debug, Default)]
pub struct Parser {
    buffer: BytesMut,
}

impl Parser {
    /// Feed bytes and return every frame that is now complete.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Frame>> {
        self.buffer.extend_from_slice(bytes);
        let mut frames = Vec::new();
        while self.buffer.len() >= 5 {
            let id = FrameId::from_byte(self.buffer[0]).ok_or_else(|| {
                Error::protocol(format!("unknown shell v2 frame id {}", self.buffer[0]))
            })?;
            let len = u32::from_le_bytes([
                self.buffer[1],
                self.buffer[2],
                self.buffer[3],
                self.buffer[4],
            ]) as usize;
            if self.buffer.len() < 5 + len {
                break;
            }
            let _ = self.buffer.split_to(5);
            frames.push(Frame {
                id,
                data: self.buffer.split_to(len).freeze(),
            });
        }
        Ok(frames)
    }

    /// Bytes received but not yet forming a whole frame.
    pub fn pending(&self) -> usize {
        self.buffer.len()
    }
}

/// The collected result of a `shell,v2:` command.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShellOutput {
    /// Everything written to stdout.
    pub stdout: Vec<u8>,
    /// Everything written to stderr.
    pub stderr: Vec<u8>,
    /// The exit code, if the device reported one (`shell,v2` only).
    pub exit_code: Option<u8>,
}

impl ShellOutput {
    /// Fold a frame into the output.
    pub fn apply(&mut self, frame: &Frame) {
        match frame.id {
            FrameId::Stdout => self.stdout.extend_from_slice(&frame.data),
            FrameId::Stderr => self.stderr.extend_from_slice(&frame.data),
            FrameId::Exit => self.exit_code = frame.data.first().copied(),
            FrameId::Stdin | FrameId::CloseStdin | FrameId::WindowSize => {}
        }
    }

    /// `stdout` as text (lossily).
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// `stderr` as text (lossily).
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }

    /// Whether the command reported success (exit 0, or no exit code at all).
    pub fn success(&self) -> bool {
        self.exit_code.is_none_or(|c| c == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_split_frames() {
        let mut p = Parser::default();
        let mut bytes = encode(FrameId::Stdout, b"hi").unwrap_or_default().to_vec();
        bytes.extend_from_slice(&encode(FrameId::Exit, &[7]).unwrap_or_default());
        let first = p.push(&bytes[..4]).unwrap_or_default();
        assert!(first.is_empty());
        let rest = p.push(&bytes[4..]).unwrap_or_default();
        assert_eq!(rest.len(), 2);
        let mut out = ShellOutput::default();
        for f in &rest {
            out.apply(f);
        }
        assert_eq!(out.stdout, b"hi");
        assert_eq!(out.exit_code, Some(7));
        assert!(!out.success());
        assert!(p.push(&[9, 0, 0, 0, 0]).is_err());
    }
}

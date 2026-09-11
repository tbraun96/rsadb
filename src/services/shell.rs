//! `shell:`, `shell,v2:` and `exec:` services.

use super::shell_v2::{self, FrameId, ShellOutput};
use crate::channel::{Channel, ChannelReader, Connection, drain};
use crate::error::{Error, Result};
use bytes::Bytes;

/// Feature a device must advertise for `shell,v2:` to work.
pub const SHELL_V2_FEATURE: &str = "shell_v2";

fn check_command(command: &str) -> Result<()> {
    if command.contains('\0') {
        return Err(Error::InvalidArgument("command contains NUL".into()));
    }
    Ok(())
}

/// Run `command` through the legacy `shell:` service: stdout and stderr merged, no exit code.
pub async fn shell_v1<C: Connection>(conn: &C, command: &str) -> Result<Bytes> {
    check_command(command)?;
    drain(conn.open(&format!("shell:{command}")).await?).await
}

/// Run `command` through `exec:`: raw stdout only, no pty, suitable for binary output.
pub async fn exec<C: Connection>(conn: &C, command: &str) -> Result<Bytes> {
    check_command(command)?;
    drain(conn.open(&format!("exec:{command}")).await?).await
}

/// Run `command` through `shell,v2:` with `stdin` fed to the process, collecting everything.
pub async fn shell_v2<C: Connection>(conn: &C, command: &str, stdin: &[u8]) -> Result<ShellOutput> {
    check_command(command)?;
    let mut channel = conn.open(&format!("shell,v2:{command}")).await?;
    if !stdin.is_empty() {
        channel
            .send(shell_v2::encode(FrameId::Stdin, stdin)?)
            .await?;
    }
    channel
        .send(shell_v2::encode(FrameId::CloseStdin, &[])?)
        .await?;
    collect_v2(channel).await
}

/// Consume a `shell,v2:` channel to the end.
pub async fn collect_v2<C: Channel>(channel: C) -> Result<ShellOutput> {
    let mut reader = ChannelReader::new(channel);
    let mut parser = shell_v2::Parser::default();
    let mut out = ShellOutput::default();
    while let Some(chunk) = reader.next_chunk().await? {
        for frame in parser.push(&chunk)? {
            out.apply(&frame);
        }
    }
    reader.channel().close().await?;
    if parser.pending() != 0 {
        return Err(Error::protocol(format!(
            "{} trailing bytes in shell v2 stream",
            parser.pending()
        )));
    }
    Ok(out)
}

/// Run `command` with the best protocol the device offers.
///
/// Uses `shell,v2:` when `features` lists `shell_v2` (separate stderr, exit
/// code) and falls back to `shell:` otherwise.
pub async fn shell<C: Connection>(
    conn: &C,
    features: &[String],
    command: &str,
) -> Result<ShellOutput> {
    if features.iter().any(|f| f == SHELL_V2_FEATURE) {
        shell_v2(conn, command, &[]).await
    } else {
        let stdout = shell_v1(conn, command).await?;
        Ok(ShellOutput {
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
            exit_code: None,
        })
    }
}

/// Quote `arg` for `/system/bin/sh` so it is passed through verbatim.
pub fn quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./:=@,".contains(&b))
    {
        return arg.to_owned();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn quoting() {
        assert_eq!(quote("content://sms/inbox"), "content://sms/inbox");
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), "'it'\\''s'");
        assert_eq!(quote(""), "''");
        assert_eq!(quote("$(rm -rf /)"), "'$(rm -rf /)'");
    }
}

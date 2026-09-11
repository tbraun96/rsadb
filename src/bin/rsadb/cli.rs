//! Argument definitions and exit-code mapping.

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Pure-Rust ADB client.
#[derive(Debug, Parser)]
#[command(name = "rsadb", version, about, long_about = None)]
pub struct Cli {
    #[command(flatten)]
    pub target: Target,

    /// Seconds to wait for the "Allow USB debugging?" tap after offering our key.
    #[arg(long, global = true, default_value_t = 60)]
    pub auth_wait: u64,

    /// Private key file (default: ~/.android/adbkey, created if missing).
    #[arg(long, global = true, value_name = "PATH")]
    pub key: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

/// How to reach the device.
#[derive(Debug, Args)]
pub struct Target {
    /// USB serial number (or server-side serial with --server).
    #[arg(short, long, global = true, value_name = "SERIAL")]
    pub serial: Option<String>,

    /// Talk to adbd over TCP at HOST[:PORT] instead of USB.
    #[arg(
        short = 't',
        long,
        global = true,
        value_name = "HOST[:PORT]",
        conflicts_with = "server"
    )]
    pub tcp: Option<String>,

    /// Go through a running Google adb server (127.0.0.1:5037) instead of USB.
    #[arg(long, global = true)]
    pub server: bool,
}

/// Subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// List attached devices (USB directly, or via the server with --server).
    Devices,
    /// Run a shell command and print its output.
    Shell {
        /// The command line, passed to /system/bin/sh.
        #[arg(required = true, trailing_var_arg = true)]
        command: Vec<String>,
    },
    /// Run a command through `exec:` and write its raw stdout to our stdout.
    Exec {
        /// The command line.
        #[arg(required = true, trailing_var_arg = true)]
        command: Vec<String>,
    },
    /// Copy a file from the device.
    Pull {
        /// Path on the device.
        remote: String,
        /// Local destination file.
        local: PathBuf,
    },
    /// Copy a file to the device.
    Push {
        /// Local source file.
        local: PathBuf,
        /// Path on the device.
        remote: String,
    },
    /// List a directory on the device.
    Ls {
        /// Directory path.
        path: String,
    },
    /// Take a screenshot and save it as PNG.
    Screencap {
        /// Output file.
        out: PathBuf,
    },
    /// Print one property, or all of them.
    Getprop {
        /// Property name.
        name: Option<String>,
    },
    /// Connect to adbd over TCP and print its banner (no background server is kept).
    Connect {
        /// HOST[:PORT], port 5555 by default.
        address: String,
    },
    /// Create the host key if it does not exist, and print the public key line.
    Keygen,
}

/// Map an error to a process exit code.
pub fn exit_code(err: &rsadb::Error) -> u8 {
    use rsadb::Error;
    match err {
        Error::InvalidArgument(_) => 2,
        Error::NoDevice(_) => 3,
        Error::Unauthorized => 4,
        Error::ClaimFailed(_) => 5,
        Error::Unsupported(_) => 6,
        Error::RemoteFailure(_) | Error::ServiceRefused { .. } => 7,
        _ => 1,
    }
}

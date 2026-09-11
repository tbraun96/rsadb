//! Subcommand implementations.

use crate::cli::{Cli, Command};
use crate::connect::{AnyDevice, connect, key_comment, key_paths};
use rsadb::host::HostClient;
use rsadb::services::Entry;
use rsadb::transport::{tcp, usb};
use rsadb::{Connection, Device, Error, Result, Session};
use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

/// Dispatch one parsed command line.
pub async fn run(cli: Cli) -> Result<()> {
    match &cli.command {
        Command::Devices => devices(&cli).await,
        Command::Keygen => keygen(&cli),
        Command::Connect { address } => connect_tcp(&cli, address).await,
        _ => match connect(&cli).await? {
            AnyDevice::Direct(device) => on_device(&device, &cli.command).await,
            AnyDevice::Server(device) => on_device(&device, &cli.command).await,
        },
    }
}

async fn devices(cli: &Cli) -> Result<()> {
    if cli.target.server {
        for d in HostClient::local().devices().await? {
            let attrs: Vec<String> = d
                .attributes
                .iter()
                .map(|(k, v)| format!("{k}:{v}"))
                .collect();
            println!("{:<24}{:<14}{}", d.serial, d.state, attrs.join(" "));
        }
        return Ok(());
    }
    let list = usb::list().await?;
    if list.is_empty() {
        println!("no ADB USB devices attached");
    }
    for d in list {
        println!(
            "{:<24}{:04x}:{:04x}  {} {}",
            d.serial().unwrap_or("(no serial)"),
            d.vendor_id(),
            d.product_id(),
            d.manufacturer().unwrap_or(""),
            d.product().unwrap_or("")
        );
    }
    Ok(())
}

fn keygen(cli: &Cli) -> Result<()> {
    let paths = key_paths(cli)?;
    let existed = paths.private.exists();
    let key = rsadb::auth::load_or_generate(&paths, &key_comment())?;
    eprintln!(
        "{} {}",
        if existed { "using" } else { "created" },
        paths.private.display()
    );
    println!("{}", key.public_key_line()?);
    Ok(())
}

async fn connect_tcp(cli: &Cli, address: &str) -> Result<()> {
    let (host, port) = tcp::parse_endpoint(address)?;
    let key = rsadb::auth::load_or_generate(&key_paths(cli)?, &key_comment())?;
    let transport = tcp::connect((host.as_str(), port)).await?;
    let session = Session::connect(transport, &key, Duration::from_secs(cli.auth_wait)).await?;
    let banner = session.banner();
    println!(
        "connected to {host}:{port}: {} {}",
        banner.kind,
        banner.to_wire()
    );
    Ok(())
}

async fn on_device<C: Connection>(device: &Device<C>, command: &Command) -> Result<()> {
    match command {
        Command::Shell { command } => shell(device, &command.join(" ")).await,
        Command::Exec { command } => {
            let out = device.exec(&command.join(" ")).await?;
            std::io::stdout().write_all(&out)?;
            Ok(())
        }
        Command::Pull { remote, local } => {
            let n = device.pull_to_file(remote, local).await?;
            eprintln!("{remote} -> {}: {n} bytes", local.display());
            Ok(())
        }
        Command::Push { local, remote } => {
            device.push_file(local, remote).await?;
            eprintln!("{} -> {remote}", local.display());
            Ok(())
        }
        Command::Ls { path } => {
            for entry in device.list_dir(path).await? {
                println!("{}", format_entry(&entry));
            }
            Ok(())
        }
        Command::Screencap { out } => screencap(device, out).await,
        Command::Getprop { name: Some(name) } => {
            println!("{}", device.getprop(name).await?);
            Ok(())
        }
        Command::Getprop { name: None } => {
            for (k, v) in device.getprops().await? {
                println!("[{k}]: [{v}]");
            }
            Ok(())
        }
        Command::Devices | Command::Keygen | Command::Connect { .. } => Err(
            Error::InvalidArgument("command does not need a device".into()),
        ),
    }
}

async fn shell<C: Connection>(device: &Device<C>, command: &str) -> Result<()> {
    let out = device.shell(command).await?;
    std::io::stdout().write_all(&out.stdout)?;
    std::io::stderr().write_all(&out.stderr)?;
    match out.exit_code {
        Some(0) | None => Ok(()),
        Some(code) => Err(Error::RemoteFailure(format!("exit status {code}"))),
    }
}

async fn screencap<C: Connection>(device: &Device<C>, out: &Path) -> Result<()> {
    let png = device.screencap().await?;
    tokio::fs::write(out, &png).await?;
    eprintln!("{}: {} bytes", out.display(), png.len());
    Ok(())
}

fn format_entry(entry: &Entry) -> String {
    let kind = if entry.is_dir() {
        'd'
    } else if entry.is_symlink() {
        'l'
    } else {
        '-'
    };
    format!(
        "{kind}{:o} {:>10} {:>11} {}",
        entry.mode & 0o777,
        entry.size,
        entry.mtime,
        entry.name
    )
}

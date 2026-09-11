//! Stream a file off the device chunk by chunk, then list its directory.
//!
//! ```sh
//! cargo run --example pull -- /sdcard/DCIM/Camera/IMG_0001.jpg out.jpg
//! ```

mod common;

use futures::StreamExt as _;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt as _;

#[tokio::main]
async fn main() -> rsadb::Result<()> {
    let mut args = std::env::args().skip(1);
    let remote = args
        .next()
        .unwrap_or_else(|| "/system/build.prop".to_owned());
    let local = PathBuf::from(args.next().unwrap_or_else(|| "pulled.bin".to_owned()));

    let device = common::connect().await?;
    let stat = device.stat(&remote).await?;
    println!("{remote}: {} bytes, mode {:o}", stat.size, stat.mode);

    let mut file = tokio::fs::File::create(&local).await?;
    let mut chunks = std::pin::pin!(device.pull(&remote).await?);
    let mut total = 0usize;
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk?;
        total += chunk.len();
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    println!("wrote {total} bytes to {}", local.display());

    let parent = remote
        .rsplit_once('/')
        .map_or("/", |(dir, _)| if dir.is_empty() { "/" } else { dir });
    for entry in device.list_dir(parent).await? {
        println!("{:>10} {}", entry.size, entry.name);
    }
    Ok(())
}

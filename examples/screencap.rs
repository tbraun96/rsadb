//! Save a screenshot as PNG.
//!
//! ```sh
//! cargo run --example screencap -- screen.png
//! ```

mod common;

#[tokio::main]
async fn main() -> rsadb::Result<()> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "screen.png".to_owned());
    let device = common::connect().await?;
    let png = device.screencap().await?;
    tokio::fs::write(&out, &png).await?;
    println!("{out}: {} bytes", png.len());
    Ok(())
}

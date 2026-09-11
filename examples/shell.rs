//! Run a shell command with `shell,v2:` and show stdout, stderr and the exit code.
//!
//! ```sh
//! cargo run --example shell -- 'ls -l /sdcard'
//! ```

mod common;

#[tokio::main]
async fn main() -> rsadb::Result<()> {
    let command = std::env::args().nth(1).unwrap_or_else(|| "id".to_owned());
    let device = common::connect().await?;
    let out = device.shell(&command).await?;
    print!("{}", out.stdout_text());
    eprint!("{}", out.stderr_text());
    println!("exit code: {:?}", out.exit_code);
    Ok(())
}

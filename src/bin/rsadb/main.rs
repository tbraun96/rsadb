//! `rsadb` — a small command-line front end for the library.

mod cli;
mod commands;
mod connect;

use clap::Parser as _;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let args = cli::Cli::parse();
    match commands::run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("rsadb: {err}");
            ExitCode::from(cli::exit_code(&err))
        }
    }
}

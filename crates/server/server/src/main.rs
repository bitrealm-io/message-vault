//! The `message-crate-server` binary: parses the command line and runs it on
//! a Tokio runtime. Everything else lives in the library crate.

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    message_crate_server::logging::init();
    let cli = message_crate_server::cli::Cli::parse();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(message_crate_server::cli::run(cli))
}

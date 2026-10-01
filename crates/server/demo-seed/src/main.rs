//! Command-line entry point that writes the generated demo files.

use std::path::Path;

use anyhow::Result;
use clap::Parser;
use demo_seed::{DemoSize, SeedConfig};

#[derive(Parser)]
#[command(name = "demo-seed")]
#[command(
    about = "Generate the demo message dataset (iMessage, SMS Backup & Restore, WhatsApp) for Message Crate"
)]
struct Cli {
    /// Which built-in data set to write
    #[arg(long, value_enum, default_value_t = DemoSize::Medium)]
    size: DemoSize,

    /// Path to a settings file to use in place of a built-in size
    #[arg(long, conflicts_with = "size")]
    config: Option<String>,

    /// Output directory for the generated files. Overrides the path in the settings file.
    #[arg(long)]
    out: Option<String>,

    /// Random seed. Overrides the seed in the settings file.
    #[arg(long)]
    seed: Option<u64>,
}

/// Load settings, then write the demo files. Command-line flags override the
/// output path and seed from the settings file.
fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut cfg = match &cli.config {
        Some(path) => SeedConfig::load(Path::new(path))?,
        None => SeedConfig::for_size(cli.size)?,
    };
    if let Some(out) = cli.out {
        cfg.out = out;
    }
    if let Some(seed) = cli.seed {
        cfg.seed = seed;
    }
    demo_seed::generate(&cfg)?;
    Ok(())
}

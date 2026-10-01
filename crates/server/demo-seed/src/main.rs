//! Command-line entry point that writes the generated demo files.

use std::path::Path;

use anyhow::Result;
use clap::Parser;
use demo_seed::SeedConfig;

#[derive(Parser)]
#[command(name = "demo-seed")]
#[command(
    about = "Generate the demo message dataset (iMessage, SMS Backup & Restore, WhatsApp) for Message Crate"
)]
struct Cli {
    /// Path to the `demo_seed.toml` settings file
    #[arg(long, default_value_t = SeedConfig::default_path().display().to_string())]
    config: String,

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
    let mut cfg = SeedConfig::load(Path::new(&cli.config))?;
    if let Some(out) = cli.out {
        cfg.out = out;
    }
    if let Some(seed) = cli.seed {
        cfg.seed = seed;
    }
    demo_seed::generate(&cfg)?;
    Ok(())
}

//! Where the server's diagnostics go.
//!
//! Every runtime message, an internal error behind a 500, a warning the
//! importer could not act on, the request log, goes through `tracing` and
//! lands on stderr through one subscriber installed by [`init`]. `RUST_LOG`
//! picks the level (`info` when unset); the CLI subcommands keep printing
//! their own progress to stdout, which is their output, not a log.

use std::io::IsTerminal;

use tracing_subscriber::EnvFilter;

/// Install the stderr subscriber. Call once, before any work; a second call
/// is ignored so tests that build a server in-process do not panic.
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(false)
        .try_init();
}

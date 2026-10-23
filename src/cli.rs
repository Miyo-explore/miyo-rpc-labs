//! CLI argument parsing for Miyo RPC Labs.
//!
//! Uses clap derive macros. The CLI is intentionally flat: a single
//! top-level command (`check`) with an `--url` flag and an optional `--json`
//! flag for machine-readable output.

use clap::{Parser, Subcommand, ValueEnum};

/// Miyo RPC Labs — Stellar RPC diagnostics toolkit.
///
/// Diagnose endpoint connectivity, JSON-RPC compliance, and network
/// identification for Stellar JSON-RPC endpoints.
#[derive(Debug, Parser)]
#[command(
    name = "miyo-rpc",
    version,
    about = "Stellar RPC diagnostics and compatibility testing toolkit",
    long_about = "Miyo RPC Labs is an open-source toolkit for diagnosing Stellar\nJSON-RPC endpoints, verifying supported RPC behavior, and reproducing\ncompatibility issues using deterministic tests.\n\nRun 'miyo-rpc check --url <RPC_URL>' to run all supported diagnostic checks\nagainst a Stellar JSON-RPC endpoint."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Clone, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Colored, human-readable terminal output (default).
    #[value(name = "human")]
    Human,
    /// Machine-readable JSON output.
    #[value(name = "json")]
    Json,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    /// Run diagnostic checks against a Stellar RPC endpoint.
    Check(CheckArgs),
}

#[derive(Debug, Clone, Parser)]
pub struct CheckArgs {
    /// The Stellar JSON-RPC endpoint URL to diagnose.
    ///
    /// Example: https://soroban-testnet.stellar.org
    #[arg(short, long, value_name = "URL")]
    pub url: String,

    /// Output format: 'human' (default) or 'json'.
    #[arg(short, long, value_enum, default_value = "human")]
    pub output: OutputFormat,

    /// Request timeout in seconds (default: 15).
    #[arg(short = 't', long, default_value_t = 15)]
    pub timeout_secs: u64,

    /// RPC method to test in isolation (e.g. getHealth). Runs only that check.
    #[arg(short = 'm', long, value_name = "METHOD")]
    pub method: Option<String>,

    /// Number of diagnostic runs to perform (default: 1, max: 10).
    /// Repeats all checks and aggregates results across runs to measure
    /// reliability and latency distribution.
    #[arg(short = 'n', long = "runs", default_value_t = 1)]
    pub runs: u32,
}

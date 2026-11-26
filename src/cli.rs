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
    /// Diagnose multiple Stellar RPC endpoints listed in a file.
    Batch(BatchArgs),
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

    /// JSON-RPC `params` for the probed method: a JSON object, array, or null.
    /// Only applies when --method names a non-built-in method.
    #[arg(long, value_name = "JSON")]
    pub params: Option<String>,

    /// Read JSON-RPC `params` from a file (same JSON rules as --params).
    /// Only applies when --method names a non-built-in method.
    #[arg(long, value_name = "FILE")]
    pub params_file: Option<String>,

    /// Number of diagnostic runs to perform (default: 1, max: 10).
    /// Repeats all checks and aggregates results across runs to measure
    /// reliability and latency distribution.
    #[arg(short = 'n', long = "runs", default_value_t = 1)]
    pub runs: u32,

    /// Validate the endpoint's `getNetwork` passphrase against a known
    /// network (`testnet`, `mainnet`, or `futurenet`). A mismatch is reported as a FAIL.
    #[arg(long, value_name = "NETWORK")]
    pub expected_network: Option<String>,

    /// Compare the primary endpoint's network identity and latest ledger
    /// against a second endpoint (`<URL>`). Divergences are appended as
    /// `evidence` on the relevant checks (informational; not a failure).
    #[arg(long, value_name = "URL")]
    pub compare: Option<String>,

    /// Override the HTTP `User-Agent` header sent with requests
    /// (default: `miyo-rpc/<VERSION>`).
    #[arg(long, value_name = "STRING")]
    pub user_agent: Option<String>,

    /// Maximum response body size in bytes (default: 10 MiB).
    /// Must be greater than zero.
    #[arg(long, value_name = "N")]
    pub max_response_bytes: Option<usize>,

    /// Do not follow HTTP redirects (default: follow redirects).
    /// When set, a 3xx response is reported in the check result instead
    /// of being followed to a different host.
    #[arg(long)]
    pub no_follow_redirects: bool,

    /// Contract ID (strkey `C...`) whose instance and code TTL to check.
    /// The `contractTtl` check only runs when this is given; without it
    /// the check is absent from the report (default runs keep 12 checks).
    #[arg(long, value_name = "CONTRACT")]
    pub contract: Option<String>,

    /// TTL threshold in ledgers (default: 17280 ≈ 1 day). Remaining
    /// ledgers at or below this threshold report `Warn` (inclusive).
    /// Only applies when `--contract` is given.
    #[arg(long, value_name = "N")]
    pub ttl_threshold: Option<u64>,
}

/// Arguments for the `batch` subcommand (multiple endpoints from a file).
#[derive(Debug, Clone, Parser)]
pub struct BatchArgs {
    /// File with one endpoint URL per line. Blank lines and `#` comments are ignored.
    #[arg(short, long, value_name = "FILE")]
    pub file: String,

    /// Output format: 'human' (default) or 'json'.
    #[arg(short, long, value_enum, default_value = "human")]
    pub output: OutputFormat,

    /// Request timeout per endpoint in seconds (default: 15).
    #[arg(short = 't', long, default_value_t = 15)]
    pub timeout_secs: u64,

    /// Number of diagnostic runs per endpoint (default: 1, max: 10).
    #[arg(short = 'n', long = "runs", default_value_t = 1)]
    pub runs: u32,

    /// RPC method to test in isolation (e.g. getHealth). Runs only that check.
    #[arg(short = 'm', long, value_name = "METHOD")]
    pub method: Option<String>,

    /// Maximum endpoints to diagnose concurrently (default: 8, clamped to [1, 32]).
    #[arg(long, default_value_t = 8)]
    pub concurrency: usize,

    /// Validate each endpoint's `getNetwork` passport against a known network.
    #[arg(long, value_name = "NETWORK")]
    pub expected_network: Option<String>,
}

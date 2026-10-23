//! Miyo RPC Labs — Stellar RPC diagnostics toolkit.
//!
//! A minimal but functional CLI for diagnosing Stellar JSON-RPC endpoints,
//! verifying supported RPC behavior, and reproducing compatibility issues
//! using deterministic tests.
//!
//! See README.md for full documentation.

pub mod batch;
pub mod cli;
pub mod client;
pub mod diagnostics;
pub mod error;
pub mod network;
pub mod output;

pub use batch::{
    compute_summary, format_batch_human, format_batch_json, parse_endpoints, parse_endpoints_file,
    run_batch, validate_endpoint, BatchEntry, BatchParseError, BatchReport, BatchSummary,
    MAX_ENDPOINTS,
};
pub use cli::{BatchArgs, CheckArgs, Cli, Commands, OutputFormat};
pub use client::{RpcClient, RpcClientConfig};
pub use diagnostics::{
    CheckResult, CheckStatus, DiagnosticReport, IssueCategory, ReportSummary, RpcCheck,
};
pub use error::{MiyoError, Result};
pub use network::{known_networks, network_passphrase};
pub use output::sanitize_url;

/// Schema version for machine-readable JSON reports. Bumped (major) when the
/// JSON structure changes in a non-backwards-compatible way: a field is removed,
/// renamed, or its type changes. **Additive** changes — new fields appended to
/// the report or `checks` — do NOT require a bump.
///
/// Compatibility policy: `schema_version` is emitted as the first key of every
/// JSON report. Consumers MUST read it and MUST ignore unknown fields so that
/// additive schema evolution is transparent.
pub const SCHEMA_VERSION: &str = "1.0";

/// Current version, derived from Cargo.toml at build time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

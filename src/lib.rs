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
    contract_code_ledger_key, contract_instance_ledger_key, is_builtin_method, validate_params,
    ArbitraryMethodCheck, CheckResult, CheckStatus, ContractTtlCheck, DiagnosticReport,
    IssueCategory, ReportSummary, RetentionWindowCheck, RpcCheck, TtlCheckConfig, BUILTIN_METHODS,
    DEFAULT_MIN_RETENTION, DEFAULT_TTL_THRESHOLD, LEDGERS_PER_DAY,
};
pub use error::{classify_error_shape, ErrorCategory, MiyoError, Result};
pub use network::{decode_contract_id, encode_contract_id, known_networks, network_passphrase};
pub use output::{
    colors_enabled, format_human, format_human_with_color, format_json, sanitize_url,
};

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

//! Miyo RPC Labs — Stellar RPC diagnostics toolkit.
//!
//! A minimal but functional CLI for diagnosing Stellar JSON-RPC endpoints,
//! verifying supported RPC behavior, and reproducing compatibility issues
//! using deterministic tests.
//!
//! See README.md for full documentation.

pub mod cli;
pub mod client;
pub mod diagnostics;
pub mod error;
pub mod output;

pub use cli::{CheckArgs, Cli, Commands, OutputFormat};
pub use client::{RpcClient, RpcClientConfig};
pub use diagnostics::{CheckResult, CheckStatus, DiagnosticReport, IssueCategory, RpcCheck};
pub use error::{MiyoError, Result};
pub use output::sanitize_url;

/// Current version, derived from Cargo.toml at build time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

//! Batch endpoint diagnosis from a file.
//!
//! Runs the standard [`DiagnosticReport`] checks against every endpoint in a
//! newline-delimited file with bounded concurrency, and produces a single
//! aggregate [`BatchReport`] that is JSON-serializable and versioned via
//! `schema_version` (first key of the output).
//!
//! # Limits
//! - `--concurrency` is clamped to `[1, 32]` (default 8) — at most this many
//!   endpoints are diagnosed in parallel.
//! - At most [`MAX_ENDPOINTS`] endpoint lines are read from the file (DoS guard).
//! - `--runs` per endpoint is clamped to `[1, 10]`.

use crate::diagnostics::DiagnosticReport;
use crate::output::{format_json, sanitize_url};
use crate::SCHEMA_VERSION;
use crate::VERSION;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use url::Url;

/// Maximum endpoint lines read from a file (DoS guard).
pub const MAX_ENDPOINTS: usize = 1000;
/// Hard ceiling for `--concurrency`.
const MAX_CONCURRENCY: usize = 32;
/// Hard ceiling for `--runs` per endpoint.
const MAX_RUNS: u32 = 10;

/// One endpoint's result in a batch run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEntry {
    /// Sanitized endpoint URL (credentials redacted via [`crate::output::sanitize_url`]).
    pub endpoint: String,
    /// Whether the line parsed to a usable `http`/`https` URL.
    pub valid: bool,
    /// Pre-serialized diagnostic report (present only for valid endpoints).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<serde_json::Value>,
    /// Error message for an invalid line, e.g. a malformed URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Aggregate counts and exit code for a batch run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchSummary {
    /// Total lines attempted (valid + invalid).
    pub total: usize,
    /// Valid endpoints that received a diagnostic run.
    pub valid: usize,
    /// Invalid / unparseable lines.
    pub invalid: usize,
    /// Valid endpoints where every check passed.
    pub healthy: usize,
    /// Valid endpoints with at least one failed check.
    pub failed: usize,
    /// Aggregate exit code (see [`compute_summary`]).
    pub exit_code: u8,
}

/// Aggregate diagnostic report for a batch of endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchReport {
    pub schema_version: String,
    pub tool_version: String,
    pub runs: u32,
    pub endpoints: Vec<BatchEntry>,
    pub summary: BatchSummary,
}

/// Error reading or validating an endpoints file.
#[derive(Debug, Clone)]
pub struct BatchParseError(pub String);

impl std::fmt::Display for BatchParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for BatchParseError {}

/// Parse endpoint lines from a string.
///
/// One URL per line. Blank lines and lines beginning with `#` are ignored.
/// Duplicates are removed (first occurrence wins). Output is capped at
/// [`MAX_ENDPOINTS`] entries.
pub fn parse_endpoints(content: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if seen.insert(line.to_string()) {
            out.push(line.to_string());
            if out.len() >= MAX_ENDPOINTS {
                break;
            }
        }
    }
    out
}

/// Read an endpoints file and return the parsed (unvalidated) URLs.
pub fn parse_endpoints_file(path: &str) -> Result<Vec<String>, BatchParseError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| BatchParseError(format!("cannot read file '{}': {e}", path)))?;
    Ok(parse_endpoints(&content))
}

/// Validate one endpoint line to an `http`/`https` URL.
///
/// Returns the **original** URL (not sanitized) so it can be used for actual
/// network requests. Callers must sanitize the URL at every output boundary
/// via [`crate::output::sanitize_url`]. This separation prevents credential
/// loss: sanitizing before the HTTP request turns embedded credentials into
/// `REDACTED` placeholders, breaking authentication.
pub fn validate_endpoint(url: &str) -> Result<String, BatchParseError> {
    let parsed = Url::parse(url)
        .map_err(|e| BatchParseError(format!("'{}': not a valid URL ({e})", sanitize_url(url))))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(BatchParseError(format!(
            "'{}': scheme must be http or https",
            sanitize_url(url)
        )));
    }
    if parsed.host_str().is_none() {
        return Err(BatchParseError(format!(
            "'{}': missing hostname",
            sanitize_url(url)
        )));
    }
    // Return the original URL for network requests; sanitization happens
    // at output time to preserve embedded credentials for authentication.
    Ok(url.to_string())
}

/// Compute aggregate counts + exit code from batch entries (pure, testable).
///
/// Exit-code rules:
///   - `2`: no valid endpoints were diagnosed (file error / all-invalid / empty).
///   - `1`: at least one valid endpoint had a failing check.
///   - `0`: valid endpoints exist and all passed.
pub fn compute_summary(entries: &[BatchEntry]) -> BatchSummary {
    let total = entries.len();
    let valid = entries.iter().filter(|e| e.valid).count();
    let invalid = total - valid;
    let failed = entries
        .iter()
        .filter(|e| e.valid)
        .filter(|e| {
            e.report
                .as_ref()
                .and_then(|r| r.get("summary"))
                .and_then(|s| s.get("failed"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                > 0
        })
        .count();
    let healthy = valid.saturating_sub(failed);
    let exit_code = if valid == 0 {
        2
    } else if failed > 0 {
        1
    } else {
        0
    };
    BatchSummary {
        total,
        valid,
        invalid,
        healthy,
        failed,
        exit_code,
    }
}

fn clamp_runs(runs: u32) -> usize {
    std::cmp::min(runs.max(1), MAX_RUNS)
        .try_into()
        .unwrap_or(10)
}

fn empty_report(runs: usize, exit_code: u8) -> BatchReport {
    BatchReport {
        schema_version: SCHEMA_VERSION.to_string(),
        tool_version: VERSION.to_string(),
        runs: runs as u32,
        endpoints: Vec::new(),
        summary: BatchSummary {
            total: 0,
            valid: 0,
            invalid: 0,
            healthy: 0,
            failed: 0,
            exit_code,
        },
    }
}

/// Diagnose every endpoint in `args.file` with bounded concurrency.
///
/// Reuse happens entirely through `DiagnosticReport::run_all` + `format_json`
/// — no check logic is duplicated here.
pub async fn run_batch(args: &crate::BatchArgs) -> BatchReport {
    let runs = clamp_runs(args.runs);
    let concurrency = args.concurrency.clamp(1, MAX_CONCURRENCY);
    let timeout = Duration::from_secs(args.timeout_secs);

    let lines = match parse_endpoints_file(&args.file) {
        Ok(l) => l,
        Err(_) => return empty_report(runs, 2),
    };

    let validated: Vec<(String, Result<String, BatchParseError>)> = lines
        .into_iter()
        .map(|line| {
            let res = validate_endpoint(&line);
            (line, res)
        })
        .collect();

    let sem = Arc::new(Semaphore::new(concurrency));
    let mut set = tokio::task::JoinSet::new();
    let mut slots: Vec<Option<BatchEntry>> = vec![None; validated.len()];

    for (i, (raw, res)) in validated.into_iter().enumerate() {
        let sem = Arc::clone(&sem);
        let method = args.method.clone();
        let net = args.expected_network.clone();
        set.spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed");
            let entry = match res {
                Ok(url) => {
                    let mut report =
                        DiagnosticReport::run_all(&url, timeout, method.as_deref(), runs).await;
                    if let Some(expected) = net.as_deref() {
                        report.enforce_expected_network(Some(expected));
                    }
                    // Reuse the single-endpoint serializer so the per-endpoint
                    // JSON shape is identical to `check --output json`.
                    let value = format_json(&report)
                        .ok()
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
                    BatchEntry {
                        endpoint: sanitize_url(&url),
                        valid: true,
                        report: value,
                        error: None,
                    }
                }
                Err(e) => BatchEntry {
                    endpoint: sanitize_url(&raw),
                    valid: false,
                    report: None,
                    error: Some(e.0),
                },
            };
            (i, entry)
        });
    }

    while let Some(res) = set.join_next().await {
        if let Ok((i, entry)) = res {
            slots[i] = Some(entry);
        }
    }

    let endpoints: Vec<BatchEntry> = slots.into_iter().flatten().collect();
    let summary = compute_summary(&endpoints);
    BatchReport {
        schema_version: SCHEMA_VERSION.to_string(),
        tool_version: VERSION.to_string(),
        runs: runs as u32,
        endpoints,
        summary,
    }
}

/// Machine-readable JSON for a batch report (`schema_version` is the first key).
pub fn format_batch_json(report: &BatchReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

/// Human-readable summary of a batch report.
pub fn format_batch_human(report: &BatchReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("Miyo RPC Labs v{}\n", report.tool_version));
    out.push_str(&format!(
        "Batch diagnosis: {} endpoint(s)\n\n",
        report.endpoints.len()
    ));
    for entry in &report.endpoints {
        if entry.valid {
            let (passed, failed) = entry
                .report
                .as_ref()
                .map(|r| {
                    (
                        r.get("summary")
                            .and_then(|s| s.get("passed"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        r.get("summary")
                            .and_then(|s| s.get("failed"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                    )
                })
                .unwrap_or((0, 0));
            let status = if failed > 0 { "FAIL" } else { "PASS" };
            out.push_str(&format!(
                "  {} [{}] {} passed, {} failed\n",
                entry.endpoint, status, passed, failed
            ));
        } else {
            out.push_str(&format!(
                "  {} [INVALID] {}\n",
                entry.endpoint,
                entry.error.as_deref().unwrap_or("parse error")
            ));
        }
    }
    out.push_str(&format!(
        "\nSummary: {} total, {} valid, {} invalid, {} healthy, {} failed (exit code {})\n",
        report.summary.total,
        report.summary.valid,
        report.summary.invalid,
        report.summary.healthy,
        report.summary.failed,
        report.summary.exit_code
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_strips_blanks_and_comments() {
        let out = parse_endpoints("# comment\n\n  https://a.com  \n# another\nhttps://b.com\n");
        assert_eq!(out, vec!["https://a.com", "https://b.com"]);
    }

    #[test]
    fn test_parse_dedupes() {
        let out = parse_endpoints("https://a.com\nhttps://a.com\nhttps://b.com");
        assert_eq!(out, vec!["https://a.com", "https://b.com"]);
    }

    #[test]
    fn test_parse_empty() {
        assert!(parse_endpoints("").is_empty());
        assert!(parse_endpoints("# only comments\n\n").is_empty());
    }

    #[test]
    fn test_validate_endpoint_ok() {
        assert!(validate_endpoint("https://soroban-testnet.stellar.org").is_ok());
        assert!(validate_endpoint("http://localhost:8900/rpc").is_ok());
    }

    #[test]
    fn test_validate_endpoint_rejects() {
        assert!(validate_endpoint("ftp://nope.com").is_err());
        assert!(validate_endpoint("not a url").is_err());
        assert!(validate_endpoint("https://").is_err()); // no host
    }

    #[test]
    fn test_compute_summary_exit_codes() {
        let s = compute_summary(&[healthy_entry()]);
        assert_eq!(s.exit_code, 0);
        assert_eq!(s.valid, 1);
        assert_eq!(s.healthy, 1);
        assert_eq!(s.failed, 0);

        let entries = vec![healthy_entry(), failed_entry()];
        let s = compute_summary(&entries);
        assert_eq!(s.exit_code, 1);
        assert_eq!(s.valid, 2);
        assert_eq!(s.healthy, 1);
        assert_eq!(s.failed, 1);

        let s = compute_summary(&[invalid_entry()]);
        assert_eq!(s.exit_code, 2);
        assert_eq!(s.valid, 0);
        assert_eq!(s.invalid, 1);
    }

    #[test]
    fn test_batch_json_schema_version_and_structure() {
        let report = BatchReport {
            schema_version: SCHEMA_VERSION.to_string(),
            tool_version: "0.1.0".to_string(),
            runs: 1,
            endpoints: vec![healthy_entry()],
            summary: compute_summary(&[healthy_entry()]),
        };
        let json = format_batch_json(&report).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(
            parsed.get("schema_version").and_then(|v| v.as_str()),
            Some(SCHEMA_VERSION)
        );
        let endpoints = parsed.get("endpoints").and_then(|v| v.as_array()).unwrap();
        assert_eq!(endpoints.len(), 1);
        let summary = parsed.get("summary").and_then(|v| v.as_object()).unwrap();
        assert_eq!(summary.get("exit_code").and_then(|v| v.as_u64()), Some(0));
    }

    fn healthy_entry() -> BatchEntry {
        BatchEntry {
            endpoint: "https://a.com".into(),
            valid: true,
            report: Some(serde_json::json!({ "summary": { "passed": 1, "failed": 0 } })),
            error: None,
        }
    }
    fn failed_entry() -> BatchEntry {
        BatchEntry {
            endpoint: "https://b.com".into(),
            valid: true,
            report: Some(serde_json::json!({ "summary": { "passed": 0, "failed": 1 } })),
            error: None,
        }
    }
    fn invalid_entry() -> BatchEntry {
        BatchEntry {
            endpoint: "garbage".into(),
            valid: false,
            report: None,
            error: Some("bad".into()),
        }
    }

    #[test]
    fn test_batch_report_json_schema() {
        let report = BatchReport {
            schema_version: SCHEMA_VERSION.to_string(),
            tool_version: "0.1.0".to_string(),
            runs: 1,
            endpoints: vec![],
            summary: BatchSummary {
                total: 0,
                valid: 0,
                invalid: 0,
                healthy: 0,
                failed: 0,
                exit_code: 2,
            },
        };
        let json = format_batch_json(&report).expect("should serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(
            parsed.get("schema_version").and_then(|v| v.as_str()),
            Some("1.0")
        );
        assert_eq!(
            parsed.get("tool_version").and_then(|v| v.as_str()),
            Some("0.1.0")
        );
        assert_eq!(parsed.get("runs").and_then(|v| v.as_u64()), Some(1));
        assert!(parsed.get("endpoints").and_then(|v| v.as_array()).is_some());
        assert_eq!(
            parsed
                .get("summary")
                .and_then(|s| s.get("exit_code"))
                .and_then(|v| v.as_u64()),
            Some(2)
        );
    }

    // ────────────────────────
    // Credential-handling tests
    // ────────────────────────

    /// `validate_endpoint` must return the ORIGINAL url (with credentials
    /// intact) so the HTTP request can authenticate. Sanitization is an
    /// output-only concern handled in `run_batch`.
    #[test]
    fn test_validate_endpoint_preserves_query_credentials() {
        let url = "https://host.example/rpc?api_key=secret123";
        let result = validate_endpoint(url).expect("should validate");
        assert_eq!(
            result, url,
            "validate_endpoint must return the original URL, not a sanitized copy"
        );
    }

    #[test]
    fn test_validate_endpoint_preserves_userinfo() {
        let url = "https://user:secret123@host.example/rpc";
        let result = validate_endpoint(url).expect("should validate");
        assert_eq!(
            result, url,
            "validate_endpoint must return the original URL with userinfo intact"
        );
        assert!(
            !result.contains("REDACTED"),
            "original URL must not be pre-sanitized"
        );
    }

    /// Error messages for invalid URLs must not leak credentials.
    #[test]
    fn test_validate_endpoint_error_sanitizes_query_credentials() {
        // Wrong scheme — URL parses, but scheme is rejected.
        let url = "ftp://user:secret@host.example/rpc?api_key=leaked";
        let err = validate_endpoint(url).unwrap_err();
        assert!(!err.0.contains("leaked"), "error leaks api_key: {}", err.0);
        assert!(
            !err.0.contains("secret"),
            "error leaks userinfo password: {}",
            err.0
        );
        assert!(
            err.0.contains("REDACTED"),
            "error should contain REDACTED: {}",
            err.0
        );
    }

    #[test]
    fn test_validate_endpoint_error_sanitizes_malformed_with_credentials() {
        // Unparseable URL with apparent userinfo.
        let url = "https://user:secret@bad host.example/rpc";
        let err = validate_endpoint(url).unwrap_err();
        assert!(!err.0.contains("secret"), "error leaks password: {}", err.0);
        assert!(
            err.0.contains("REDACTED"),
            "error should contain REDACTED: {}",
            err.0
        );
    }

    /// Existing sanitization behavior must remain correct after the fallback
    /// enhancement for unparseable URLs.
    #[test]
    fn test_sanitize_url_malformed_no_credentials_unchanged() {
        let url = "not a url at all";
        assert_eq!(crate::output::sanitize_url(url), url);
    }
}

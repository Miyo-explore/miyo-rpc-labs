//! Diagnostic checks for Stellar JSON-RPC endpoints.
//!
//! Each check is a small, self-contained function that exercises one
//! aspect of the RPC endpoint (connectivity, a specific RPC method,
//! response structure validation, etc.).
//!
//! Checks are composable: [`RpcCheck::run_all`] runs every check in
//! sequence and collects the results.

use crate::client::{RpcClient, RpcClientConfig};
use crate::error::MiyoError;
use crate::output::sanitize_url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

/// The outcome of a diagnostic check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// The check completed and the endpoint behaved as expected.
    Pass,
    /// The check completed but the endpoint did not behave as expected.
    Fail,
    /// The check could not be performed (e.g., a prior check failed and this
    /// check depends on it, or the method is not applicable).
    Skip,
}

/// Determine the worst (highest-precedence) status from a set of results.
///
/// Precedence: Fail > Skip > Pass. A `Fail` in any run must never be hidden
/// by `Skip` or `Pass` in another run.
pub fn worst_status(statuses: &[CheckStatus]) -> CheckStatus {
    if statuses.contains(&CheckStatus::Fail) {
        CheckStatus::Fail
    } else if statuses.contains(&CheckStatus::Skip) {
        CheckStatus::Skip
    } else {
        CheckStatus::Pass
    }
}

/// Coarse category of an issue when a check fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCategory {
    /// DNS, connection, timeout.
    Transport,
    /// HTTP 4xx or 5xx status.
    HttpError,
    /// Response body was not valid JSON / malformed RPC structure.
    MalformedResponse,
    /// JSON-RPC error object returned by the server.
    JsonRpcError,
    /// Method not available on this endpoint.
    UnsupportedMethod,
    /// Response field missing or wrong type.
    MissingField,
    /// Any other issue.
    Other,
}

impl IssueCategory {
    pub fn from_error(err: &MiyoError) -> Self {
        use crate::error::ErrorCategory;
        match err.category() {
            ErrorCategory::Transport => IssueCategory::Transport,
            ErrorCategory::ServerError => IssueCategory::HttpError,
            ErrorCategory::ClientError => IssueCategory::HttpError,
            ErrorCategory::MalformedResponse => IssueCategory::MalformedResponse,
            ErrorCategory::UnsupportedMethod => IssueCategory::UnsupportedMethod,
            ErrorCategory::JsonRpcError => IssueCategory::JsonRpcError,
            ErrorCategory::Configuration => IssueCategory::Other,
            ErrorCategory::Unknown => IssueCategory::Other,
        }
    }
}

/// The result of a single diagnostic check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    /// Name of the check (e.g., "getHealth").
    pub name: String,
    /// Overall status.
    pub status: CheckStatus,
    /// When the check was performed (ISO 8601).
    pub timestamp: String,
    /// Duration of the underlying request. Only present if a request was made.
    pub duration_ms: Option<u64>,
    /// HTTP status code if an HTTP response was received.
    pub http_status: Option<u16>,
    /// Sanitized endpoint identifier (no secrets).
    pub endpoint: String,
    /// For Pass/Skip: summary fields from the response.
    /// For Fail: structured error details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<IssueCategory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// Steps to reproduce the observed behavior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

/// A single diagnostic check. Implementors define the `run` method.
pub trait RpcCheck {
    /// Human-readable name of this check.
    fn name(&self) -> &str;

    /// Run the check against the given client. Returns a [`CheckResult`].
    fn run<'a>(
        &'a self,
        client: &'a RpcClient,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult> + Send + 'a>>;
}

/// Aggregate report of all checks run against an endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticReport {
    /// Tool version.
    pub tool_version: String,
    /// Sanitized endpoint URL.
    pub endpoint: String,
    /// Number of diagnostic runs performed (1 for a single pass).
    pub runs: usize,
    /// All individual check results.
    pub checks: Vec<CheckResult>,
    /// Overall summary.
    pub summary: ReportSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSummary {
    /// Total checks executed (not skipped).
    pub ran: u32,
    /// Number of passing checks.
    pub passed: u32,
    /// Number of failing checks.
    pub failed: u32,
    /// Number of skipped checks.
    pub skipped: u32,
    /// Total number of diagnostic runs.
    pub total_runs: u32,
}

/// A concrete check implementation for a Stellar RPC method that takes no params.
#[derive(Debug, Clone)]
pub struct SimpleMethodCheck {
    pub method_name: &'static str,
    pub description: &'static str,
    /// Field name in the result to extract as the "primary value" (optional).
    pub primary_field: Option<&'static str>,
}

impl RpcCheck for SimpleMethodCheck {
    fn name(&self) -> &str {
        self.method_name
    }

    fn run<'a>(
        &'a self,
        client: &'a RpcClient,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult> + Send + 'a>> {
        let method_name = self.method_name;
        let description = self.description;
        let primary_field = self.primary_field;
        Box::pin(async move {
            let endpoint = client.sanitized_url();
            let timestamp = chrono::Utc::now().to_rfc3339();

            match client.call::<Value>(method_name, None).await {
                Ok((info, result)) => {
                    let duration_ms = info.duration.as_millis() as u64;
                    let mut details = serde_json::Map::new();
                    details.insert("description".to_string(), Value::String(description.to_string()));
                    details.insert("http_status".to_string(), Value::Number(serde_json::Number::from(info.status)));

                    if let Some(field) = primary_field {
                        if let Some(val) = result.get(field) {
                            details.insert(field.to_string(), val.clone());
                        }
                    }

                    // Include the full result for the JSON report, but only
                    // for fields that are short and non-sensitive.
                    let result_str = serde_json::to_string(&result);
                    if let Ok(s) = &result_str {
                        if s.len() < 2048 {
                            if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(s) {
                                for (k, v) in map {
                                    details.entry(k).or_insert(v);
                                }
                            }
                        }
                    }

                    CheckResult {
                        name: method_name.to_string(),
                        status: CheckStatus::Pass,
                        timestamp,
                        duration_ms: Some(duration_ms),
                        http_status: Some(info.status),
                        endpoint: endpoint.clone(),
                        details: if details.is_empty() { None } else { Some(details) },
                        error: None,
                        error_message: None,
                        evidence: Some(format!(
                            "Sent JSON-RPC 2.0 POST to {endpoint} with method '{method_name}' and no params.",
                        )),
                    }
                }
                Err(e) => fail_result(
                    method_name,
                    &endpoint,
                    timestamp,
                    &e,
                    format!("Send a JSON-RPC 2.0 POST to {endpoint} with method '{method_name}' and empty params."),
                ),
            }
        })
    }
}

/// Check raw HTTP connectivity and status code — independent of JSON-RPC.
pub async fn http_connectivity_check(
    url: &str,
    timeout: Duration,
    user_agent: &str,
) -> CheckResult {
    let endpoint = sanitize_url(url);
    let timestamp = chrono::Utc::now().to_rfc3339();
    let method = "httpConnectivity";

    // We use a plain reqwest client here (not RpcClient) to check raw HTTP.
    let client = match reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(user_agent)
        .build()
    {
        Ok(c) => c,
        Err(_e) => {
            return fail_result(
                method,
                &endpoint,
                timestamp,
                &MiyoError::Network("failed to build HTTP client".to_string()),
                format!("Construct an HTTP client for {endpoint}."),
            );
        }
    };

    let started = std::time::Instant::now();
    let response = client.get(url).send().await;
    let duration = started.elapsed();

    match response {
        Ok(resp) => {
            let status = resp.status().as_u16();

            let mut details = serde_json::Map::new();
            details.insert("endpoint_reachable".to_string(), Value::Bool(true));
            details.insert(
                "http_status".to_string(),
                Value::Number(serde_json::Number::from(status)),
            );

            CheckResult {
                name: method.to_string(),
                status: if status < 500 { CheckStatus::Pass } else { CheckStatus::Fail },
                timestamp,
                duration_ms: Some(duration.as_millis() as u64),
                http_status: Some(status),
                endpoint: endpoint.clone(),
                details: Some(details),
                error: if status >= 500 { Some(IssueCategory::HttpError) } else { None },
                error_message: if status >= 500 {
                    Some(format!("HTTP {status} — server error"))
                } else {
                    None
                },
                evidence: Some(format!(
                    "Issued HTTP GET to {endpoint} with timeout {timeout:?} and User-Agent '{user_agent}'."
                )),
            }
        }
        Err(e) if e.is_timeout() => fail_result(
            method,
            &endpoint,
            timestamp,
            &MiyoError::Timeout(timeout),
            format!("Issue HTTP GET to {endpoint}."),
        ),
        Err(e) if e.is_connect() => fail_result(
            method,
            &endpoint,
            timestamp,
            &MiyoError::Network(format!("connection failed for {endpoint}")),
            format!("Issue HTTP GET to {endpoint}."),
        ),
        Err(_e) => fail_result(
            method,
            &endpoint,
            timestamp,
            &MiyoError::Network(format!("network failure for {endpoint}")),
            format!("Issue HTTP GET to {endpoint}."),
        ),
    }
}

/// Build a failure result from an error.
fn fail_result(
    name: &str,
    endpoint: &str,
    timestamp: String,
    err: &MiyoError,
    evidence: String,
) -> CheckResult {
    let message = err.to_string();
    let category = IssueCategory::from_error(err);

    CheckResult {
        name: name.to_string(),
        status: CheckStatus::Fail,
        timestamp,
        duration_ms: None,
        http_status: None,
        endpoint: endpoint.to_string(),
        details: None,
        error: Some(category),
        error_message: Some(message),
        evidence: Some(evidence),
    }
}

/// All built-in checks that the `check` command runs by default.
pub fn all_checks() -> Vec<SimpleMethodCheck> {
    vec![
        SimpleMethodCheck {
            method_name: "getHealth",
            description: "Node health status and ledger window bounds.",
            primary_field: Some("status"),
        },
        SimpleMethodCheck {
            method_name: "getNetwork",
            description: "Network passphrase and protocol version.",
            primary_field: Some("passphrase"),
        },
        SimpleMethodCheck {
            method_name: "getLatestLedger",
            description: "Latest known ledger sequence, hash, and close time.",
            primary_field: Some("sequence"),
        },
        SimpleMethodCheck {
            method_name: "getVersionInfo",
            description: "RPC server version and Captive Core version.",
            primary_field: Some("version"),
        },
    ]
}

impl DiagnosticReport {
    /// Run all diagnostic checks against the endpoint and return a full report.
    pub async fn run_all(
        url: &str,
        timeout: Duration,
        method_filter: Option<&str>,
        runs: usize,
    ) -> Self {
        let endpoint = sanitize_url(url);
        let config = RpcClientConfig::new(timeout);
        let client = match RpcClient::new(url, config) {
            Ok(c) => c,
            Err(e) => {
                // Can't even build a client — every check fails.
                let timestamp = chrono::Utc::now().to_rfc3339();
                let user_agent = format!("miyo-rpc/{}", crate::VERSION);

                let mut checks: Vec<CheckResult> = all_checks()
                    .iter()
                    .map(|c| {
                        fail_result(
                            c.method_name,
                            &endpoint,
                            timestamp.clone(),
                            &e,
                            format!("Build RpcClient for {endpoint}."),
                        )
                    })
                    .collect();

                // Also add HTTP connectivity as a check.
                checks.insert(0, http_connectivity_check(url, timeout, &user_agent).await);

                // Client initialization failed — no retry loop was executed.
                // Report completed runs as 1 (the single connectivity attempt).
                let summary = summarize(&checks, 1);
                return Self {
                    tool_version: crate::VERSION.to_string(),
                    endpoint,
                    runs: 1,
                    checks,
                    summary,
                };
            }
        };

        let user_agent = format!("miyo-rpc/{}", crate::VERSION);

        // Run diagnostics `runs` times, collecting raw results for aggregation.
        let mut per_run: Vec<Vec<CheckResult>> = Vec::with_capacity(runs);
        for _ in 0..runs {
            let mut run_results: Vec<CheckResult> =
                vec![http_connectivity_check(url, timeout, &user_agent).await];

            let checks = all_checks();
            for check in &checks {
                if let Some(m) = method_filter {
                    if m != check.method_name {
                        run_results.push(CheckResult {
                            name: check.method_name.to_string(),
                            status: CheckStatus::Skip,
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            duration_ms: None,
                            http_status: None,
                            endpoint: endpoint.clone(),
                            details: None,
                            error: None,
                            error_message: None,
                            evidence: Some(
                                "Skipped: --method filter does not match this check.".to_string(),
                            ),
                        });
                        continue;
                    }
                }
                run_results.push(check.run(&client).await);
            }
            per_run.push(run_results);
        }

        // Aggregate across runs (worst status wins, durations averaged).
        let checks = aggregate(&per_run);
        let summary = summarize(&checks, runs);
        Self {
            tool_version: crate::VERSION.to_string(),
            endpoint,
            runs,
            checks,
            summary,
        }
    }
}

fn summarize(checks: &[CheckResult], runs: usize) -> ReportSummary {
    let mut s = ReportSummary {
        ran: 0,
        passed: 0,
        failed: 0,
        skipped: 0,
        total_runs: runs as u32,
    };
    for c in checks {
        match c.status {
            CheckStatus::Pass => {
                s.ran += 1;
                s.passed += 1;
            }
            CheckStatus::Fail => {
                s.ran += 1;
                s.failed += 1;
            }
            CheckStatus::Skip => s.skipped += 1,
        }
    }
    s
}

/// Aggregate results from multiple runs into a single summary per check.
///
/// The status of each check is the "worst" across all runs (Fail > Skip > Pass),
/// and `duration_ms` becomes the average. When more than one run was performed,
/// latency statistics (min/max/avg) and `run_count` are added to `details`.
fn aggregate(per_run: &[Vec<CheckResult>]) -> Vec<CheckResult> {
    if per_run.is_empty() || per_run[0].is_empty() {
        return Vec::new();
    }

    let check_names: Vec<&str> = per_run[0].iter().map(|c| c.name.as_str()).collect();
    let mut aggregated = Vec::with_capacity(check_names.len());

    for name in &check_names {
        let hits: Vec<&CheckResult> = per_run
            .iter()
            .filter_map(|run| run.iter().find(|c| c.name == *name))
            .collect();
        let first = *hits.first().expect("each run has the same check names");

        // Worst status wins (Fail > Skip > Pass).
        let statuses: Vec<CheckStatus> = hits.iter().map(|c| c.status).collect();
        let status = worst_status(&statuses);

        let durations: Vec<u64> = hits.iter().filter_map(|c| c.duration_ms).collect();
        let avg_ms = if durations.is_empty() {
            None
        } else {
            Some(durations.iter().sum::<u64>() / durations.len() as u64)
        };

        let mut result = CheckResult::clone(first);
        result.status = status;
        result.duration_ms = avg_ms;

        if per_run.len() > 1 && !durations.is_empty() {
            let mut details = result.details.clone().unwrap_or_default();
            details.insert("run_count".to_string(), Value::from(per_run.len()));
            details.insert(
                "latency_min_ms".to_string(),
                Value::from(durations.iter().copied().min().unwrap()),
            );
            details.insert(
                "latency_max_ms".to_string(),
                Value::from(durations.iter().copied().max().unwrap()),
            );
            details.insert(
                "latency_avg_ms".to_string(),
                Value::from(avg_ms.expect("avg_ms computed when durations are non-empty") as f64),
            );
            result.details = Some(details);
        }

        aggregated.push(result);
    }

    aggregated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(name: &str, status: CheckStatus, duration_ms: Option<u64>) -> CheckResult {
        CheckResult {
            name: name.to_string(),
            status,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            duration_ms,
            http_status: None,
            endpoint: "https://REDACTED".to_string(),
            details: None,
            error: None,
            error_message: None,
            evidence: None,
        }
    }

    #[test]
    fn test_worst_status_precedence() {
        // Fail beats everything
        assert_eq!(
            worst_status(&[CheckStatus::Pass, CheckStatus::Fail]),
            CheckStatus::Fail
        );
        assert_eq!(
            worst_status(&[CheckStatus::Skip, CheckStatus::Fail]),
            CheckStatus::Fail
        );
        // Skip beats Pass (but not Fail)
        assert_eq!(
            worst_status(&[CheckStatus::Pass, CheckStatus::Skip]),
            CheckStatus::Skip
        );
        // Same statuses
        assert_eq!(
            worst_status(&[CheckStatus::Pass, CheckStatus::Pass]),
            CheckStatus::Pass
        );
        assert_eq!(
            worst_status(&[CheckStatus::Skip, CheckStatus::Skip]),
            CheckStatus::Skip
        );
        assert_eq!(
            worst_status(&[CheckStatus::Fail, CheckStatus::Fail]),
            CheckStatus::Fail
        );
    }

    #[test]
    fn test_aggregate_fail_beats_pass() {
        let per_run = vec![
            vec![make_result("x", CheckStatus::Pass, Some(100))],
            vec![make_result("x", CheckStatus::Fail, None)],
        ];
        let result = aggregate(&per_run);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, CheckStatus::Fail);
    }

    #[test]
    fn test_aggregate_skip_does_not_hide_fail() {
        let per_run = vec![
            vec![
                make_result("pass", CheckStatus::Pass, Some(100)),
                make_result("skip", CheckStatus::Skip, None),
            ],
            vec![
                make_result("pass", CheckStatus::Fail, None),
                make_result("skip", CheckStatus::Skip, None),
            ],
        ];
        let result = aggregate(&per_run);
        let pass = result.iter().find(|c| c.name == "pass").unwrap();
        assert_eq!(pass.status, CheckStatus::Fail);
        let skip = result.iter().find(|c| c.name == "skip").unwrap();
        assert_eq!(skip.status, CheckStatus::Skip);
    }

    #[test]
    fn test_aggregate_pass_plus_skip_to_skip() {
        let per_run = vec![
            vec![make_result("x", CheckStatus::Pass, Some(100))],
            vec![make_result("x", CheckStatus::Skip, None)],
        ];
        let result = aggregate(&per_run);
        assert_eq!(result[0].status, CheckStatus::Skip);
        // avg of single duration = 100
        assert_eq!(result[0].duration_ms, Some(100));
        let details = result[0].details.as_ref().unwrap();
        assert_eq!(details.get("latency_min_ms"), Some(&Value::from(100u64)));
        assert_eq!(details.get("latency_max_ms"), Some(&Value::from(100u64)));
        assert_eq!(details.get("run_count"), Some(&Value::from(2u64)));
    }

    #[test]
    fn test_aggregate_transient_failure() {
        let per_run = vec![
            vec![make_result("getHealth", CheckStatus::Pass, Some(120))],
            vec![make_result("getHealth", CheckStatus::Pass, Some(90))],
            vec![make_result("getHealth", CheckStatus::Fail, None)],
        ];
        let result = aggregate(&per_run);
        assert_eq!(result[0].status, CheckStatus::Fail);
        // avg = (120 + 90) / 2 = 105 (only 2 runs had durations)
        assert_eq!(result[0].duration_ms, Some(105));
        let details = result[0].details.as_ref().unwrap();
        assert_eq!(details.get("latency_min_ms"), Some(&Value::from(90u64)));
        assert_eq!(details.get("latency_max_ms"), Some(&Value::from(120u64)));
        assert_eq!(details.get("run_count"), Some(&Value::from(3u64)));
    }

    #[test]
    fn test_single_run_aggregation_unchanged() {
        let per_run = vec![vec![make_result("x", CheckStatus::Pass, Some(100))]];
        let result = aggregate(&per_run);
        assert_eq!(result[0].status, CheckStatus::Pass);
        assert_eq!(result[0].duration_ms, Some(100));
        // No latency stats for single run
        assert!(
            result[0].details.is_none()
                || !result[0]
                    .details
                    .as_ref()
                    .unwrap()
                    .contains_key("latency_min_ms")
        );
    }

    #[test]
    fn test_summarize_total_runs_field() {
        let checks = vec![make_result("x", CheckStatus::Pass, Some(100))];
        let summary = summarize(&checks, 5);
        assert_eq!(summary.total_runs, 5);
        assert_eq!(summary.ran, 1);
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 0);
    }
}

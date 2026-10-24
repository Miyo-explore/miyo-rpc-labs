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

/// Names of the methods that have a dedicated built-in check.
pub const BUILTIN_METHODS: &[&str] = &[
    "getHealth",
    "getNetwork",
    "getLatestLedger",
    "getVersionInfo",
];

/// Whether `name` refers to one of the built-in checks ([`BUILTIN_METHODS`]).
pub fn is_builtin_method(name: &str) -> bool {
    BUILTIN_METHODS.contains(&name)
}

/// Validate a JSON-RPC `params` payload supplied as text.
///
/// Accepts a JSON object, array, or `null` (null maps to `Ok(None)`, meaning
/// "no params"). Returns a human-readable error for malformed JSON or a scalar
/// shape (JSON-RPC params must be an object or array).
pub fn validate_params(text: &str) -> Result<Option<Value>, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("--params is not valid JSON: {e}"))?;
    match value {
        Value::Null => Ok(None),
        Value::Object(_) | Value::Array(_) => Ok(Some(value)),
        _ => Err("--params must be a JSON object, array, or null".to_string()),
    }
}

/// Describe params for the evidence line without ever echoing raw values.
fn describe_params(params: &Option<Value>) -> &'static str {
    match params {
        None | Some(Value::Null) => "no params",
        Some(_) => "params (values redacted)",
    }
}

/// Insert a JSON-RPC `result` into `details`, representing object and
/// non-object shapes consistently and flagging truncation explicitly.
fn insert_result(details: &mut serde_json::Map<String, Value>, result: &Value) {
    let serialized = serde_json::to_string(result).unwrap_or_default();
    if serialized.len() < 2048 {
        match result {
            Value::Object(map) => {
                for (k, v) in map {
                    details.entry(k.clone()).or_insert_with(|| v.clone());
                }
            }
            other => {
                details.insert("result".to_string(), other.clone());
            }
        }
    } else {
        details.insert("result_truncated".to_string(), Value::Bool(true));
        details.insert(
            "result_size_bytes".to_string(),
            Value::Number(serde_json::Number::from(serialized.len())),
        );
    }
}

/// A probe for a single user-specified JSON-RPC method.
///
/// Unlike [`SimpleMethodCheck`], this carries caller-supplied `params` and does
/// not assume a fixed result shape: the parsed `result` is represented in the
/// report as-is (object keys merged into `details`, non-objects under
/// `result`), with explicit truncation metadata when it exceeds the size cap.
#[derive(Clone)]
pub struct ArbitraryMethodCheck {
    pub method_name: String,
    pub params: Option<Value>,
}

impl std::fmt::Debug for ArbitraryMethodCheck {
    /// Debug output never includes `params`: a derived `Debug` would print raw
    /// caller-supplied values (which may be secrets) into any log or trace.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArbitraryMethodCheck")
            .field("method_name", &self.method_name)
            .field("params", &"<redacted>")
            .finish()
    }
}

impl RpcCheck for ArbitraryMethodCheck {
    fn name(&self) -> &str {
        &self.method_name
    }

    fn run<'a>(
        &'a self,
        client: &'a RpcClient,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult> + Send + 'a>> {
        let method_name = self.method_name.clone();
        let params = self.params.clone();
        let params_desc = describe_params(&params);
        Box::pin(async move {
            let endpoint = client.sanitized_url();
            let timestamp = chrono::Utc::now().to_rfc3339();

            match client.call::<Value>(&method_name, params).await {
                Ok((info, result)) => {
                    let duration_ms = info.duration.as_millis() as u64;
                    let mut details = serde_json::Map::new();
                    details.insert(
                        "description".to_string(),
                        Value::String("Arbitrary JSON-RPC method probe.".to_string()),
                    );
                    details.insert(
                        "http_status".to_string(),
                        Value::Number(serde_json::Number::from(info.status)),
                    );
                    insert_result(&mut details, &result);

                    CheckResult {
                        name: method_name.clone(),
                        status: CheckStatus::Pass,
                        timestamp,
                        duration_ms: Some(duration_ms),
                        http_status: Some(info.status),
                        endpoint: endpoint.clone(),
                        details: if details.is_empty() { None } else { Some(details) },
                        error: None,
                        error_message: None,
                        evidence: Some(format!(
                            "Sent JSON-RPC 2.0 POST to {endpoint} with method '{method_name}' and {params_desc}."
                        )),
                    }
                }
                Err(e) => fail_result(
                    &method_name,
                    &endpoint,
                    timestamp,
                    &e,
                    format!("Send a JSON-RPC 2.0 POST to {endpoint} with method '{method_name}' and {params_desc}."),
                ),
            }
        })
    }
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
        let mut checks = aggregate(&per_run);
        analyze_ledger_health(&per_run, &mut checks);
        let summary = summarize(&checks, runs);
        Self {
            tool_version: crate::VERSION.to_string(),
            endpoint,
            runs,
            checks,
            summary,
        }
    }

    /// Run a single arbitrary JSON-RPC method probe (plus the raw HTTP
    /// connectivity check) against the endpoint, `runs` times.
    ///
    /// Used when `--method` names a method that is not one of the built-in
    /// checks. The method is called with the caller-supplied `params`. There is
    /// no `getLatestLedger` check here, so ledger-health analysis is a no-op.
    pub async fn run_arbitrary_probe(
        url: &str,
        timeout: Duration,
        method: &str,
        params: Option<Value>,
        runs: usize,
    ) -> Self {
        // Guard against a false "success" on an empty report: a probe must run
        // at least once. The CLI already clamps to [1, 10]; this keeps the
        // library API safe for any caller.
        let runs = runs.max(1);
        let endpoint = sanitize_url(url);
        let config = RpcClientConfig::new(timeout);
        let client = match RpcClient::new(url, config) {
            Ok(c) => c,
            Err(e) => {
                let timestamp = chrono::Utc::now().to_rfc3339();
                let user_agent = format!("miyo-rpc/{}", crate::VERSION);
                let mut checks: Vec<CheckResult> =
                    vec![http_connectivity_check(url, timeout, &user_agent).await];
                checks.push(fail_result(
                    method,
                    &endpoint,
                    timestamp,
                    &e,
                    format!("Build RpcClient for {endpoint}."),
                ));
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

        let mut per_run: Vec<Vec<CheckResult>> = Vec::with_capacity(runs);
        for _ in 0..runs {
            let mut run_results = vec![http_connectivity_check(url, timeout, &user_agent).await];
            let probe = ArbitraryMethodCheck {
                method_name: method.to_string(),
                params: params.clone(),
            };
            run_results.push(probe.run(&client).await);
            per_run.push(run_results);
        }

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

    /// Apply `--expected-network` validation after a report has been generated.
    ///
    /// If `expected` is a known network name, the `getNetwork` check's
    /// passphrase detail is compared against the known passphrase. A mismatch
    /// downgrades the check to `Fail` and the summary is recomputed so
    /// `passed`/`failed`/`ran` stay consistent. Checks that already failed or
    /// are skipped are left unchanged; an unknown network name is a no-op
    /// (the CLI rejects unknown names before reaching this point).
    pub fn enforce_expected_network(&mut self, expected: Option<&str>) {
        if let Some(name) = expected {
            enforce_network_identity(&mut self.checks, name);
            self.summary = summarize(&self.checks, self.runs);
        }
    }

    /// Compare this report's network identity and ledger state against a
    /// second endpoint's report.
    ///
    /// Divergences are appended to the relevant check's `evidence` field and
    /// do **not** change the check status or the exit-code contract — the
    /// comparison is informational, intended to flag misconfigured or
    /// divergent providers. Call this after `enforce_expected_network` so
    /// passport validation applies to the primary report first.
    pub fn compare_with(&mut self, other: &DiagnosticReport) {
        compare_network_identity(&mut self.checks, &other.checks, &other.endpoint);
        compare_ledger_state(&mut self.checks, &other.checks, &other.endpoint);
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

/// Validate the `getNetwork` check's passport against an expected network.
///
/// Looks up the passphrase for `expected_name`, then compares it to the
/// `passphrase` value in the `getNetwork` check result's `details`. On
/// mismatch, the check is downgraded to `Fail` with `IssueCategory::Other` and
/// a descriptive `error_message`/`evidence`. Already-failing checks and
/// skipped checks are left untouched (a skipped `getNetwork` cannot be
/// validated).
fn enforce_network_identity(checks: &mut [CheckResult], expected_name: &str) {
    let passphrase = match crate::network::network_passphrase(expected_name) {
        Some(p) => p,
        None => return,
    };
    if let Some(net) = checks.iter_mut().find(|c| c.name == "getNetwork") {
        if net.status == CheckStatus::Fail {
            return;
        }
        let actual = net
            .details
            .as_ref()
            .and_then(|d| d.get("passphrase"))
            .and_then(Value::as_str);
        if let Some(actual) = actual {
            if actual != passphrase {
                net.status = CheckStatus::Fail;
                net.error = Some(IssueCategory::Other);
                net.error_message = Some(format!(
                    "network passphrase mismatch: expected {passphrase:?}, got {actual:?}"
                ));
                net.evidence = Some(format!(
                    "Validate getNetwork passphrase against --expected-network {expected_name:?}."
                ));
            }
        }
    }
}

/// Append a ledger-health analysis to the `getLatestLedger` check's evidence.
///
/// Compares the ledger `sequence` and `id` (hash) across runs. Because checks
/// execute back-to-back with no delay, a repeated sequence is **not** treated
/// as proof of staleness — we surface "possibly stale" evidence rather than a
/// hard `Fail` — so a verdict is only ever attached to evidence the data can
/// substantiate. A strictly increasing sequence is healthy progress; an
/// identical sequence *and* identical hash across consecutive runs is
/// "possibly stale"; an identical sequence with a changing hash is a ledger
/// inconsistency. If there is no successful `getLatestLedger` run this is a
/// no-op (the check is already `Fail`).
fn analyze_ledger_health(per_run: &[Vec<CheckResult>], checks: &mut [CheckResult]) {
    let ledger = match checks.iter_mut().find(|c| c.name == "getLatestLedger") {
        Some(c) => c,
        None => return,
    };

    // Collect sequence + hash from every run that produced a result.
    let mut seqs: Vec<String> = Vec::new();
    let mut hashes: Vec<String> = Vec::new();
    for run in per_run {
        if let Some(lr) = run.iter().find(|c| c.name == "getLatestLedger") {
            if let Some(details) = &lr.details {
                if let Some(seq) = details.get("sequence").and_then(|v| v.as_str()) {
                    seqs.push(seq.to_string());
                }
                if let Some(h) = details.get("id").and_then(|v| v.as_str()) {
                    hashes.push(h.to_string());
                }
            }
        }
    }

    // No successful getLatestLedger runs — already Failed by the aggregator.
    if seqs.is_empty() {
        return;
    }

    let existing = ledger.evidence.take().unwrap_or_default();
    let verdict = if seqs.len() == 1 {
        let hash = hashes.first().and_then(|h| h.get(..8)).unwrap_or("?");
        format!(
            " Ledger sequence {}, hash {}: self-run freshness inconclusive — use --runs > 1.",
            seqs[0], hash
        )
    } else {
        let unique_seqs: std::collections::HashSet<&str> =
            seqs.iter().map(String::as_str).collect();
        let unique_hashes: std::collections::HashSet<&str> =
            hashes.iter().map(String::as_str).collect();
        if unique_seqs.len() > 1 {
            format!(
                " Ledger advanced from {} to {} across {} runs — healthy.",
                seqs.first().unwrap(),
                seqs.last().unwrap(),
                seqs.len()
            )
        } else if unique_hashes.len() == 1 {
            let hash = hashes.first().and_then(|h| h.get(..8)).unwrap_or("?");
            format!(
                " Ledger sequence {} (hash {}:) unchanged across {} consecutive runs — possibly stale endpoint.",
                seqs[0], hash, seqs.len()
            )
        } else {
            format!(
                " Ledger sequence {} stable but hash changed across {} runs — possible ledger inconsistency.",
                seqs[0], seqs.len()
            )
        }
    };
    ledger.evidence = Some(format!("{}{}", existing, verdict));
}

/// Append network-identity (passport) divergence evidence to the `getNetwork`
/// check when the two endpoints report different passphrases.
fn compare_network_identity(
    checks: &mut [CheckResult],
    other: &[CheckResult],
    other_endpoint: &str,
) {
    // Extract passports via immutable borrows first so we can safely take a
    // mutable borrow afterwards to write evidence (avoids E0502).
    // The field is `passphrase` (see `all_checks`).
    let primary_pp = checks
        .iter()
        .find(|c| c.name == "getNetwork" && c.status == CheckStatus::Pass)
        .and_then(|c| c.details.as_ref())
        .and_then(|d| d.get("passphrase"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let other_pp = other
        .iter()
        .find(|c| c.name == "getNetwork" && c.status == CheckStatus::Pass)
        .and_then(|c| c.details.as_ref())
        .and_then(|d| d.get("passphrase"))
        .and_then(|v| v.as_str())
        .map(String::from);

    if let (Some(p), Some(o)) = (primary_pp, other_pp) {
        if p != o {
            if let Some(c) = checks.iter_mut().find(|c| c.name == "getNetwork") {
                let ev = c.evidence.take().unwrap_or_default();
                c.evidence = Some(format!(
                    "{ev} Network divergence vs {other_endpoint}: passport '{p}' != '{o}'."
                ));
            }
        }
    }
}

/// Append ledger-state divergence evidence to the `getLatestLedger` check when
/// the two endpoints report a different ledger sequence or hash.
fn compare_ledger_state(checks: &mut [CheckResult], other: &[CheckResult], other_endpoint: &str) {
    let primary = checks
        .iter()
        .find(|c| c.name == "getLatestLedger" && c.status == CheckStatus::Pass)
        .and_then(|c| c.details.as_ref());
    let other_ledger = other
        .iter()
        .find(|c| c.name == "getLatestLedger" && c.status == CheckStatus::Pass)
        .and_then(|c| c.details.as_ref());

    if let (Some(p), Some(o)) = (primary, other_ledger) {
        let p_seq = p.get("sequence").and_then(|v| v.as_str()).map(String::from);
        let o_seq = o.get("sequence").and_then(|v| v.as_str()).map(String::from);
        let p_hash = p.get("id").and_then(|v| v.as_str()).map(String::from);
        let o_hash = o.get("id").and_then(|v| v.as_str()).map(String::from);

        if let (Some(p_seq), Some(o_seq)) = (p_seq, o_seq) {
            if p_seq != o_seq || p_hash != o_hash {
                if let Some(c) = checks.iter_mut().find(|c| c.name == "getLatestLedger") {
                    let ev = c.evidence.take().unwrap_or_default();
                    c.evidence = Some(format!(
                        "{ev} Ledger divergence vs {other_endpoint}: sequence {p_seq} (hash {}) != {o_seq} (hash {}).",
                        p_hash.unwrap_or_else(|| "?".to_string()),
                        o_hash.unwrap_or_else(|| "?".to_string())
                    ));
                }
            }
        }
    }
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

    fn make_ledger_result(seq: &str, hash: &str) -> CheckResult {
        let mut details = serde_json::Map::new();
        details.insert(
            "sequence".to_string(),
            serde_json::Value::String(seq.to_string()),
        );
        details.insert(
            "id".to_string(),
            serde_json::Value::String(hash.to_string()),
        );
        CheckResult {
            name: "getLatestLedger".to_string(),
            status: CheckStatus::Pass,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            duration_ms: Some(100),
            http_status: Some(200),
            endpoint: "https://example.com".to_string(),
            details: Some(details),
            error: None,
            error_message: None,
            evidence: None,
        }
    }

    /// Build a report from a list of per-run check vectors, running the
    /// ledger-health analysis exactly like `run_all` does.
    fn ledger_evidence_from(per_run: &[Vec<CheckResult>]) -> String {
        let mut checks = aggregate(per_run);
        analyze_ledger_health(per_run, &mut checks);
        checks
            .iter()
            .find(|c| c.name == "getLatestLedger")
            .and_then(|c| c.evidence.clone())
            .unwrap_or_default()
    }

    #[test]
    fn test_ledger_health_single_run_inconclusive() {
        let per_run = vec![vec![make_ledger_result("100", "abc123def456")]];
        let evidence = ledger_evidence_from(&per_run);
        assert!(
            evidence.contains("100"),
            "should name the sequence: {evidence}"
        );
        assert!(
            evidence.contains("inconclusive"),
            "single run must be inconclusive: {evidence}"
        );
    }

    #[test]
    fn test_ledger_health_advancing() {
        let per_run = vec![
            vec![make_ledger_result("100", "h0")],
            vec![make_ledger_result("101", "h0")],
            vec![make_ledger_result("102", "h0")],
        ];
        let evidence = ledger_evidence_from(&per_run);
        assert!(evidence.contains("advanced"), "evidence: {evidence}");
        assert!(evidence.contains("102"), "evidence: {evidence}");
        assert!(
            !evidence.contains("stale"),
            "advancing must not be stale: {evidence}"
        );
    }

    #[test]
    fn test_ledger_health_stagnant() {
        let per_run = vec![
            vec![make_ledger_result("100", "deadbeef")],
            vec![make_ledger_result("100", "deadbeef")],
            vec![make_ledger_result("100", "deadbeef")],
        ];
        let evidence = ledger_evidence_from(&per_run);
        assert!(evidence.contains("possibly stale"), "evidence: {evidence}");
    }

    #[test]
    fn test_ledger_health_inconsistent_hash() {
        let per_run = vec![
            vec![make_ledger_result("100", "aaa")],
            vec![make_ledger_result("100", "bbb")],
        ];
        let evidence = ledger_evidence_from(&per_run);
        assert!(evidence.contains("inconsistency"), "evidence: {evidence}");
    }

    #[test]
    fn test_ledger_health_no_get_latest_ledger() {
        let per_run = vec![vec![make_result("getHealth", CheckStatus::Pass, Some(100))]];
        let evidence = ledger_evidence_from(&per_run);
        assert!(
            evidence.is_empty(),
            "no getLatestLedger check → no ledger evidence: {evidence}"
        );
    }

    fn make_compare_report(passport: &str, seq: &str, hash: &str) -> DiagnosticReport {
        let mut net_details = serde_json::Map::new();
        net_details.insert(
            "passphrase".to_string(),
            serde_json::Value::String(passport.to_string()),
        );
        let mut ledger_details = serde_json::Map::new();
        ledger_details.insert(
            "sequence".to_string(),
            serde_json::Value::String(seq.to_string()),
        );
        ledger_details.insert(
            "id".to_string(),
            serde_json::Value::String(hash.to_string()),
        );
        DiagnosticReport {
            tool_version: "0.1.0".to_string(),
            endpoint: format!("https://peer-{seq}.example.com"),
            runs: 1,
            summary: ReportSummary {
                ran: 2,
                passed: 2,
                failed: 0,
                skipped: 0,
                total_runs: 1,
            },
            checks: vec![
                CheckResult {
                    name: "getNetwork".to_string(),
                    status: CheckStatus::Pass,
                    timestamp: "2026-01-01T00:00:00Z".to_string(),
                    duration_ms: Some(100),
                    http_status: Some(200),
                    endpoint: "https://primary.example.com".to_string(),
                    details: Some(net_details),
                    error: None,
                    error_message: None,
                    evidence: Some("Sent JSON-RPC 2.0 POST".to_string()),
                },
                CheckResult {
                    name: "getLatestLedger".to_string(),
                    status: CheckStatus::Pass,
                    timestamp: "2026-01-01T00:00:00Z".to_string(),
                    duration_ms: Some(100),
                    http_status: Some(200),
                    endpoint: "https://primary.example.com".to_string(),
                    details: Some(ledger_details),
                    error: None,
                    error_message: None,
                    evidence: Some("Sent JSON-RPC 2.0 POST".to_string()),
                },
            ],
        }
    }

    #[test]
    fn test_compare_network_divergence() {
        let mut primary = make_compare_report("Test SDF Network ; February 2023", "100", "abc");
        let other = make_compare_report(
            "Public Global Stellar Network ; September 2015",
            "100",
            "abc",
        );
        primary.compare_with(&other);
        let evidence = primary
            .checks
            .iter()
            .find(|c| c.name == "getNetwork")
            .and_then(|c| c.evidence.as_deref())
            .unwrap_or("");
        assert!(
            evidence.contains("Network divergence"),
            "expected network divergence: {evidence}"
        );
    }

    #[test]
    fn test_compare_ledger_divergence() {
        let mut primary = make_compare_report("Test SDF Network ; February 2023", "100", "abc");
        let other = make_compare_report("Test SDF Network ; February 2023", "101", "def");
        primary.compare_with(&other);
        let evidence = primary
            .checks
            .iter()
            .find(|c| c.name == "getLatestLedger")
            .and_then(|c| c.evidence.as_deref())
            .unwrap_or("");
        assert!(
            evidence.contains("Ledger divergence"),
            "expected ledger divergence: {evidence}"
        );
    }

    #[test]
    fn test_compare_consistent() {
        let mut primary = make_compare_report("Test SDF Network ; February 2023", "100", "abc");
        let other = make_compare_report("Test SDF Network ; February 2023", "100", "abc");
        primary.compare_with(&other);
        for c in &primary.checks {
            let ev = c.evidence.as_deref().unwrap_or("");
            assert!(!ev.contains("divergence"), "no divergence expected: {ev}");
        }
    }

    // ────────────────────────
    // Arbitrary-method probe helpers
    // ────────────────────────

    #[test]
    fn test_is_builtin_method() {
        for m in [
            "getHealth",
            "getNetwork",
            "getLatestLedger",
            "getVersionInfo",
        ] {
            assert!(is_builtin_method(m), "{m} should be built-in");
        }
        for m in [
            "getLedgerEntries",
            "getTransaction",
            "simulateTransaction",
            "",
        ] {
            assert!(!is_builtin_method(m), "{m} should not be built-in");
        }
    }

    #[test]
    fn test_validate_params_accepts_object_array_null() {
        assert!(validate_params(r#"{"keys":["abc"]}"#).unwrap().is_some());
        assert!(validate_params(r#"["a","b"]"#).unwrap().is_some());
        assert!(validate_params("null").unwrap().is_none());
    }

    #[test]
    fn test_validate_params_rejects_scalar_and_malformed() {
        assert!(validate_params("42").is_err());
        assert!(validate_params("\"str\"").is_err());
        assert!(validate_params("true").is_err());
        assert!(validate_params("not json").is_err());
        assert!(validate_params("{bad}").is_err());
    }

    #[test]
    fn test_insert_result_object_keys_merged() {
        let mut details = serde_json::Map::new();
        insert_result(&mut details, &serde_json::json!({"a": 1, "b": "x"}));
        assert_eq!(details.get("a"), Some(&Value::from(1)));
        assert_eq!(details.get("b"), Some(&Value::from("x")));
        assert!(
            details.get("result").is_none(),
            "object keys are merged; no synthetic 'result' key"
        );
    }

    #[test]
    fn test_insert_result_non_object_under_result_key() {
        let mut details = serde_json::Map::new();
        insert_result(&mut details, &serde_json::json!([1, 2, 3]));
        assert_eq!(details.get("result"), Some(&serde_json::json!([1, 2, 3])));
    }

    #[test]
    fn test_insert_result_truncation_flagged() {
        let mut details = serde_json::Map::new();
        let big = "x".repeat(5000);
        insert_result(&mut details, &Value::String(big));
        assert_eq!(details.get("result_truncated"), Some(&Value::Bool(true)));
        assert!(details.get("result_size_bytes").is_some());
        assert!(
            details.get("result").is_none(),
            "oversized result must not be stored in full"
        );
    }

    #[test]
    fn test_insert_result_boundary_2047_stored() {
        let mut details = serde_json::Map::new();
        // serialized length = 2045 chars + 2 quotes = 2047 (< 2048) -> stored.
        insert_result(&mut details, &Value::String("x".repeat(2045)));
        assert!(details.get("result_truncated").is_none());
        assert!(details.get("result").is_some(), "2047 bytes must be stored");
    }

    #[test]
    fn test_insert_result_boundary_2048_truncated() {
        let mut details = serde_json::Map::new();
        // serialized length = 2046 chars + 2 quotes = 2048 (not < 2048) -> truncated.
        insert_result(&mut details, &Value::String("x".repeat(2046)));
        assert_eq!(details.get("result_truncated"), Some(&Value::Bool(true)));
        assert!(
            details.get("result").is_none(),
            "2048 bytes must be truncated"
        );
    }

    #[test]
    fn test_describe_params_never_echoes_values() {
        assert_eq!(describe_params(&None), "no params");
        assert_eq!(describe_params(&Some(Value::Null)), "no params");
        let secret = Some(serde_json::json!({"api_key": "SUPERSECRET"}));
        let desc = describe_params(&secret);
        assert!(!desc.contains("SUPERSECRET"), "leaked: {desc}");
        assert!(desc.contains("redacted"));
    }

    #[test]
    fn test_arbitrary_method_check_debug_redacts_params() {
        let check = ArbitraryMethodCheck {
            method_name: "getSecret".to_string(),
            params: Some(serde_json::json!({"api_key": "SUPERSECRET123"})),
        };
        let dbg = format!("{check:?}");
        assert!(
            !dbg.contains("SUPERSECRET123"),
            "Debug output leaked params: {dbg}"
        );
        assert!(dbg.contains("getSecret"), "Debug output: {dbg}");
        assert!(dbg.contains("<redacted>"), "Debug output: {dbg}");
    }
}

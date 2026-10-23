//! Integration tests for diagnostic checks using a mock RPC server.
//!
//! All tests use `mockito` to spin up a local HTTP server that returns
//! deterministic JSON-RPC responses. No live Stellar endpoint is required.

use miyo_rpc::{
    sanitize_url, BatchArgs, CheckStatus, DiagnosticReport, OutputFormat, RpcClient,
    RpcClientConfig,
};
use serde_json::{json, Value};
use std::time::Duration;

/// Helper: serialize a JSON value into a body string for mockito's `with_body`.
fn json_body(v: Value) -> String {
    serde_json::to_string(&v).expect("valid JSON")
}

// ────────────────────────────
// Async tests via RpcClient (direct call)
// ────────────────────────────

/// Test successful getHealth response via the RPC client.
#[tokio::test]
async fn test_get_health_success() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    let mock = server
        .mock("POST", "/")
        .match_header("content-type", "application/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "healthy",
                "latestLedger": 51583040,
                "oldestLedger": 51565760,
                "ledgerRetentionWindow": 17281
            }
        })))
        .create_async()
        .await;

    let config = RpcClientConfig::new(Duration::from_secs(5));
    let client = RpcClient::new(&url, config).expect("should create client");

    let result = client.call::<Value>("getHealth", None).await;
    assert!(result.is_ok(), "call should succeed");

    let (info, result) = result.unwrap();
    assert_eq!(info.status, 200);
    assert_eq!(
        result.get("status").and_then(|v| v.as_str()),
        Some("healthy")
    );

    mock.assert_async().await;
}

/// Test HTTP 500 error.
#[tokio::test]
async fn test_http_500_error() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("POST", "/")
        .with_status(500)
        .with_header("content-type", "text/plain")
        .with_body("Internal Server Error")
        .create_async()
        .await;

    let config = RpcClientConfig::new(Duration::from_secs(5));
    let client = RpcClient::new(&url, config).expect("should create client");

    let result = client.call::<Value>("getHealth", None).await;
    assert!(result.is_err(), "call should fail");
    assert!(result.err().unwrap().to_string().contains("500"));
}

/// Test HTTP 404 error.
#[tokio::test]
async fn test_http_404_error() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("POST", "/")
        .with_status(404)
        .with_header("content-type", "text/plain")
        .with_body("Not Found")
        .create_async()
        .await;

    let config = RpcClientConfig::new(Duration::from_secs(5));
    let client = RpcClient::new(&url, config).expect("should create client");

    let result = client.call::<Value>("getHealth", None).await;
    assert!(result.is_err());
    assert_eq!(result.err().unwrap().to_string(), "HTTP 404: Not Found");
}

/// Test JSON-RPC error response (-32601 method not found).
#[tokio::test]
async fn test_json_rpc_error_method_not_found() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32601,
                "message": "Method not found"
            }
        })))
        .create_async()
        .await;

    let config = RpcClientConfig::new(Duration::from_secs(5));
    let client = RpcClient::new(&url, config).expect("should create client");

    let result = client.call::<Value>("nonexistentMethod", None).await;
    assert!(result.is_err());

    let err = result.err().unwrap();
    let err_str = err.to_string();
    assert!(
        err_str.contains("-32601"),
        "should have code -32601: {err_str}"
    );
    assert!(
        err_str.contains("Method not found"),
        "should have message: {err_str}"
    );
}

/// Test invalid JSON response.
#[tokio::test]
async fn test_invalid_json() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "text/plain")
        .with_body("This is not JSON at all")
        .create_async()
        .await;

    let config = RpcClientConfig::new(Duration::from_secs(5));
    let client = RpcClient::new(&url, config).expect("should create client");

    let result = client.call::<Value>("getHealth", None).await;
    assert!(result.is_err());
    let err_str = result.err().unwrap().to_string().to_lowercase();
    assert!(
        err_str.contains("invalid json") || err_str.contains("malformed"),
        "should be invalid JSON error: {err_str}"
    );
}

/// Test missing 'result' field in response.
#[tokio::test]
async fn test_missing_result_field() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1
        })))
        .create_async()
        .await;

    let config = RpcClientConfig::new(Duration::from_secs(5));
    let client = RpcClient::new(&url, config).expect("should create client");

    let result = client.call::<Value>("getHealth", None).await;
    assert!(result.is_err());
    assert!(result
        .err()
        .unwrap()
        .to_string()
        .contains("missing 'result'"));
}

/// Test incorrect type for a response field.
#[tokio::test]
async fn test_type_mismatch_in_result() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    // Return latestLedger as a string instead of a number.
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "latestLedger": "not-a-number",
                "oldestLedger": 123,
                "ledgerRetentionWindow": 100,
                "status": "healthy"
            }
        })))
        .create_async()
        .await;

    let config = RpcClientConfig::new(Duration::from_secs(5));
    let client = RpcClient::new(&url, config).expect("should create client");

    // Value accepts anything — so the call succeeds.
    let result = client.call::<Value>("getHealth", None).await;
    assert!(result.is_ok());

    // Now try a typed deserialization — latestLedger as u64 should fail.
    let (_, resp) = client.call_raw("getHealth", None).await.unwrap();

    #[allow(dead_code)]
    #[derive(serde::Deserialize)]
    struct HealthResult {
        latest_ledger: u64,
    }

    let typed: Result<HealthResult, _> =
        serde_json::from_value(resp.get("result").cloned().unwrap_or(json!(null)));
    assert!(typed.is_err(), "should fail to parse string as u64");
}

/// Test connection failure (server not running).
#[tokio::test]
async fn test_connection_failure() {
    let config = RpcClientConfig::new(Duration::from_millis(100));
    let client = RpcClient::new("http://127.0.0.1:1", config).expect("should create client");

    let result = client.call::<Value>("getHealth", None).await;
    assert!(result.is_err());

    let lower = result.err().unwrap().to_string().to_lowercase();
    assert!(
        lower.contains("network")
            || lower.contains("connection")
            || lower.contains("connect")
            || lower.contains("refused"),
        "should be a connection error: {lower}"
    );
}

// ────────────────────────────
// URL sanitization unit tests
// ────────────────────────────

#[tokio::test]
async fn test_sanitize_url_redacts_secrets() {
    let url = "https://example.com/rpc?key=secret123&token=abc456&param=value";
    let sanitized = sanitize_url(url);
    assert!(
        !sanitized.contains("secret123"),
        "key should be redacted: {sanitized}"
    );
    assert!(
        !sanitized.contains("abc456"),
        "token should be redacted: {sanitized}"
    );
    assert!(
        sanitized.contains("param=value"),
        "non-sensitive param should remain: {sanitized}"
    );
    assert!(sanitized.contains("REDACTED"));
}

#[tokio::test]
async fn test_sanitize_url_no_secrets() {
    let url = "https://soroban-testnet.stellar.org/?param=value";
    let sanitized = sanitize_url(url);
    assert_eq!(sanitized, url);
}

#[tokio::test]
async fn test_sanitize_url_malformed() {
    let url = "not a url at all";
    let sanitized = sanitize_url(url);
    assert_eq!(sanitized, url);
}

#[tokio::test]
async fn test_sanitize_url_only_secret() {
    let url = "https://example.com/rpc?Authorization=BearerMySecret";
    let sanitized = sanitize_url(url);
    assert!(
        !sanitized.contains("BearerMySecret"),
        "auth should be redacted"
    );
    assert!(sanitized.contains("REDACTED"));
}

// ────────────────────────────
// Sync integration tests (DiagnosticReport::run_all — manages its own runtime)
// ────────────────────────────

/// Test DiagnosticReport with successful responses for all methods.
#[tokio::test]
async fn test_report_all_success() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    // Mock GET / for HTTP connectivity check.
    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    // Mock getHealth
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getHealth""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "healthy",
                "latestLedger": 51583040,
                "oldestLedger": 51565760,
                "ledgerRetentionWindow": 17281
            }
        })))
        .create();

    // Mock getNetwork
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getNetwork""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "passphrase": "Test SDF Network ; September 2015",
                "protocolVersion": 20,
                "friendbotUrl": "https://friendbot-testnet.stellar.org/"
            }
        })))
        .create();

    // Mock getLatestLedger
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getLatestLedger""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "id": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984",
                "sequence": 2002985,
                "closeTime": "1765223032",
                "headerXdr": "AAAAGBVaCGp9GqWZ",
                "metadataXdr": "AAAAAgAAAACfMw"
            }
        })))
        .create();

    // Mock getVersionInfo
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getVersionInfo""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "version": "23.0.1",
                "commitHash": "fcd2f0523f04279bae4502f3e3fa00ca627e6f6a",
                "buildTimestamp": "2025-05-10T11:18:38",
                "captiveCoreVersion": "stellar-core 23.0.1",
                "protocolVersion": 23
            }
        })))
        .create();

    let report = DiagnosticReport::run_all(&url, Duration::from_secs(5), None, 1).await;

    // Should have httpConnectivity + 4 method checks = 5 total
    assert_eq!(report.checks.len(), 5);

    // HTTP connectivity should pass (200)
    let http_check = report.checks.iter().find(|c| c.name == "httpConnectivity");
    assert!(http_check.is_some());
    assert_eq!(http_check.unwrap().status, CheckStatus::Pass);

    // All 4 RPC method checks should pass
    for name in [
        "getHealth",
        "getNetwork",
        "getLatestLedger",
        "getVersionInfo",
    ] {
        let check = report.checks.iter().find(|c| c.name == name);
        assert!(check.is_some(), "should have check for {name}");
        assert_eq!(
            check.unwrap().status,
            CheckStatus::Pass,
            "check for {name} should pass"
        );
    }

    assert_eq!(report.summary.passed, 5);
    assert_eq!(report.summary.failed, 0);
}

/// Test report with JSON-RPC errors on all methods.
#[tokio::test]
async fn test_report_all_json_rpc_errors() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"(getHealth|getNetwork|getLatestLedger|getVersionInfo)""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32601,
                "message": "Method not found"
            }
        })))
        .create();

    let report = DiagnosticReport::run_all(&url, Duration::from_secs(5), None, 1).await;

    // 4 RPC method checks should all be Fail
    let rpc_checks: Vec<_> = report
        .checks
        .iter()
        .filter(|c| c.name != "httpConnectivity")
        .collect();
    assert_eq!(rpc_checks.len(), 4);
    for c in &rpc_checks {
        assert_eq!(c.status, CheckStatus::Fail, "{} should fail", c.name);
    }

    assert_eq!(report.summary.failed, 5);
}

/// Test method filter (--method).
#[tokio::test]
async fn test_report_method_filter() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getHealth""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "healthy",
                "latestLedger": 100,
                "oldestLedger": 50,
                "ledgerRetentionWindow": 50
            }
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getHealth"), 1).await;

    // getHealth should pass, others should be skipped
    let health_check = report.checks.iter().find(|c| c.name == "getHealth");
    assert!(health_check.is_some());
    assert_eq!(health_check.unwrap().status, CheckStatus::Pass);

    let network_check = report.checks.iter().find(|c| c.name == "getNetwork");
    assert!(network_check.is_some());
    assert_eq!(network_check.unwrap().status, CheckStatus::Skip);
}

/// Test JSON output format.
#[tokio::test]
async fn test_json_output_format() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getHealth""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "healthy",
                "latestLedger": 100,
                "oldestLedger": 50,
                "ledgerRetentionWindow": 50
            }
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getHealth"), 1).await;
    let json_output = miyo_rpc::output::format_json(&report).expect("JSON serialization");
    let parsed: serde_json::Value = serde_json::from_str(&json_output).expect("valid JSON");

    assert!(parsed.get("tool_version").is_some());
    assert!(parsed.get("endpoint").is_some());
    assert!(parsed.get("checks").is_some());
    assert!(parsed.get("summary").is_some());

    let endpoint = parsed.get("endpoint").and_then(|v| v.as_str()).unwrap();
    assert!(
        !endpoint.contains("secret") && !endpoint.contains("key="),
        "endpoint should be sanitized: {endpoint}"
    );
}

/// Test human-readable output format.
#[tokio::test]
async fn test_human_output_format() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getHealth""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "healthy",
                "latestLedger": 100,
                "oldestLedger": 50,
                "ledgerRetentionWindow": 50
            }
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getHealth"), 1).await;
    let human_output = miyo_rpc::output::format_human(&report);

    assert!(
        human_output.contains("Miyo RPC Labs"),
        "should contain tool name"
    );
    assert!(
        human_output.contains("Endpoint:"),
        "should contain endpoint label"
    );
    assert!(
        human_output.contains("getHealth"),
        "should mention getHealth"
    );
    assert!(human_output.contains("PASS"), "should show PASS status");
    assert!(human_output.contains("Summary:"), "should have summary");
}

/// Test report with a connection failure (unreachable port).
#[tokio::test]
async fn test_report_connection_failure() {
    let report =
        DiagnosticReport::run_all("http://127.0.0.1:1", Duration::from_millis(200), None, 1).await;

    let http_check = report.checks.iter().find(|c| c.name == "httpConnectivity");
    assert!(http_check.is_some());
    assert_eq!(http_check.unwrap().status, CheckStatus::Fail);

    let rpc_checks: Vec<_> = report
        .checks
        .iter()
        .filter(|c| c.name != "httpConnectivity")
        .collect();
    assert!(!rpc_checks.is_empty());
    for c in &rpc_checks {
        assert_eq!(
            c.status,
            CheckStatus::Fail,
            "{} should fail on connection error",
            c.name
        );
    }
}

/// Test that an invalid URL scheme is rejected at the client level.
#[tokio::test]
async fn test_invalid_url_scheme() {
    let config = RpcClientConfig::new(Duration::from_secs(5));
    let result = RpcClient::new("ftp://example.com/rpc", config);
    assert!(result.is_err());
    assert!(result.err().unwrap().to_string().contains("scheme"));
}

/// Test output redaction: output never contains raw credentials even
/// when the input URL has sensitive query params.
#[tokio::test]
async fn test_output_never_leaks_secrets() {
    let url_with_secret = "https://example.com/rpc?key=supersecretvalue&token=topsecret";
    let report = DiagnosticReport::run_all(url_with_secret, Duration::from_secs(5), None, 1).await;

    let human = miyo_rpc::output::format_human(&report);
    assert!(
        !human.contains("supersecretvalue"),
        "human output should not contain raw secret"
    );
    assert!(
        !human.contains("topsecret"),
        "human output should not contain raw token"
    );

    let json_output = miyo_rpc::output::format_json(&report).expect("JSON serialization");
    assert!(
        !json_output.contains("supersecretvalue"),
        "JSON output should not contain raw secret"
    );
    assert!(
        !json_output.contains("topsecret"),
        "JSON output should not contain raw token"
    );
    assert!(json_output.contains("REDACTED"));
}

#[tokio::test]
async fn test_sanitize_url_redacts_userinfo() {
    // Userinfo (username:password) must be redacted to prevent credential leakage.
    let url = "https://user:secret123@example.com/rpc?api_key=mykey&param=ok";
    let sanitized = sanitize_url(url);

    assert!(
        !sanitized.contains("secret123"),
        "password should be redacted: {sanitized}"
    );
    assert!(
        !sanitized.contains("mykey"),
        "api_key should be redacted: {sanitized}"
    );
    assert!(
        sanitized.contains("param=ok"),
        "non-sensitive param should remain: {sanitized}"
    );
    assert!(sanitized.contains("example.com"));
    assert!(sanitized.contains("/rpc"));
    assert!(sanitized.contains("REDACTED"));
}

#[tokio::test]
async fn test_network_error_redacts_url() {
    // Network errors must not leak the raw URL (which may carry credentials).
    let config = miyo_rpc::RpcClientConfig::new(Duration::from_millis(100));
    let client = miyo_rpc::RpcClient::new("http://user:secret123@127.0.0.1:1", config)
        .expect("should create client");

    let result = client.call::<Value>("getHealth", None).await;
    assert!(result.is_err());
    let err = result.err().unwrap();
    let err_str = err.to_string();
    assert!(
        !err_str.contains("secret123"),
        "network error should not contain credentials: {err_str}"
    );
    assert!(
        err_str.contains("REDACTED"),
        "network error should reference sanitized endpoint: {err_str}"
    );
}

// ────────────────────────────────
// Bounded response size tests
// ────────────────────────────────

#[tokio::test]
async fn test_oversized_response_rejected() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    // Build a JSON body larger than the configured limit.
    let payload = "A".repeat(200);
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"status":"healthy","big":"{}"}}}}"#,
        payload
    );

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getHealth""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create();

    let mut config = miyo_rpc::RpcClientConfig::new(Duration::from_secs(5));
    config.max_response_bytes = 50; // 50-byte limit to force rejection
    let client = miyo_rpc::RpcClient::new(&url, config).expect("should create client");

    let result = client.call::<Value>("getHealth", None).await;
    assert!(
        result.is_err(),
        "should reject response exceeding size limit"
    );
    let err_str = result.err().unwrap().to_string();
    assert!(
        err_str.contains("exceeds maximum size"),
        "error should mention size limit: {err_str}"
    );
    // The oversized body content must not appear in the error message.
    assert!(
        !err_str.contains("AAAA"),
        "error must not leak response body content: {err_str}"
    );
}

#[tokio::test]
async fn test_oversized_http_error_body_rejected() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    // HTTP 500 with a body larger than the configured limit.
    let big_body = format!(
        r#"{{"error":"internal server error details {}"}}"#,
        "Z".repeat(200)
    );

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getHealth""#.to_string(),
        ))
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(big_body)
        .create();

    let mut config = miyo_rpc::RpcClientConfig::new(Duration::from_secs(5));
    config.max_response_bytes = 50;
    let client = miyo_rpc::RpcClient::new(&url, config).expect("should create client");

    let result = client.call::<Value>("getHealth", None).await;
    assert!(
        result.is_err(),
        "should reject oversized HTTP error response"
    );
    let err_str = result.err().unwrap().to_string();
    assert!(
        err_str.contains("exceeds maximum size"),
        "error should mention size limit: {err_str}"
    );
    // Error body content must not leak.
    assert!(
        !err_str.contains("ZZZ"),
        "error must not leak response body: {err_str}"
    );
}

// ────────────────────────────────
// Multi-run latency aggregation tests
// ────────────────────────────────

#[tokio::test]
async fn test_multi_run_aggregates_latency() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getHealth""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"jsonrpc":"2.0","id":1,"result":{"status":"healthy","latest_ledger":"00000001"}}"#,
        )
        .create();

    let report =
        DiagnosticReport::run_all(&server.url(), Duration::from_secs(5), Some("getHealth"), 3)
            .await;

    assert_eq!(report.runs, 3);
    assert_eq!(report.summary.total_runs, 3);

    let health = report
        .checks
        .iter()
        .find(|c| c.name == "getHealth")
        .expect("getHealth check present");
    assert_eq!(health.status, miyo_rpc::CheckStatus::Pass);
    assert!(health.duration_ms.is_some(), "duration should be averaged");

    let details = health
        .details
        .as_ref()
        .expect("latency stats should be in details");
    assert!(details.contains_key("latency_min_ms"));
    assert!(details.contains_key("latency_max_ms"));
    assert!(details.contains_key("latency_avg_ms"));
    assert_eq!(details.get("run_count"), Some(&serde_json::json!(3)));
}

#[tokio::test]
async fn test_single_run_skips_latency_stats() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getHealth""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"jsonrpc":"2.0","id":1,"result":{"status":"healthy","latest_ledger":"00000001"}}"#,
        )
        .create();

    let report =
        DiagnosticReport::run_all(&server.url(), Duration::from_secs(5), Some("getHealth"), 1)
            .await;

    assert_eq!(report.runs, 1);
    let health = report
        .checks
        .iter()
        .find(|c| c.name == "getHealth")
        .expect("getHealth check present");
    assert!(
        health.details.is_none()
            || !health
                .details
                .as_ref()
                .unwrap()
                .contains_key("latency_min_ms"),
        "single run should not include latency stats"
    );
}

/// When `--runs > 1` targets an unreachable host, the loop still executes
/// `runs` times and reports the completed count honestly.
#[tokio::test]
async fn test_multiple_runs_on_connection_failure() {
    let report =
        DiagnosticReport::run_all("http://127.0.0.1:1", Duration::from_millis(100), None, 5).await;

    assert_eq!(report.runs, 5);
    assert_eq!(report.summary.total_runs, 5);
    // No check should be Pass against an unreachable endpoint.
    for c in &report.checks {
        assert_ne!(c.status, miyo_rpc::CheckStatus::Pass);
    }
    // Endpoint must be sanitized.
    assert!(!report.endpoint.contains("secret"));
}

/// When reqwest::Client::build() fails (e.g. unsupported `ftp://` scheme),
/// `RpcClient::new` returns an error and `run_all` takes the early-return path.
/// No retry loop executes, so `runs` must report 1 (the single connectivity
/// attempt), not the user-requested count.
#[tokio::test]
async fn test_client_init_failure_reports_one_run() {
    let report =
        DiagnosticReport::run_all("ftp://example.com/rpc", Duration::from_millis(100), None, 3)
            .await;

    assert_eq!(report.runs, 1);
    assert_eq!(report.summary.total_runs, 1);
    // All checks fail when the client cannot be initialised.
    for c in &report.checks {
        assert_eq!(c.status, miyo_rpc::CheckStatus::Fail);
    }
    assert!(!report.endpoint.contains("secret"));
}

/// Helper: build a minimal report whose single check is `getNetwork`.
fn build_network_report(
    passphrase: Option<&str>,
    status: miyo_rpc::CheckStatus,
) -> miyo_rpc::DiagnosticReport {
    let mut details = serde_json::Map::new();
    details.insert(
        "description".to_string(),
        serde_json::Value::String("Network passphrase and protocol version.".to_string()),
    );
    if let Some(p) = passphrase {
        details.insert(
            "passphrase".to_string(),
            serde_json::Value::String(p.to_string()),
        );
    }
    let error = if status == miyo_rpc::CheckStatus::Fail {
        Some(miyo_rpc::IssueCategory::HttpError)
    } else {
        None
    };
    miyo_rpc::DiagnosticReport {
        tool_version: "0.1.0".to_string(),
        endpoint: "https://example.com".to_string(),
        runs: 1,
        summary: miyo_rpc::ReportSummary {
            ran: 1,
            passed: 1,
            failed: 0,
            skipped: 0,
            total_runs: 1,
        },
        checks: vec![miyo_rpc::CheckResult {
            name: "getNetwork".to_string(),
            status,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            duration_ms: Some(100),
            http_status: Some(200),
            endpoint: "https://example.com".to_string(),
            details: Some(details),
            error,
            error_message: None,
            evidence: Some("Sent JSON-RPC 2.0 POST...".to_string()),
        }],
    }
}

#[test]
fn test_enforce_expected_network_match() {
    let mut report = build_network_report(
        Some("Test SDF Network ; February 2023"),
        miyo_rpc::CheckStatus::Pass,
    );
    report.enforce_expected_network(Some("testnet"));
    assert_eq!(report.checks[0].status, miyo_rpc::CheckStatus::Pass);
    assert_eq!(report.summary.passed, 1);
    assert_eq!(report.summary.failed, 0);
    assert_eq!(report.summary.ran, 1);
}

#[test]
fn test_enforce_expected_network_mismatch() {
    let mut report = build_network_report(
        Some("Evil Network That Is Not Testnet"),
        miyo_rpc::CheckStatus::Pass,
    );
    report.enforce_expected_network(Some("testnet"));
    assert_eq!(report.checks[0].status, miyo_rpc::CheckStatus::Fail);
    assert_eq!(report.checks[0].error, Some(miyo_rpc::IssueCategory::Other));
    assert!(
        report.checks[0]
            .error_message
            .as_ref()
            .unwrap()
            .contains("passphrase mismatch"),
        "got: {:?}",
        report.checks[0].error_message
    );
    // Summary recomputed after downgrade.
    assert_eq!(report.summary.passed, 0);
    assert_eq!(report.summary.failed, 1);
    assert_eq!(report.summary.ran, 1);
}

#[test]
fn test_enforce_expected_network_no_getnetwork_check() {
    let mut report = miyo_rpc::DiagnosticReport {
        tool_version: "0.1.0".to_string(),
        endpoint: "https://example.com".to_string(),
        runs: 1,
        summary: miyo_rpc::ReportSummary {
            ran: 1,
            passed: 1,
            failed: 0,
            skipped: 0,
            total_runs: 1,
        },
        checks: vec![miyo_rpc::CheckResult {
            name: "getHealth".to_string(),
            status: miyo_rpc::CheckStatus::Pass,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            duration_ms: Some(50),
            http_status: Some(200),
            endpoint: "https://example.com".to_string(),
            details: None,
            error: None,
            error_message: None,
            evidence: Some("Sent JSON-RPC 2.0 POST...".to_string()),
        }],
    };
    // No getNetwork check present — enforcement is a no-op, summary unchanged.
    report.enforce_expected_network(Some("testnet"));
    assert_eq!(report.checks[0].name, "getHealth");
    assert_eq!(report.checks[0].status, miyo_rpc::CheckStatus::Pass);
    assert_eq!(report.summary.failed, 0);
}

#[test]
fn test_enforce_expected_network_already_failed() {
    // getNetwork already Fail (HttpError) — must not be overwritten to Other.
    let mut report = build_network_report(None, miyo_rpc::CheckStatus::Fail);
    report.enforce_expected_network(Some("testnet"));
    assert_eq!(report.checks[0].status, miyo_rpc::CheckStatus::Fail);
    assert_eq!(
        report.checks[0].error,
        Some(miyo_rpc::IssueCategory::HttpError)
    );
    assert_eq!(report.summary.failed, 1);
}

/// Build a deterministic report with a populated `getNetwork` and
/// `getLatestLedger` check (both Pass) for schema/convergence assertions.
fn build_report(passport: &str, seq: &str, hash: &str) -> miyo_rpc::DiagnosticReport {
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
    miyo_rpc::DiagnosticReport {
        tool_version: "0.1.0".to_string(),
        endpoint: "https://example.com".to_string(),
        runs: 1,
        summary: miyo_rpc::ReportSummary {
            ran: 2,
            passed: 2,
            failed: 0,
            skipped: 0,
            total_runs: 1,
        },
        checks: vec![
            miyo_rpc::CheckResult {
                name: "getNetwork".to_string(),
                status: miyo_rpc::CheckStatus::Pass,
                timestamp: "2026-01-01T00:00:00Z".to_string(),
                duration_ms: Some(50),
                http_status: Some(200),
                endpoint: "https://example.com".to_string(),
                details: Some(net_details),
                error: None,
                error_message: None,
                evidence: Some("Sent JSON-RPC 2.0 POST to getNetwork.".to_string()),
            },
            miyo_rpc::CheckResult {
                name: "getLatestLedger".to_string(),
                status: miyo_rpc::CheckStatus::Pass,
                timestamp: "2026-01-01T00:00:00Z".to_string(),
                duration_ms: Some(50),
                http_status: Some(200),
                endpoint: "https://example.com".to_string(),
                details: Some(ledger_details),
                error: None,
                error_message: None,
                evidence: Some("Sent JSON-RPC 2.0 POST to getLatestLedger.".to_string()),
            },
        ],
    }
}

#[test]
fn test_json_schema_version_and_field_types() {
    let mut details = serde_json::Map::new();
    details.insert(
        "sequence".to_string(),
        serde_json::Value::String("100".to_string()),
    );
    details.insert(
        "id".to_string(),
        serde_json::Value::String("abc123def456".to_string()),
    );
    let report = miyo_rpc::DiagnosticReport {
        tool_version: "0.1.0".to_string(),
        endpoint: "https://example.com".to_string(),
        runs: 3,
        summary: miyo_rpc::ReportSummary {
            ran: 3,
            passed: 1,
            failed: 0,
            skipped: 0,
            total_runs: 3,
        },
        checks: vec![miyo_rpc::CheckResult {
            name: "getLatestLedger".to_string(),
            status: miyo_rpc::CheckStatus::Pass,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            duration_ms: Some(100),
            http_status: Some(200),
            endpoint: "https://example.com".to_string(),
            details: Some(details),
            error: None,
            error_message: None,
            evidence: Some(
                "Sent JSON-RPC 2.0 POST. Ledger sequence 100, hash abc: \
                 unchanged across 3 runs — possibly stale endpoint."
                    .to_string(),
            ),
        }],
    };

    let json_output = miyo_rpc::output::format_json(&report).expect("JSON serialization");
    let parsed: serde_json::Value = serde_json::from_str(&json_output).expect("valid JSON");

    // schema_version is present with the expected value. Field order is not
    // guaranteed without serde_json's `preserve_order` feature, so we assert
    // presence + value only.
    assert_eq!(
        parsed.get("schema_version").and_then(|v| v.as_str()),
        Some("1.0")
    );

    // Stable field types.
    assert_eq!(parsed.get("runs").and_then(|v| v.as_u64()), Some(3));
    let checks = parsed
        .get("checks")
        .and_then(|v| v.as_array())
        .expect("checks is an array");
    assert_eq!(checks.len(), 1);
    let check = &checks[0];
    assert_eq!(
        check.get("name").and_then(|v| v.as_str()),
        Some("getLatestLedger")
    );
    assert_eq!(check.get("status").and_then(|v| v.as_str()), Some("pass"));
    assert_eq!(check.get("duration_ms").and_then(|v| v.as_u64()), Some(100));
    assert_eq!(
        check.get("http_status").and_then(|v| v.as_u64()),
        Some(200u64)
    );
    assert_eq!(
        check.get("evidence").and_then(|v| v.as_str()),
        Some(
            "Sent JSON-RPC 2.0 POST. Ledger sequence 100, hash abc: \
unchanged across 3 runs — possibly stale endpoint."
        )
    );
    assert_eq!(
        parsed
            .get("summary")
            .and_then(|v| v.get("passed"))
            .and_then(|v| v.as_u64()),
        Some(1)
    );
}

#[test]
fn test_json_includes_ledger_evidence() {
    let report = build_report("Test SDF Network ; February 2023", "100", "abc");
    let json_output = miyo_rpc::output::format_json(&report).expect("JSON serialization");
    assert!(
        json_output.contains("\"schema_version\""),
        "JSON output must contain schema_version: {json_output}"
    );
    // The full evidence string survives into the machine-readable report.
    assert!(json_output.contains("Sent JSON-RPC 2.0 POST to getLatestLedger."));
}

#[test]
fn test_json_includes_compare_divergence() {
    // Primary = testnet, Comparison = mainnet (passport mismatch).
    let mut report = build_report("Test SDF Network ; February 2023", "100", "abc");
    let other = build_report(
        "Public Global Stellar Network ; September 2015",
        "100",
        "abc",
    );
    report.compare_with(&other);

    let json_output = miyo_rpc::output::format_json(&report).expect("JSON serialization");
    let parsed: serde_json::Value = serde_json::from_str(&json_output).expect("valid JSON");
    assert_eq!(
        parsed.get("schema_version").and_then(|v| v.as_str()),
        Some("1.0")
    );
    // Divergence is surfaced on the getNetwork check's evidence field.
    let net_evidence = parsed
        .get("checks")
        .and_then(|v| v.as_array())
        .expect("checks is an array")
        .iter()
        .find(|c| c.get("name").and_then(|v| v.as_str()) == Some("getNetwork"))
        .and_then(|c| c.get("evidence").and_then(|v| v.as_str()))
        .unwrap_or("");
    assert!(
        net_evidence.contains("Network divergence"),
        "getNetwork evidence should record divergence: {net_evidence}"
    );
}

#[tokio::test]
async fn test_batch_all_valid_pass() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("GET", "/")
        .with_status(200)
        .create_async()
        .await;
    server
        .mock("POST", "/")
        .match_header("content-type", "application/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "healthy",
                "latestLedger": 51583040,
                "oldestLedger": 51565760,
                "ledgerRetentionWindow": 17281
            }
        })))
        .create_async()
        .await;

    let mut path = std::env::temp_dir();
    path.push("miyo_batch_ok.txt");
    std::fs::write(&path, format!("# comment\n{}\n", url)).unwrap();

    let args = BatchArgs {
        file: path.to_string_lossy().to_string(),
        output: OutputFormat::Human,
        timeout_secs: 5,
        runs: 1,
        method: Some("getHealth".to_string()),
        concurrency: 1,
        expected_network: None,
    };
    let report = miyo_rpc::run_batch(&args).await;
    assert_eq!(report.summary.total, 1);
    assert_eq!(report.summary.valid, 1);
    assert_eq!(report.summary.invalid, 0);
    assert_eq!(report.summary.healthy, 1);
    assert_eq!(report.summary.exit_code, 0);
    assert!(report.endpoints[0].valid);
    assert!(report.endpoints[0].report.is_some());
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn test_batch_unhealthy_exit_1() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("GET", "/")
        .with_status(200)
        .create_async()
        .await;
    server
        .mock("POST", "/")
        .with_status(500)
        .with_header("content-type", "text/plain")
        .with_body("Internal Server Error")
        .create_async()
        .await;

    let mut path = std::env::temp_dir();
    path.push("miyo_batch_bad.txt");
    std::fs::write(&path, &url).unwrap();
    let args = BatchArgs {
        file: path.to_string_lossy().to_string(),
        output: OutputFormat::Human,
        timeout_secs: 5,
        runs: 1,
        method: Some("getHealth".to_string()),
        concurrency: 1,
        expected_network: None,
    };
    let report = miyo_rpc::run_batch(&args).await;
    assert_eq!(report.summary.total, 1);
    assert_eq!(report.summary.valid, 1);
    assert_eq!(report.summary.invalid, 0);
    assert_eq!(report.summary.failed, 1);
    assert_eq!(report.summary.exit_code, 1);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn test_batch_all_invalid_exit_2() {
    let mut path = std::env::temp_dir();
    path.push("miyo_batch_invalid.txt");
    std::fs::write(&path, "ftp://bad.example\nnot-a-url\n").unwrap();
    let args = BatchArgs {
        file: path.to_string_lossy().to_string(),
        output: OutputFormat::Human,
        timeout_secs: 5,
        runs: 1,
        method: Some("getHealth".to_string()),
        concurrency: 1,
        expected_network: None,
    };
    let report = miyo_rpc::run_batch(&args).await;
    assert_eq!(report.summary.total, 2);
    assert_eq!(report.summary.valid, 0);
    assert_eq!(report.summary.invalid, 2);
    assert_eq!(report.summary.exit_code, 2);
    let _ = std::fs::remove_file(&path);
}

// ────────────────────────────────
// Batch credential-handling tests
// ────────────────────────────────

/// A valid URL with a sensitive query parameter must be used verbatim for
/// network requests (so the API key reaches the server) while every output
/// surface must redact it.
#[tokio::test]
async fn test_batch_query_credentials_preserved_in_request_sanitized_in_output() {
    let mut server = mockito::Server::new_async().await;
    let mock_url = format!("{}?api_key=secret123", server.url());

    // GET mock for httpConnectivity — matches any query string.
    server
        .mock("GET", "/")
        .with_status(200)
        .create_async()
        .await;

    // POST mock for getHealth — only matches if the ORIGINAL api_key is
    // present. If the sanitized URL (api_key=REDACTED) were used for the
    // request, this mock would NOT match and getHealth would fail.
    server
        .mock("POST", "/")
        .match_query(mockito::Matcher::Regex("api_key=secret123".to_string()))
        .match_body(mockito::Matcher::Regex(
            r#""method":"getHealth""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "healthy",
                "latestLedger": 51583040,
                "oldestLedger": 51565760,
                "ledgerRetentionWindow": 17281
            }
        })))
        .create_async()
        .await;

    let mut path = std::env::temp_dir();
    path.push("miyo_batch_qcreds.txt");
    std::fs::write(&path, &mock_url).unwrap();

    let args = BatchArgs {
        file: path.to_string_lossy().to_string(),
        output: OutputFormat::Json,
        timeout_secs: 5,
        runs: 1,
        method: Some("getHealth".to_string()),
        concurrency: 1,
        expected_network: None,
    };
    let report = miyo_rpc::run_batch(&args).await;

    // getHealth must PASS — proving the original URL (with the real api_key)
    // was used for the HTTP request, not the sanitized one.
    let entry = &report.endpoints[0];
    assert!(entry.valid, "endpoint should be valid");
    let checks = entry
        .report
        .as_ref()
        .and_then(|r| r.get("checks"))
        .and_then(|c| c.as_array())
        .expect("checks array");
    let health = checks
        .iter()
        .find(|c| c.get("name").and_then(|v| v.as_str()) == Some("getHealth"))
        .expect("getHealth check present");
    assert_eq!(
        health.get("status").and_then(|v| v.as_str()),
        Some("pass"),
        "getHealth should pass — original URL with credentials must be used for requests"
    );

    // Output surfaces must be sanitized.
    let json = miyo_rpc::format_batch_json(&report).unwrap();
    assert!(
        !json.contains("secret123"),
        "JSON output must not contain raw secret: {}",
        json
    );
    assert!(json.contains("REDACTED"), "JSON should contain REDACTED");
    assert!(
        !entry.endpoint.contains("secret123"),
        "BatchEntry.endpoint must not contain raw secret"
    );
    assert!(entry.endpoint.contains("REDACTED"));

    let _ = std::fs::remove_file(&path);
}

/// Invalid URLs containing apparent credentials must not leak secrets in
/// error messages, JSON reports, or human-readable output.
#[tokio::test]
async fn test_batch_invalid_url_credentials_no_leak_json() {
    // ftp:// with userinfo + sensitive query param → wrong scheme, but URL parses.
    let input = "ftp://user:secret@host.example/rpc?api_key=leaked";

    let mut path = std::env::temp_dir();
    path.push("miyo_batch_bad_creds.txt");
    std::fs::write(&path, input).unwrap();

    let args = BatchArgs {
        file: path.to_string_lossy().to_string(),
        output: OutputFormat::Json,
        timeout_secs: 5,
        runs: 1,
        method: None,
        concurrency: 1,
        expected_network: None,
    };
    let report = miyo_rpc::run_batch(&args).await;
    let json = miyo_rpc::format_batch_json(&report).unwrap();

    assert!(!report.endpoints[0].valid);

    // Neither the password nor the api_key value may appear anywhere in JSON.
    assert!(!json.contains("secret"), "JSON leaks password: {}", json);
    assert!(
        !json.contains("leaked"),
        "JSON leaks api_key value: {}",
        json
    );
    assert!(json.contains("REDACTED"), "JSON should contain REDACTED");
    assert!(
        !report.endpoints[0].endpoint.contains("secret"),
        "endpoint field must not leak password"
    );

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn test_batch_invalid_url_credentials_no_leak_human() {
    let input = "ftp://user:secret@host.example/rpc?api_key=leaked";

    let mut path = std::env::temp_dir();
    path.push("miyo_batch_bad_creds_human.txt");
    std::fs::write(&path, input).unwrap();

    let args = BatchArgs {
        file: path.to_string_lossy().to_string(),
        output: OutputFormat::Human,
        timeout_secs: 5,
        runs: 1,
        method: None,
        concurrency: 1,
        expected_network: None,
    };
    let report = miyo_rpc::run_batch(&args).await;
    let human = miyo_rpc::format_batch_human(&report);

    assert!(
        !human.contains("secret"),
        "human output leaks password: {}",
        human
    );
    assert!(
        !human.contains("leaked"),
        "human output leaks api_key: {}",
        human
    );
    assert!(
        human.contains("REDACTED"),
        "human output should contain REDACTED"
    );

    let _ = std::fs::remove_file(&path);
}

/// Unparseable URL strings that contain apparent userinfo must also be
/// redacted in batch output (defense-in-depth for malformed input).
#[tokio::test]
async fn test_batch_unparseable_url_credentials_redacted() {
    let input = "https://user:secret@bad host.example/rpc?api_key=leaked\n";

    let mut path = std::env::temp_dir();
    path.push("miyo_batch_unparseable.txt");
    std::fs::write(&path, input).unwrap();

    let args = BatchArgs {
        file: path.to_string_lossy().to_string(),
        output: OutputFormat::Json,
        timeout_secs: 5,
        runs: 1,
        method: None,
        concurrency: 1,
        expected_network: None,
    };
    let report = miyo_rpc::run_batch(&args).await;
    let json = miyo_rpc::format_batch_json(&report).unwrap();

    assert!(!report.endpoints[0].valid);
    assert!(!json.contains("secret"), "JSON leaks password: {}", json);
    assert!(!json.contains("leaked"), "JSON leaks api_key: {}", json);
    assert!(json.contains("REDACTED"));

    let _ = std::fs::remove_file(&path);
}

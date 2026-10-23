//! Integration tests for diagnostic checks using a mock RPC server.
//!
//! All tests use `mockito` to spin up a local HTTP server that returns
//! deterministic JSON-RPC responses. No live Stellar endpoint is required.

use miyo_rpc::{sanitize_url, CheckStatus, DiagnosticReport, RpcClient, RpcClientConfig};
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
    assert!(sanitized.contains("[REDACTED]"));
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
    assert!(sanitized.contains("[REDACTED]"));
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

    let report = DiagnosticReport::run_all(&url, Duration::from_secs(5), None).await;

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

    let report = DiagnosticReport::run_all(&url, Duration::from_secs(5), None).await;

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

    let report = DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getHealth")).await;

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

    let report = DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getHealth")).await;
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

    let report = DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getHealth")).await;
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
        DiagnosticReport::run_all("http://127.0.0.1:1", Duration::from_millis(200), None).await;

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
    let report = DiagnosticReport::run_all(url_with_secret, Duration::from_secs(5), None).await;

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
    assert!(json_output.contains("[REDACTED]"));
}

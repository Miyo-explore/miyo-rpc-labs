//! Integration tests for diagnostic checks using a mock RPC server.
//!
//! All tests use `mockito` to spin up a local HTTP server that returns
//! deterministic JSON-RPC responses. No live Stellar endpoint is required.

use miyo_rpc::{
    sanitize_url, BatchArgs, CheckStatus, DiagnosticReport, OutputFormat, RpcClient,
    RpcClientConfig, TtlCheckConfig,
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

    // Mock getEvents. The check first resolves startLedger via
    // getLatestLedger (reusing the mock above, which returns the sequence
    // as a number); the string form is covered by the dedicated test.
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getEvents""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "events": [
                    {"type": "contract", "ledger": "2002985", "ledgerClosedAt": "1765223032", "contractId": "CDLZUD4T7B6R2P2L2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2", "topic": ["transfer"], "value": "AAAAAQ=="},
                    {"type": "system", "ledger": "2002985", "ledgerClosedAt": "1765223032", "topic": [], "value": "AAAAAQ=="}
                ],
                "latestLedger": 2002985,
                "oldestLedger": 2002900,
                "cursor": "2002985-0000000001"
            }
        })))
        .create();

    // Mock getLedgerEntries. The probe key has no entry on this mock —
    // an empty array is still a Pass.
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getLedgerEntries""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "entries": [],
                "latestLedger": 2002985
            }
        })))
        .create();

    // Mock getLedgers. The check first resolves startLedger via
    // getLatestLedger (reusing the mock above). The window is wide
    // enough (17,985 ledgers retained) that `retentionWindow` passes
    // against the default 17,280 threshold, like a healthy endpoint.
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getLedgers""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "ledgers": [
                    {"hash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984", "sequence": 2002985, "ledgerCloseTime": "1765223032"}
                ],
                "latestLedger": 2002985,
                "oldestLedger": 1985000,
                "cursor": "2002985"
            }
        })))
        .create();

    // Mock getTransactions. The check first resolves startLedger via
    // getLatestLedger (reusing the mock above). The latest ledger holds
    // one transaction, which is still a Pass.
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getTransactions""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "transactions": [
                    {"status": "SUCCESS", "txHash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984", "ledger": 2002985, "createdAt": "1765223032", "applicationOrder": 1, "feeCharged": "100", "operationCount": 1, "envelopeXdr": "AAAAAQ==", "resultXdr": "AAAAAAAAAAGQAAAAAAAAAA", "resultMetaXdr": "AAAAAA=="}
                ],
                "latestLedger": 2002985,
                "oldestLedger": 2002900,
                "cursor": "2002985-0000000001"
            }
        })))
        .create();

    // Mock getFeeStats. No params are sent; the response carries the
    // two fee objects plus latestLedger.
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getFeeStats",
            "params": null
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "sorobanInclusionFee": {
                    "max": 600, "min": 100, "mode": 100,
                    "p10": 100, "p20": 100, "p30": 100, "p40": 100,
                    "p50": 100, "p60": 100, "p70": 100, "p80": 100,
                    "p90": 100, "p95": 200, "p99": 500,
                    "transactionCount": 575, "ledgerCount": 50
                },
                "inclusionFee": {
                    "max": 300, "min": 100, "mode": 100,
                    "p10": 100, "p20": 100, "p30": 100, "p40": 100,
                    "p50": 100, "p60": 100, "p70": 100, "p80": 100,
                    "p90": 100, "p95": 100, "p99": 200,
                    "transactionCount": 153, "ledgerCount": 10
                },
                "latestLedger": 2002985
            }
        })))
        .create();

    // Mock getTransaction. The check resolves the hash from the
    // getTransactions mock above (its single transaction) and sends
    // exactly {"hash": <that hash>}.
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTransaction",
            "params": {"hash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984"}
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "SUCCESS",
                "txHash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984",
                "ledger": 2002985,
                "createdAt": "1765223032",
                "applicationOrder": 1,
                "feeCharged": "100",
                "operationCount": 1,
                "envelopeXdr": "AAAAAQ==",
                "resultXdr": "AAAAAAAAAAGQAAAAAAAAAA",
                "resultMetaXdr": "AAAAAA=="
            }
        })))
        .create();

    // Mock simulateTransaction. The check first calls getNetwork (mock
    // above returns the Testnet passphrase), then replays the embedded
    // Testnet probe envelope; the exact envelope bytes are not matched
    // here — the dedicated tests pin the request shape.
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"simulateTransaction""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "latestLedger": 2002985,
                "minResourceFee": "31052",
                "cost": {"cpuInsns": "1036915", "memBytes": "51240"},
                "results": [{"auth": [], "retval": "AAAAAQAAAAA="}],
                "transactionData": "AAAAAQ==",
                "events": [],
                "stateChanges": []
            }
        })))
        .create();

    let report = DiagnosticReport::run_all(&url, Duration::from_secs(5), None, 1).await;

    // Should have httpConnectivity + 12 method checks = 13 total
    assert_eq!(report.checks.len(), 13);

    // HTTP connectivity should pass (200)
    let http_check = report.checks.iter().find(|c| c.name == "httpConnectivity");
    assert!(http_check.is_some());
    assert_eq!(http_check.unwrap().status, CheckStatus::Pass);

    // All 12 RPC method checks should pass
    for name in [
        "getHealth",
        "getNetwork",
        "getLatestLedger",
        "getVersionInfo",
        "getEvents",
        "getLedgerEntries",
        "getLedgers",
        "retentionWindow",
        "getTransactions",
        "getFeeStats",
        "getTransaction",
        "simulateTransaction",
    ] {
        let check = report.checks.iter().find(|c| c.name == name);
        assert!(check.is_some(), "should have check for {name}");
        assert_eq!(
            check.unwrap().status,
            CheckStatus::Pass,
            "check for {name} should pass"
        );
    }

    // getEvents resolved startLedger from getLatestLedger and reported the page.
    let events_check = find_check(&report, "getEvents");
    let details = events_check
        .details
        .as_ref()
        .expect("getEvents should carry details");
    assert_eq!(details.get("start_ledger"), Some(&Value::from(2002985u64)));
    assert_eq!(details.get("events_returned"), Some(&Value::from(2u64)));
    assert_eq!(details.get("has_cursor"), Some(&Value::Bool(true)));

    // getLedgerEntries reported the empty result; getLedgers resolved
    // startLedger from getLatestLedger and reported the one ledger.
    let entries_check = find_check(&report, "getLedgerEntries");
    let details = entries_check
        .details
        .as_ref()
        .expect("getLedgerEntries should carry details");
    assert_eq!(details.get("entries_returned"), Some(&Value::from(0u64)));
    let ledgers_check = find_check(&report, "getLedgers");
    let details = ledgers_check
        .details
        .as_ref()
        .expect("getLedgers should carry details");
    assert_eq!(details.get("start_ledger"), Some(&Value::from(2002985u64)));
    assert_eq!(details.get("ledgers_returned"), Some(&Value::from(1u64)));
    assert_eq!(details.get("has_cursor"), Some(&Value::Bool(true)));

    // getTransactions resolved startLedger from getLatestLedger and
    // reported the one transaction.
    let transactions_check = find_check(&report, "getTransactions");
    let details = transactions_check
        .details
        .as_ref()
        .expect("getTransactions should carry details");
    assert_eq!(details.get("start_ledger"), Some(&Value::from(2002985u64)));
    assert_eq!(
        details.get("transactions_returned"),
        Some(&Value::from(1u64))
    );
    assert_eq!(details.get("has_cursor"), Some(&Value::Bool(true)));

    // getFeeStats reported the fee percentiles from the two fee objects.
    let fee_stats_check = find_check(&report, "getFeeStats");
    let details = fee_stats_check
        .details
        .as_ref()
        .expect("getFeeStats should carry details");
    assert_eq!(
        details.get("soroban_inclusion_fee_mode"),
        Some(&Value::from(100u64))
    );
    assert_eq!(
        details.get("soroban_inclusion_fee_p95"),
        Some(&Value::from(200u64))
    );
    assert_eq!(
        details.get("soroban_inclusion_fee_transaction_count"),
        Some(&Value::from(575u64))
    );
    assert_eq!(
        details.get("inclusion_fee_mode"),
        Some(&Value::from(100u64))
    );
    assert_eq!(details.get("latest_ledger"), Some(&Value::from(2002985u64)));

    // getTransaction resolved the hash from the getTransactions page and
    // reported the single transaction.
    let transaction_check = find_check(&report, "getTransaction");
    let details = transaction_check
        .details
        .as_ref()
        .expect("getTransaction should carry details");
    assert_eq!(
        details.get("tx_hash"),
        Some(&Value::String(
            "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984".to_string()
        ))
    );
    assert_eq!(
        details.get("status"),
        Some(&Value::String("SUCCESS".to_string()))
    );
    assert_eq!(details.get("ledger"), Some(&Value::from(2002985u64)));
    assert_eq!(
        details.get("fee_charged"),
        Some(&Value::String("100".to_string()))
    );
    assert_eq!(details.get("has_envelope_xdr"), Some(&Value::Bool(true)));
    assert_eq!(details.get("has_result_xdr"), Some(&Value::Bool(true)));
    assert_eq!(details.get("has_result_meta_xdr"), Some(&Value::Bool(true)));

    // retentionWindow derived the retained window from the same
    // getLedgers page: 17,985 ledgers (~1.04 days), above threshold.
    let retention_check = find_check(&report, "retentionWindow");
    let details = retention_check
        .details
        .as_ref()
        .expect("retentionWindow should carry details");
    assert_eq!(
        details.get("retained_ledgers"),
        Some(&Value::from(17_985u64))
    );
    assert_eq!(
        details.get("retained_days_approx"),
        Some(&Value::from(1.04f64))
    );
    assert_eq!(details.get("min_retention"), Some(&Value::from(17_280u64)));

    // simulateTransaction replayed the Testnet probe envelope and
    // reported the simulation result.
    let simulate_check = find_check(&report, "simulateTransaction");
    let details = simulate_check
        .details
        .as_ref()
        .expect("simulateTransaction should carry details");
    assert_eq!(
        details.get("min_resource_fee"),
        Some(&Value::from(31052u64))
    );
    assert_eq!(details.get("results_count"), Some(&Value::from(1u64)));
    assert_eq!(details.get("latest_ledger"), Some(&Value::from(2002985u64)));
    assert_eq!(
        details.get("cost_cpu_insns"),
        Some(&Value::from(1036915u64))
    );
    assert_eq!(details.get("cost_mem_bytes"), Some(&Value::from(51240u64)));

    assert_eq!(report.summary.passed, 13);
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
            r#""method":"(getHealth|getNetwork|getLatestLedger|getVersionInfo|getEvents|getLedgerEntries|getLedgers|getTransactions|getFeeStats|getTransaction|simulateTransaction)""#
                .to_string(),
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

    // 12 RPC method checks should all be Fail (getEvents, getLedgers,
    // retentionWindow, getTransactions, getTransaction, and
    // simulateTransaction fail too: their internal getLatestLedger /
    // getNetwork calls hit the same -32601 mock).
    let rpc_checks: Vec<_> = report
        .checks
        .iter()
        .filter(|c| c.name != "httpConnectivity")
        .collect();
    assert_eq!(rpc_checks.len(), 12);
    for c in &rpc_checks {
        assert_eq!(c.status, CheckStatus::Fail, "{} should fail", c.name);
    }

    assert_eq!(report.summary.failed, 13);
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
async fn test_sanitize_url_ipv6_userinfo() {
    // IPv6 literal with userinfo credentials: credentials must be redacted
    // while the bracketed host and port are preserved.
    let url = "http://user:pass@[::1]:8000/";
    let sanitized = sanitize_url(url);
    assert!(
        !sanitized.contains("user:pass"),
        "userinfo credentials should be redacted: {sanitized}"
    );
    assert!(
        sanitized.contains("[::1]:8000"),
        "IPv6 host and port should be preserved: {sanitized}"
    );
    assert!(sanitized.contains("REDACTED"));
}

#[tokio::test]
async fn test_sanitize_url_ipv6_sensitive_query() {
    // IPv6 literal with a sensitive query parameter: the secret value must
    // be redacted while non-sensitive parameters survive.
    let url = "http://[2001:db8::1]:8000/rpc?api_key=secret123&x=1";
    let sanitized = sanitize_url(url);
    assert!(
        !sanitized.contains("secret123"),
        "api_key should be redacted: {sanitized}"
    );
    assert!(
        sanitized.contains("x=1"),
        "non-sensitive param should remain: {sanitized}"
    );
    assert!(
        sanitized.contains("[2001:db8::1]:8000"),
        "IPv6 host and port should be preserved: {sanitized}"
    );
    assert!(sanitized.contains("REDACTED"));
}

#[tokio::test]
async fn test_sanitize_url_ipv6_malformed_userinfo() {
    // Malformed IPv6 literal (unclosed bracket) that fails to parse:
    // the fallback path must still redact the embedded credentials.
    let url = "http://user:pass@[::1:8000/";
    let sanitized = sanitize_url(url);
    assert!(
        !sanitized.contains("user:pass"),
        "userinfo credentials should be redacted: {sanitized}"
    );
    assert!(
        sanitized.contains("[::1:8000/"),
        "host portion should be preserved: {sanitized}"
    );
    assert!(sanitized.contains("REDACTED"));
}

#[tokio::test]
async fn test_sanitize_url_idn_userinfo() {
    // Internationalized hostname with userinfo credentials. The `url`
    // crate normalizes the host to punycode; credentials must be redacted
    // regardless of the hostname encoding.
    let url = "https://user:pass@münchen.example/rpc";
    let sanitized = sanitize_url(url);
    assert!(
        !sanitized.contains("user:pass"),
        "userinfo credentials should be redacted: {sanitized}"
    );
    assert!(
        sanitized.contains("xn--mnchen-3ya.example"),
        "punycode host should be preserved: {sanitized}"
    );
    assert!(sanitized.contains("/rpc"));
    assert!(sanitized.contains("REDACTED"));
}

#[tokio::test]
async fn test_sanitize_url_idn_sensitive_query() {
    // Punycode hostname with a sensitive query parameter.
    let url = "https://xn--mnchen-3ya.example/rpc?token=abc456&ok=yes";
    let sanitized = sanitize_url(url);
    assert!(
        !sanitized.contains("abc456"),
        "token should be redacted: {sanitized}"
    );
    assert!(
        sanitized.contains("ok=yes"),
        "non-sensitive param should remain: {sanitized}"
    );
    assert!(
        sanitized.contains("xn--mnchen-3ya.example"),
        "IDN host should be preserved: {sanitized}"
    );
    assert!(sanitized.contains("REDACTED"));
}

#[tokio::test]
async fn test_sanitize_url_malformed_ipv6_with_credentials() {
    // Malformed URL (space in the query) containing an IPv6 literal and
    // credentials: the fallback path must redact both the userinfo and the
    // sensitive query parameter.
    let url = "http://user:pass@[::1]:8000/rpc?api_key=secret with space";
    let sanitized = sanitize_url(url);
    assert!(
        !sanitized.contains("user:pass"),
        "userinfo credentials should be redacted: {sanitized}"
    );
    assert!(
        !sanitized.contains("secret"),
        "api_key should be redacted: {sanitized}"
    );
    assert!(
        sanitized.contains("[::1]:8000"),
        "IPv6 host and port should be preserved: {sanitized}"
    );
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
// RPC client flag tests (--user-agent, --max-response-bytes)
// ────────────────────────────────

#[test]
fn test_rpc_client_config_defaults_and_builders() {
    let default = RpcClientConfig::default();
    assert_eq!(
        default.user_agent,
        format!("miyo-rpc/{}", miyo_rpc::VERSION),
        "default User-Agent must stay miyo-rpc/<VERSION>"
    );
    assert_eq!(
        default.max_response_bytes,
        10 * 1024 * 1024,
        "default response cap must stay 10 MiB"
    );
    assert!(default.follow_redirects);

    let custom = RpcClientConfig::new(Duration::from_secs(5))
        .with_user_agent("custom/1.0")
        .with_max_response_bytes(1024);
    assert_eq!(custom.user_agent, "custom/1.0");
    assert_eq!(custom.max_response_bytes, 1024);
    // The builders touch only their own field.
    assert_eq!(custom.timeout, Duration::from_secs(5));
    assert!(custom.follow_redirects);

    let no_redirects = RpcClientConfig::new(Duration::from_secs(5)).with_follow_redirects(false);
    assert!(!no_redirects.follow_redirects);
    // Only the redirect policy changes; everything else stays default.
    assert_eq!(no_redirects.timeout, Duration::from_secs(5));
    assert_eq!(
        no_redirects.user_agent,
        format!("miyo-rpc/{}", miyo_rpc::VERSION)
    );
    assert_eq!(no_redirects.max_response_bytes, 10 * 1024 * 1024);
}

/// A 302 redirect is followed by default: the client ends up at the
/// redirect target and the JSON-RPC call succeeds.
#[tokio::test]
async fn test_redirect_followed_by_default() {
    let mut server = mockito::Server::new_async().await;
    let target = format!("{}/target", server.url());

    let redirect = server
        .mock("POST", "/")
        .with_status(302)
        .with_header("location", target.as_str())
        .create_async()
        .await;

    // reqwest converts POST to GET when following a 302, so the target
    // mock answers GET.
    let target_mock = server
        .mock("GET", "/target")
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
    let client = RpcClient::new(&server.url(), config).expect("should create client");

    let result = client.call::<Value>("getHealth", None).await;
    assert!(result.is_ok(), "redirect should be followed by default");
    let (info, body) = result.unwrap();
    assert_eq!(info.status, 200);
    assert_eq!(body.get("status").and_then(|v| v.as_str()), Some("healthy"));

    redirect.assert_async().await;
    target_mock.assert_async().await;
}

/// With redirect following disabled, the same 302 is reported as an
/// HTTP error instead of being followed: the target is never contacted.
#[tokio::test]
async fn test_redirect_not_followed_with_policy_none() {
    let mut server = mockito::Server::new_async().await;
    let target = format!("{}/target", server.url());

    let redirect = server
        .mock("POST", "/")
        .with_status(302)
        .with_header("location", target.as_str())
        .create_async()
        .await;

    let target_mock = server
        .mock("GET", "/target")
        .expect(0)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "status": "healthy" }
        })))
        .create_async()
        .await;

    let config = RpcClientConfig::new(Duration::from_secs(5)).with_follow_redirects(false);
    let client = RpcClient::new(&server.url(), config).expect("should create client");

    let result = client.call::<Value>("getHealth", None).await;
    assert!(result.is_err(), "redirect must not be followed");
    let err_str = result.err().unwrap().to_string();
    assert!(
        err_str.contains("302"),
        "error should report the 302 status: {err_str}"
    );

    redirect.assert_async().await;
    target_mock.assert_async().await;
}

/// The httpConnectivity probe follows redirects by default: a 302 to a
/// healthy target passes with the target's status.
#[tokio::test]
async fn test_http_connectivity_follows_redirect_by_default() {
    let mut server = mockito::Server::new_async().await;
    let target = format!("{}/ok", server.url());

    let redirect = server
        .mock("GET", "/")
        .with_status(302)
        .with_header("location", target.as_str())
        .create_async()
        .await;

    let target_mock = server
        .mock("GET", "/ok")
        .with_status(200)
        .create_async()
        .await;

    let result = miyo_rpc::diagnostics::http_connectivity_check(
        &server.url(),
        Duration::from_secs(5),
        "miyo-rpc/test",
        true,
    )
    .await;

    assert_eq!(result.status, CheckStatus::Pass);
    assert_eq!(result.http_status, Some(200));

    redirect.assert_async().await;
    target_mock.assert_async().await;
}

/// With redirect following disabled, the httpConnectivity probe reports
/// the 302 as a failure and never contacts the redirect target.
#[tokio::test]
async fn test_http_connectivity_redirect_not_followed() {
    let mut server = mockito::Server::new_async().await;
    let target = format!("{}/ok", server.url());

    let redirect = server
        .mock("GET", "/")
        .with_status(302)
        .with_header("location", target.as_str())
        .create_async()
        .await;

    let target_mock = server
        .mock("GET", "/ok")
        .expect(0)
        .with_status(200)
        .create_async()
        .await;

    let result = miyo_rpc::diagnostics::http_connectivity_check(
        &server.url(),
        Duration::from_secs(5),
        "miyo-rpc/test",
        false,
    )
    .await;

    assert_eq!(result.status, CheckStatus::Fail);
    assert_eq!(result.http_status, Some(302));
    let message = result.error_message.clone().unwrap_or_default();
    assert!(
        message.contains("redirect not followed"),
        "error should name the redirect: {message}"
    );
    let details = result.details.clone().expect("details present");
    assert_eq!(
        details.get("redirect_location").and_then(|v| v.as_str()),
        Some(target.as_str())
    );

    redirect.assert_async().await;
    target_mock.assert_async().await;
}

#[tokio::test]
async fn test_default_user_agent_sent_on_wire() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    let expected_ua = format!("miyo-rpc/{}", miyo_rpc::VERSION);
    let mock = server
        .mock("POST", "/")
        .match_header("user-agent", expected_ua.as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "status": "healthy" }
        })))
        .create_async()
        .await;

    let config = RpcClientConfig::new(Duration::from_secs(5));
    let client = RpcClient::new(&url, config).expect("should create client");

    let result = client.call::<Value>("getHealth", None).await;
    assert!(result.is_ok(), "call should succeed");

    mock.assert_async().await;
}

#[tokio::test]
async fn test_custom_user_agent_sent_on_wire() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    let mock = server
        .mock("POST", "/")
        .match_header("user-agent", "custom/1.0")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "status": "healthy" }
        })))
        .create_async()
        .await;

    let config = RpcClientConfig::new(Duration::from_secs(5)).with_user_agent("custom/1.0");
    let client = RpcClient::new(&url, config).expect("should create client");

    let result = client.call::<Value>("getHealth", None).await;
    assert!(result.is_ok(), "call should succeed");

    mock.assert_async().await;
}

#[tokio::test]
async fn test_report_with_custom_user_agent() {
    // The override must flow through run_all_with_config into both the
    // httpConnectivity GET and the JSON-RPC POST, and the connectivity
    // evidence must name the custom value.
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    let get_mock = server
        .mock("GET", "/")
        .match_header("user-agent", "custom/1.0")
        .with_status(200)
        .create_async()
        .await;

    server
        .mock("POST", "/")
        .match_header("user-agent", "custom/1.0")
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

    let config = RpcClientConfig::new(Duration::from_secs(5)).with_user_agent("custom/1.0");
    let report = DiagnosticReport::run_all_with_config(&url, config, Some("getHealth"), 1).await;

    let health_check = report
        .checks
        .iter()
        .find(|c| c.name == "getHealth")
        .expect("should have getHealth check");
    assert_eq!(health_check.status, CheckStatus::Pass);

    let http_check = report
        .checks
        .iter()
        .find(|c| c.name == "httpConnectivity")
        .expect("should have httpConnectivity check");
    assert_eq!(http_check.status, CheckStatus::Pass);
    assert!(
        http_check
            .evidence
            .as_deref()
            .unwrap_or("")
            .contains("custom/1.0"),
        "connectivity evidence should name the custom User-Agent: {:?}",
        http_check.evidence
    );

    get_mock.assert_async().await;
}

#[tokio::test]
async fn test_report_max_response_bytes_rejects_oversized() {
    // A small cap wired through run_all_with_config must make an oversized
    // getHealth response fail the check with the size-limit error.
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

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

    let config = RpcClientConfig::new(Duration::from_secs(5)).with_max_response_bytes(50);
    let report = DiagnosticReport::run_all_with_config(&url, config, Some("getHealth"), 1).await;

    let health_check = report
        .checks
        .iter()
        .find(|c| c.name == "getHealth")
        .expect("should have getHealth check");
    assert_eq!(
        health_check.status,
        CheckStatus::Fail,
        "oversized response should fail the check"
    );
    let msg = health_check.error_message.clone().unwrap_or_default();
    assert!(
        msg.contains("exceeds maximum size"),
        "error should mention size limit: {msg}"
    );
    assert!(
        !msg.contains("AAAA"),
        "error must not leak response body content: {msg}"
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

// ────────────────────────────
// Warn tier for ledger progression
// ────────────────────────────

/// Mock a `getLatestLedger` success with an explicit sequence (number
/// form, as live endpoints return it) and ledger hash. Each mock answers
/// exactly one call, so sequential mocks script advancing / changing
/// ledgers across `--runs N`.
fn mock_latest_ledger_seq_hash(server: &mut mockito::Server, sequence: u64, id: &str) {
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getLatestLedger""#.to_string(),
        ))
        .expect(1)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "id": id,
                "sequence": sequence,
                "closeTime": "1765223032"
            }
        })))
        .create();
}

/// A ledger that does not advance across `--runs 2` downgrades
/// `getLatestLedger` from Pass to Warn — a frozen endpoint must not look
/// healthy in CI — while the exit-code signal stays clean (failed == 0).
#[tokio::test]
async fn test_ledger_health_stagnant_warns() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLatestLedger"), 2).await;

    let check = find_check(&report, "getLatestLedger");
    assert_eq!(check.status, CheckStatus::Warn);
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("possibly stale"),
        "evidence must name the staleness verdict: {evidence}"
    );
    assert_eq!(report.summary.warned, 1);
    assert_eq!(report.summary.passed, 1); // httpConnectivity only
    assert_eq!(report.summary.failed, 0);
    assert_eq!(report.summary.skipped, 11);
}

/// A `getLatestLedger` result with hundred-kilobyte XDR blobs must still
/// Warn on a stagnant ledger: the short fields (`sequence`, `id`) are
/// kept in `details` even when the blobs are dropped, so the health
/// analysis sees them. (Before the fix, the all-or-nothing size cap
/// discarded the whole result, `details` had no `id`, and the analysis
/// misread "no hashes" as an inconsistency — a false Warn on the live
/// Testnet, whose ledger advances normally.)
#[tokio::test]
async fn test_ledger_health_large_result_still_warns() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

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
                "sequence": 5841158,
                "closeTime": "1765223032",
                "headerXdr": "A".repeat(3000),
                "metadataXdr": "B".repeat(3000)
            }
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLatestLedger"), 2).await;

    let check = find_check(&report, "getLatestLedger");
    assert_eq!(check.status, CheckStatus::Warn);
    let details = check.details.as_ref().expect("getLatestLedger details");
    assert!(
        details.get("id").is_some(),
        "short fields must survive a large result: {details:?}"
    );
    assert_eq!(
        details.get("sequence"),
        Some(&Value::from(5841158u64)),
        "sequence must survive a large result"
    );
    assert!(
        details.get("headerXdr").is_none(),
        "XDR blobs must still be dropped from details"
    );
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("possibly stale"),
        "same seq + same hash must be stale, not inconsistent: {evidence}"
    );
    assert_eq!(report.summary.warned, 1);
    assert_eq!(report.summary.failed, 0);
}

/// Same sequence with a changing hash across runs is a ledger
/// inconsistency, which also Warns.
#[tokio::test]
async fn test_ledger_health_inconsistent_warns() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger_seq_hash(&mut server, 5841158, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    mock_latest_ledger_seq_hash(&mut server, 5841158, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLatestLedger"), 2).await;

    let check = find_check(&report, "getLatestLedger");
    assert_eq!(check.status, CheckStatus::Warn);
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("inconsistency"),
        "evidence must name the inconsistency verdict: {evidence}"
    );
    assert_eq!(report.summary.warned, 1);
    assert_eq!(report.summary.failed, 0);
}

/// An advancing ledger stays Pass across runs — no false Warn.
#[tokio::test]
async fn test_ledger_health_advancing_stays_pass() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger_seq_hash(&mut server, 5841158, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    mock_latest_ledger_seq_hash(&mut server, 5841159, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLatestLedger"), 2).await;

    let check = find_check(&report, "getLatestLedger");
    assert_eq!(check.status, CheckStatus::Pass);
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("advanced"),
        "evidence must name the healthy verdict: {evidence}"
    );
    assert_eq!(report.summary.warned, 0);
    assert_eq!(report.summary.passed, 2);
}

/// A single run stays inconclusive: no false Warn from one sample.
#[tokio::test]
async fn test_ledger_health_single_run_stays_pass() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLatestLedger"), 1).await;

    let check = find_check(&report, "getLatestLedger");
    assert_eq!(check.status, CheckStatus::Pass);
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("inconclusive"),
        "single run must stay inconclusive: {evidence}"
    );
    assert_eq!(report.summary.warned, 0);
}

/// Warn is visible in the machine-readable report: status `"warn"` and
/// a `warned` summary count (additive — `schema_version` stays `"1.0"`).
#[tokio::test]
async fn test_warn_in_json_output() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLatestLedger"), 2).await;

    let json_output = miyo_rpc::format_json(&report).expect("JSON serialization");
    let parsed: serde_json::Value = serde_json::from_str(&json_output).expect("valid JSON");
    assert_eq!(
        parsed.get("schema_version").and_then(|v| v.as_str()),
        Some("1.0"),
        "Warn is additive: schema_version must not bump"
    );
    let check = parsed
        .get("checks")
        .and_then(|v| v.as_array())
        .and_then(|cs| {
            cs.iter()
                .find(|c| c.get("name").and_then(|n| n.as_str()) == Some("getLatestLedger"))
        })
        .expect("getLatestLedger present in JSON");
    assert_eq!(check.get("status").and_then(|v| v.as_str()), Some("warn"));
    assert_eq!(
        parsed
            .get("summary")
            .and_then(|v| v.get("warned"))
            .and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        parsed
            .get("summary")
            .and_then(|v| v.get("failed"))
            .and_then(|v| v.as_u64()),
        Some(0)
    );
}

/// Warn is visible in the human report: a magenta WARN label and a
/// `warned` count in the summary line.
#[tokio::test]
async fn test_warn_in_human_output() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLatestLedger"), 2).await;

    let out = miyo_rpc::format_human_with_color(&report, true);
    assert!(
        out.contains("\x1b[35mWARN\x1b[0m"),
        "WARN must render magenta: {out}"
    );
    assert!(
        out.contains("1 warned"),
        "summary line must count warned: {out}"
    );
    let plain = miyo_rpc::format_human_with_color(&report, false);
    assert!(
        plain.contains("WARN"),
        "WARN text must render without color"
    );
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
            warned: 0,
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
        Some("Test SDF Network ; September 2015"),
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
            warned: 0,
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
            warned: 0,
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
            warned: 0,
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

/// Raw key=value strings without a URL scheme must not leak credentials
/// in batch output (error messages, JSON, human-readable).
#[tokio::test]
async fn test_batch_raw_key_value_no_leak() {
    let input = "secret_key=sk-abc123value\n";

    let mut path = std::env::temp_dir();
    path.push("miyo_batch_rawkv.txt");
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
    assert!(
        !json.contains("«redacted:sk-…»"),
        "JSON leaks raw secret: {}",
        json
    );
    assert!(json.contains("REDACTED"));

    let _ = std::fs::remove_file(&path);
}

/// Bare secret-like tokens (no URL scheme, no `?`, no `=`) must not leak.
#[tokio::test]
async fn test_batch_bare_secret_token_no_leak() {
    let input = "pk_live_abcdef0123456789\n";

    let mut path = std::env::temp_dir();
    path.push("miyo_batch_bare_secret.txt");
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
    assert!(
        !json.contains("pk_live_abcdef0123456789"),
        "bare secret leaked in JSON: {}",
        json
    );
    assert!(json.contains("REDACTED"));

    let _ = std::fs::remove_file(&path);
}

/// Composite sensitive keys in raw key=value input must not leak.
#[tokio::test]
async fn test_batch_composite_key_no_leak() {
    let input = "my_secret=s3cr3tvalue\n";

    let mut path = std::env::temp_dir();
    path.push("miyo_batch_composite.txt");
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
    assert!(
        !json.contains("s3cr3tvalue"),
        "composite key value leaked: {}",
        json
    );
    assert!(json.contains("REDACTED"));

    let _ = std::fs::remove_file(&path);
}

/// URL-encoded sensitive parameter names in malformed URLs.
#[tokio::test]
async fn test_batch_encoded_param_name_no_leak() {
    // Unparseable URL (space in host) with URL-encoded api_key param name
    let input = "https://user:secret@bad host.example?api%5fkey=leaked\n";

    let mut path = std::env::temp_dir();
    path.push("miyo_batch_encoded.txt");
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
    assert!(
        !json.contains("leaked"),
        "encoded param value leaked: {}",
        json
    );
    assert!(json.contains("REDACTED"));

    let _ = std::fs::remove_file(&path);
}

// ────────────────────────────
// Arbitrary JSON-RPC method probe (C1)
// ────────────────────────────

async fn probe(url: &str, method: &str, params: Option<Value>) -> DiagnosticReport {
    DiagnosticReport::run_arbitrary_probe(url, Duration::from_secs(5), method, params, 1).await
}

fn find_check<'a>(report: &'a DiagnosticReport, name: &str) -> &'a miyo_rpc::CheckResult {
    report
        .checks
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("check '{name}' missing"))
}

#[tokio::test]
#[allow(non_snake_case)]
async fn test_arbitrary_method_named_httpConnectivity_success_does_not_collide() {
    // Regression: --method httpConnectivity produced two checks with the same
    // name (the internal HTTP check + the JSON-RPC probe). aggregate() matched
    // by name (first hit wins), so the probe result was silently dropped.
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    // Internal HTTP check does a plain GET — mock it so it passes.
    server
        .mock("GET", "/")
        .with_status(200)
        .create_async()
        .await;
    // Probe does a JSON-RPC POST — mock it with a successful result.
    server
        .mock("POST", "/")
        .match_header("content-type", "application/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0", "id": 1, "result": {"probe": "ok"}
        })))
        .create_async()
        .await;

    let report = probe(&url, "httpConnectivity", Some(json!({"a": 1}))).await;

    // Both checks must survive aggregation under distinct names.
    let names: Vec<&str> = report.checks.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["httpConnectivity", "httpConnectivity (probe)"],
        "probe check must not collide with the internal HTTP check: {names:?}"
    );

    let internal = find_check(&report, "httpConnectivity");
    assert_eq!(internal.status, CheckStatus::Pass);
    let probe_result = find_check(&report, "httpConnectivity (probe)");
    assert_eq!(probe_result.status, CheckStatus::Pass);
    assert_eq!(
        probe_result
            .details
            .as_ref()
            .and_then(|d| d.get("probe"))
            .cloned(),
        Some(json!("ok")),
        "probe result must be represented, not dropped by aggregation"
    );
    assert_eq!(report.summary.ran, 2);
}

#[tokio::test]
#[allow(non_snake_case)]
async fn test_arbitrary_method_named_httpConnectivity_jsonrpc_error_does_not_collide() {
    // Same collision, but the probe fails: the Fail status must not be masked
    // by the internal HTTP connectivity check (which passes).
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    // Internal HTTP check does a plain GET — mock it so it passes.
    server
        .mock("GET", "/")
        .with_status(200)
        .create_async()
        .await;
    // Probe does a JSON-RPC POST — mock it with a JSON-RPC error.
    server
        .mock("POST", "/")
        .match_header("content-type", "application/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0", "id": 1,
            "error": {"code": -32601, "message": "Method not found"}
        })))
        .create_async()
        .await;

    let report = probe(&url, "httpConnectivity", Some(json!({"a": 1}))).await;

    let names: Vec<&str> = report.checks.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["httpConnectivity", "httpConnectivity (probe)"],
        "probe check must not collide with the internal HTTP check: {names:?}"
    );

    let internal = find_check(&report, "httpConnectivity");
    assert_eq!(internal.status, CheckStatus::Pass);
    let probe_result = find_check(&report, "httpConnectivity (probe)");
    assert_eq!(
        probe_result.status,
        CheckStatus::Fail,
        "JSON-RPC error must stay Fail even when the HTTP check passes"
    );
    assert_eq!(report.summary.failed, 1);
}

#[tokio::test]
#[allow(non_snake_case)]
async fn test_arbitrary_method_named_httpConnectivity_multi_run_aggregation() {
    // Multi-run variant: verifies that when --method httpConnectivity is used
    // with --runs > 1, both the internal HTTP check and the probe check remain
    // separate in the aggregated report, and the probe follows worst-status-wins.
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    // Internal HTTP check: plain GET, always passes.
    server
        .mock("GET", "/")
        .with_status(200)
        .create_async()
        .await;

    // Run 1: probe succeeds.
    server
        .mock("POST", "/")
        .match_header("content-type", "application/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0", "id": 1, "result": {"run": 1}
        })))
        .expect(1)
        .create_async()
        .await;

    // Run 2: probe fails with JSON-RPC error.
    server
        .mock("POST", "/")
        .match_header("content-type", "application/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0", "id": 1,
            "error": {"code": -32601, "message": "Method not found"}
        })))
        .expect(1)
        .create_async()
        .await;

    let report = DiagnosticReport::run_arbitrary_probe(
        &url,
        Duration::from_secs(5),
        "httpConnectivity",
        Some(json!({"a": 1})),
        2,
    )
    .await;

    // Both checks must be present with distinct names after aggregation.
    let names: Vec<&str> = report.checks.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["httpConnectivity", "httpConnectivity (probe)"],
        "probe check must not collide with the internal HTTP check: {names:?}"
    );

    let internal = find_check(&report, "httpConnectivity");
    assert_eq!(internal.status, CheckStatus::Pass);

    let probe = find_check(&report, "httpConnectivity (probe)");
    assert_eq!(
        probe.status,
        CheckStatus::Fail,
        "probe must be Fail (worst of Pass + Fail across 2 runs)"
    );
    assert_eq!(report.summary.failed, 1);
    assert_eq!(report.summary.passed, 1);
    assert_eq!(report.summary.ran, 2);
}

#[tokio::test]
#[allow(non_snake_case)]
async fn test_arbitrary_object_result_passes_and_merges_keys() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("POST", "/")
        .match_header("content-type", "application/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "ledgers": [1, 2], "count": 2 }
        })))
        .create_async()
        .await;

    let report = probe(&url, "getLedgerEntries", Some(json!({"keys": []}))).await;
    let check = find_check(&report, "getLedgerEntries");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("details present");
    assert_eq!(details.get("count"), Some(&json!(2)));
    assert_eq!(details.get("ledgers"), Some(&json!([1, 2])));
    // evidence must not echo the raw params
    let ev = check.evidence.as_deref().unwrap_or("");
    assert!(!ev.contains("keys"), "evidence leaked params: {ev}");
}

#[tokio::test]
async fn test_arbitrary_params_are_sent_in_request() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    let mock = server
        .mock("POST", "/")
        .match_header("content-type", "application/json")
        .match_body(mockito::Matcher::JsonString(
            json!({
                "jsonrpc": "2.0", "id": 1,
                "method": "getLedgerEntries",
                "params": { "keys": ["abc"] }
            })
            .to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(
            json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}}),
        ))
        .create_async()
        .await;

    let report = probe(&url, "getLedgerEntries", Some(json!({"keys": ["abc"]}))).await;
    assert_eq!(
        find_check(&report, "getLedgerEntries").status,
        CheckStatus::Pass
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn test_arbitrary_array_result() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(
            json!({"jsonrpc": "2.0", "id": 1, "result": [1, 2, 3]}),
        ))
        .create_async()
        .await;

    let report = probe(&url, "getList", None).await;
    let check = find_check(&report, "getList");
    assert_eq!(check.status, CheckStatus::Pass);
    assert_eq!(
        check.details.as_ref().unwrap().get("result"),
        Some(&json!([1, 2, 3]))
    );
}

#[tokio::test]
async fn test_arbitrary_scalar_and_null_results() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({"jsonrpc": "2.0", "id": 1, "result": 42})))
        .create_async()
        .await;
    let report = probe(&url, "getNumber", None).await;
    assert_eq!(
        find_check(&report, "getNumber")
            .details
            .as_ref()
            .unwrap()
            .get("result"),
        Some(&json!(42))
    );

    let mut server2 = mockito::Server::new_async().await;
    let url2 = server2.url();
    server2
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(
            json!({"jsonrpc": "2.0", "id": 1, "result": null}),
        ))
        .create_async()
        .await;
    let report2 = probe(&url2, "getNothing", None).await;
    let check2 = find_check(&report2, "getNothing");
    assert_eq!(check2.status, CheckStatus::Pass);
    assert_eq!(
        check2.details.as_ref().unwrap().get("result"),
        Some(&Value::Null)
    );
}

#[tokio::test]
async fn test_arbitrary_jsonrpc_error_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0", "id": 1,
            "error": { "code": -32601, "message": "method not found" }
        })))
        .create_async()
        .await;

    let report = probe(&url, "noSuchMethod", None).await;
    let check = find_check(&report, "noSuchMethod");
    assert_eq!(
        check.status,
        CheckStatus::Fail,
        "JSON-RPC error must stay Fail"
    );
    let msg = check.error_message.as_deref().unwrap_or("");
    assert!(msg.contains("method not found"), "error message: {msg}");
}

#[tokio::test]
async fn test_arbitrary_malformed_response_fails_without_leaking_body() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("<html>not json</html>")
        .create_async()
        .await;

    let report = probe(&url, "getBroken", None).await;
    let check = find_check(&report, "getBroken");
    assert_eq!(check.status, CheckStatus::Fail);
    let blob = serde_json::to_string(&check.details).unwrap_or_default();
    let msg = check.error_message.as_deref().unwrap_or("").to_string()
        + check.evidence.as_deref().unwrap_or("")
        + &blob;
    assert!(
        !msg.contains("not json"),
        "malformed body must not be echoed: {msg}"
    );
}

#[tokio::test]
async fn test_arbitrary_large_result_is_flagged_truncated() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    let big = "x".repeat(5000);
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(
            json!({"jsonrpc": "2.0", "id": 1, "result": {"blob": big}}),
        ))
        .create_async()
        .await;

    let report = probe(&url, "getBig", None).await;
    let check = find_check(&report, "getBig");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().unwrap();
    assert_eq!(details.get("result_truncated"), Some(&json!(true)));
    assert!(details.get("result_size_bytes").is_some());
    assert!(
        details.get("blob").is_none(),
        "oversized result must not be stored in full"
    );
}

#[tokio::test]
async fn test_arbitrary_runs_aggregation_worst_status_wins() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    let failing = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0", "id": 1,
            "error": { "code": -32000, "message": "boom" }
        })))
        .expect(1)
        .create_async()
        .await;
    let passing = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(
            json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}}),
        ))
        .expect(1)
        .create_async()
        .await;

    let report =
        DiagnosticReport::run_arbitrary_probe(&url, Duration::from_secs(5), "flaky", None, 2).await;
    let check = find_check(&report, "flaky");
    assert_eq!(
        check.status,
        CheckStatus::Fail,
        "worst status must win across runs"
    );
    failing.assert_async().await;
    passing.assert_async().await;
}

#[tokio::test]
async fn test_arbitrary_missing_result_field_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    // Valid envelope, but no "result" and no "error" -> malformed.
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({"jsonrpc": "2.0", "id": 1})))
        .create_async()
        .await;

    let report = probe(&url, "getNoResult", None).await;
    let check = find_check(&report, "getNoResult");
    assert_eq!(check.status, CheckStatus::Fail);
    let msg = check.error_message.as_deref().unwrap_or("");
    assert!(
        msg.contains("result"),
        "expected a result-field error: {msg}"
    );
}

#[tokio::test]
async fn test_arbitrary_malformed_envelope_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    // Missing "jsonrpc" field -> malformed envelope.
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({"id": 1, "result": {"ok": true}})))
        .create_async()
        .await;

    let report = probe(&url, "getBadEnvelope", None).await;
    assert_eq!(
        find_check(&report, "getBadEnvelope").status,
        CheckStatus::Fail
    );
}

#[tokio::test]
async fn test_arbitrary_runs_zero_still_probes() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    let m = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(
            json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}}),
        ))
        .expect(1)
        .create_async()
        .await;

    // A library caller passing runs = 0 must not get a false-success empty report.
    let report =
        DiagnosticReport::run_arbitrary_probe(&url, Duration::from_secs(5), "getZero", None, 0)
            .await;
    assert!(
        !report.checks.is_empty(),
        "runs=0 must not yield an empty report"
    );
    let check = find_check(&report, "getZero");
    assert_eq!(check.status, CheckStatus::Pass);
    assert_eq!(report.runs, 1, "runs=0 must be clamped to a single probe");
    // The probe actually reached the server (mock expected exactly one POST).
    m.assert_async().await;
}

#[tokio::test]
async fn test_arbitrary_mode_omits_builtin_checks() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(
            json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}}),
        ))
        .create_async()
        .await;

    let report = probe(&url, "getLedgerEntries", None).await;
    let names: Vec<&str> = report.checks.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"getLedgerEntries"));
    assert!(names.contains(&"httpConnectivity"));
    for builtin in [
        "getHealth",
        "getNetwork",
        "getLatestLedger",
        "getVersionInfo",
    ] {
        assert!(
            !names.contains(&builtin),
            "{builtin} should not run in arbitrary mode"
        );
    }
}

#[tokio::test]
async fn test_arbitrary_no_param_leak_in_output() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0", "id": 1,
            "error": { "code": -32000, "message": "denied" }
        })))
        .create_async()
        .await;

    let report = probe(
        &url,
        "getSecret",
        Some(json!({"api_key": "SUPERSECRET123"})),
    )
    .await;
    let check = find_check(&report, "getSecret");
    let ev = check.evidence.as_deref().unwrap_or("");
    let em = check.error_message.as_deref().unwrap_or("");
    let det = serde_json::to_string(&check.details).unwrap_or_default();
    for (label, blob) in [("evidence", ev), ("error", em), ("details", &det)] {
        assert!(
            !blob.contains("SUPERSECRET123"),
            "secret leaked via {label}: {blob}"
        );
    }
}

#[tokio::test]
async fn test_builtin_method_filter_unchanged() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "status": "healthy" }
        })))
        .create_async()
        .await;

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getHealth"), 1).await;
    // Existing contract: every built-in check stays present; non-selected ones
    // are reported as Skip (not omitted).
    let names: Vec<&str> = report.checks.iter().map(|c| c.name.as_str()).collect();
    for builtin in [
        "getHealth",
        "getNetwork",
        "getLatestLedger",
        "getVersionInfo",
        "getEvents",
        "getLedgerEntries",
        "getLedgers",
        "getTransactions",
    ] {
        assert!(names.contains(&builtin), "{builtin} should remain present");
    }
    assert!(names.contains(&"httpConnectivity"));
    assert_eq!(find_check(&report, "getHealth").status, CheckStatus::Pass);
    for other in [
        "getNetwork",
        "getLatestLedger",
        "getVersionInfo",
        "getEvents",
        "getLedgerEntries",
        "getLedgers",
        "getTransactions",
    ] {
        assert_eq!(
            find_check(&report, other).status,
            CheckStatus::Skip,
            "{other} should be Skip when filtered out"
        );
    }
}

#[test]
fn test_is_builtin_method_public() {
    assert!(miyo_rpc::is_builtin_method("getHealth"));
    assert!(miyo_rpc::is_builtin_method("getEvents"));
    assert!(miyo_rpc::is_builtin_method("getLedgerEntries"));
    assert!(miyo_rpc::is_builtin_method("getLedgers"));
    assert!(miyo_rpc::is_builtin_method("getTransactions"));
    assert!(miyo_rpc::is_builtin_method("getFeeStats"));
    assert!(miyo_rpc::is_builtin_method("getTransaction"));
    assert!(miyo_rpc::is_builtin_method("simulateTransaction"));
    assert!(miyo_rpc::is_builtin_method("contractTtl"));
    assert!(!miyo_rpc::is_builtin_method("sendTransaction"));
    assert_eq!(miyo_rpc::BUILTIN_METHODS.len(), 13);
}

#[test]
fn test_validate_params_public() {
    assert!(miyo_rpc::validate_params(r#"{"a":1}"#).unwrap().is_some());
    assert!(miyo_rpc::validate_params("[1,2]").unwrap().is_some());
    assert!(miyo_rpc::validate_params("null").unwrap().is_none());
    assert!(miyo_rpc::validate_params("42").is_err());
    assert!(miyo_rpc::validate_params("nope").is_err());
}

// ────────────────────────────
// getEvents built-in check
// ────────────────────────────

/// Mock a `getLatestLedger` success returning `sequence` as the given JSON
/// value (string or number — the wire format varies by response).
fn mock_latest_ledger(server: &mut mockito::Server, sequence: Value) {
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
                "sequence": sequence,
                "closeTime": "1765223032"
            }
        })))
        .create();
}

/// Mock a `getEvents` success returning the given `result` object.
fn mock_get_events_result(server: &mut mockito::Server, result: Value) {
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getEvents""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": result
        })))
        .create();
}

/// The check resolves `startLedger` from `getLatestLedger` (string form)
/// and sends exactly `{"startLedger": <seq>, "pagination": {"limit": 10}}`
/// — no filters, no cursor input. The exact-body matcher proves the
/// request shape, including the absence of a `filters` key.
#[tokio::test]
async fn test_get_events_request_params_from_latest_ledger() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!("2002985"));

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getEvents",
            "params": {"startLedger": 2002985, "pagination": {"limit": 10}}
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "events": [
                    {"type": "contract", "ledger": "2002985", "ledgerClosedAt": "1765223032", "contractId": "CDLZUD4T7B6R2P2L2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2Y2", "topic": ["transfer"], "value": "AAAAAQ=="},
                    {"type": "system", "ledger": "2002985", "ledgerClosedAt": "1765223032", "topic": [], "value": "AAAAAQ=="}
                ],
                "latestLedger": 2002985,
                "oldestLedger": 2002900,
                "cursor": "2002985-0000000001"
            }
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getEvents"), 1).await;

    let check = find_check(&report, "getEvents");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("getEvents details");
    assert_eq!(details.get("start_ledger"), Some(&Value::from(2002985u64)));
    assert_eq!(details.get("events_returned"), Some(&Value::from(2u64)));
    assert_eq!(details.get("latest_ledger"), Some(&Value::from(2002985u64)));
    assert_eq!(details.get("has_cursor"), Some(&Value::Bool(true)));
    // The filter leaves the other built-ins skipped, not omitted.
    assert_eq!(report.summary.passed, 2); // httpConnectivity + getEvents
    assert_eq!(report.summary.skipped, 11);
}

/// An empty page is still a Pass: the method works, the latest ledger
/// simply emitted no events. No cursor is returned for an empty page.
#[tokio::test]
async fn test_get_events_empty_page_passes() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(2002985));
    mock_get_events_result(
        &mut server,
        json!({
            "events": [],
            "latestLedger": 2002985,
            "oldestLedger": 2002900
        }),
    );

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getEvents"), 1).await;

    let check = find_check(&report, "getEvents");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("getEvents details");
    assert_eq!(details.get("events_returned"), Some(&Value::from(0u64)));
    assert_eq!(details.get("has_cursor"), Some(&Value::Bool(false)));
}

/// When `getLatestLedger` fails, `getEvents` cannot resolve a startLedger
/// and must Fail (not silently probe with a guessed ledger).
#[tokio::test]
async fn test_get_events_fails_when_latest_ledger_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

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
            "error": {"code": -32603, "message": "Internal error"}
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getEvents"), 1).await;

    let check = find_check(&report, "getEvents");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::JsonRpcError),
        "underlying getLatestLedger JSON-RPC error must surface"
    );
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("getLatestLedger"),
        "evidence must name the failed dependency: {evidence}"
    );
}

/// A `getEvents` result without an `events` array is malformed, not a Pass.
#[tokio::test]
async fn test_get_events_missing_events_array_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(2002985));
    mock_get_events_result(&mut server, json!({"latestLedger": 2002985}));

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getEvents"), 1).await;

    let check = find_check(&report, "getEvents");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::MalformedResponse),
        "missing 'events' array must classify as MalformedResponse"
    );
    let message = check.error_message.as_deref().unwrap_or("");
    assert!(
        message.contains("events"),
        "error message must name the missing field: {message}"
    );
}

/// A JSON-RPC error from `getEvents` itself (e.g. invalid params) is a Fail
/// with the server's error classification preserved.
#[tokio::test]
async fn test_get_events_json_rpc_error_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(2002985));
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getEvents""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {"code": -32602, "message": "Invalid params"}
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getEvents"), 1).await;

    let check = find_check(&report, "getEvents");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::JsonRpcError),
        "getEvents JSON-RPC error must classify as JsonRpcError"
    );
}

// ────────────────────────────
// getLedgerEntries built-in check
// ────────────────────────────

/// Mock a `getLedgerEntries` success returning the given `result` object.
fn mock_get_ledger_entries_result(server: &mut mockito::Server, result: Value) {
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getLedgerEntries""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": result
        })))
        .create();
}

/// The check sends exactly `{"keys": [<probe LedgerKey>]}` — the fixed
/// synthetic all-zero Account key — with no `ledgerSeq` and no
/// `getLatestLedger` round-trip. The exact-body matcher proves the request
/// shape. The probe key resolves to a real entry on testnet; the check
/// reports it as `entries_returned: 1`.
#[tokio::test]
async fn test_get_ledger_entries_request_params_probe_key() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLedgerEntries",
            "params": {"keys": ["AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="]}
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "entries": [
                    {"key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==", "xdr": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAH3oKlu+AAXqMAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAA", "lastModifiedLedgerSeq": 5629649, "extXdr": "AAAAAA=="}
                ],
                "latestLedger": 5841158
            }
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLedgerEntries"), 1).await;

    let check = find_check(&report, "getLedgerEntries");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("getLedgerEntries details");
    assert_eq!(details.get("entries_returned"), Some(&Value::from(1u64)));
    assert_eq!(details.get("latest_ledger"), Some(&Value::from(5841158u64)));
    // The filter leaves the other built-ins skipped, not omitted.
    assert_eq!(report.summary.passed, 2); // httpConnectivity + getLedgerEntries
    assert_eq!(report.summary.skipped, 11);
}

/// An empty `entries` array is still a Pass: the method works, the probe
/// account simply does not exist on this network.
#[tokio::test]
async fn test_get_ledger_entries_empty_passes() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_get_ledger_entries_result(
        &mut server,
        json!({
            "entries": [],
            "latestLedger": 5841158
        }),
    );

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLedgerEntries"), 1).await;

    let check = find_check(&report, "getLedgerEntries");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("getLedgerEntries details");
    assert_eq!(details.get("entries_returned"), Some(&Value::from(0u64)));
}

/// A `getLedgerEntries` result without an `entries` array is malformed, not
/// a Pass.
#[tokio::test]
async fn test_get_ledger_entries_missing_entries_array_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_get_ledger_entries_result(&mut server, json!({"latestLedger": 5841158}));

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLedgerEntries"), 1).await;

    let check = find_check(&report, "getLedgerEntries");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::MalformedResponse),
        "missing 'entries' array must classify as MalformedResponse"
    );
    let message = check.error_message.as_deref().unwrap_or("");
    assert!(
        message.contains("entries"),
        "error message must name the missing field: {message}"
    );
}

/// A JSON-RPC error from `getLedgerEntries` itself (e.g. invalid key) is a
/// Fail with the server's error classification preserved.
#[tokio::test]
async fn test_get_ledger_entries_json_rpc_error_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getLedgerEntries""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {"code": -32602, "message": "Invalid params"}
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLedgerEntries"), 1).await;

    let check = find_check(&report, "getLedgerEntries");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::JsonRpcError),
        "getLedgerEntries JSON-RPC error must classify as JsonRpcError"
    );
}

// ────────────────────────────
// getLedgers built-in check
// ────────────────────────────

/// Mock a `getLedgers` success returning the given `result` object.
fn mock_get_ledgers_result(server: &mut mockito::Server, result: Value) {
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getLedgers""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": result
        })))
        .create();
}

/// The check resolves `startLedger` from `getLatestLedger` (number form)
/// and sends exactly `{"startLedger": <seq>, "pagination": {"limit": 2}}`.
/// The exact-body matcher proves the request shape.
#[tokio::test]
async fn test_get_ledgers_request_params_from_latest_ledger() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLedgers",
            "params": {"startLedger": 5841158, "pagination": {"limit": 2}}
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "ledgers": [
                    {"hash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984", "sequence": 5841158, "ledgerCloseTime": "1763726400"}
                ],
                "latestLedger": 5841158,
                "latestLedgerCloseTime": 1763726400,
                "oldestLedger": 5720199,
                "oldestLedgerCloseTime": 1763121600,
                "cursor": "5841158"
            }
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLedgers"), 1).await;

    let check = find_check(&report, "getLedgers");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("getLedgers details");
    assert_eq!(details.get("start_ledger"), Some(&Value::from(5841158u64)));
    assert_eq!(details.get("ledgers_returned"), Some(&Value::from(1u64)));
    assert_eq!(details.get("latest_ledger"), Some(&Value::from(5841158u64)));
    assert_eq!(details.get("oldest_ledger"), Some(&Value::from(5720199u64)));
    assert_eq!(details.get("has_cursor"), Some(&Value::Bool(true)));
    // The filter leaves the other built-ins skipped, not omitted.
    assert_eq!(report.summary.passed, 2); // httpConnectivity + getLedgers
    assert_eq!(report.summary.skipped, 11);
}

/// An empty page is still a Pass: the method works, the window simply
/// holds no further ledger. No cursor is returned for an empty page.
#[tokio::test]
async fn test_get_ledgers_empty_page_passes() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!("5841158"));
    mock_get_ledgers_result(
        &mut server,
        json!({
            "ledgers": [],
            "latestLedger": 5841158,
            "oldestLedger": 5720199
        }),
    );

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLedgers"), 1).await;

    let check = find_check(&report, "getLedgers");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("getLedgers details");
    assert_eq!(details.get("ledgers_returned"), Some(&Value::from(0u64)));
    assert_eq!(details.get("has_cursor"), Some(&Value::Bool(false)));
}

/// When `getLatestLedger` fails, `getLedgers` cannot resolve a startLedger
/// and must Fail (not silently probe with a guessed ledger).
#[tokio::test]
async fn test_get_ledgers_fails_when_latest_ledger_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

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
            "error": {"code": -32603, "message": "Internal error"}
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLedgers"), 1).await;

    let check = find_check(&report, "getLedgers");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::JsonRpcError),
        "underlying getLatestLedger JSON-RPC error must surface"
    );
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("getLatestLedger"),
        "evidence must name the failed dependency: {evidence}"
    );
}

/// A `getLedgers` result without a `ledgers` array is malformed, not a Pass.
#[tokio::test]
async fn test_get_ledgers_missing_ledgers_array_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));
    mock_get_ledgers_result(&mut server, json!({"latestLedger": 5841158}));

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLedgers"), 1).await;

    let check = find_check(&report, "getLedgers");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::MalformedResponse),
        "missing 'ledgers' array must classify as MalformedResponse"
    );
    let message = check.error_message.as_deref().unwrap_or("");
    assert!(
        message.contains("ledgers"),
        "error message must name the missing field: {message}"
    );
}

/// A JSON-RPC error from `getLedgers` itself (e.g. invalid params) is a Fail
/// with the server's error classification preserved.
#[tokio::test]
async fn test_get_ledgers_json_rpc_error_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getLedgers""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {"code": -32602, "message": "Invalid params"}
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getLedgers"), 1).await;

    let check = find_check(&report, "getLedgers");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::JsonRpcError),
        "getLedgers JSON-RPC error must classify as JsonRpcError"
    );
}

// ────────────────────────────
// getTransactions built-in check
// ────────────────────────────

/// Mock a `getTransactions` success returning the given `result` object.
fn mock_get_transactions_result(server: &mut mockito::Server, result: Value) {
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getTransactions""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": result
        })))
        .create();
}

/// The check resolves `startLedger` from `getLatestLedger` (number form)
/// and sends exactly `{"startLedger": <seq>, "pagination": {"limit": 2}}`.
/// The exact-body matcher proves the request shape.
#[tokio::test]
async fn test_get_transactions_request_params_from_latest_ledger() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTransactions",
            "params": {"startLedger": 5841158, "pagination": {"limit": 2}}
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "transactions": [
                    {"status": "SUCCESS", "txHash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984", "ledger": 5841158, "createdAt": "1763726400", "applicationOrder": 1, "feeCharged": "100", "operationCount": 1, "envelopeXdr": "AAAAAQ==", "resultXdr": "AAAAAAAAAAGQAAAAAAAAAA", "resultMetaXdr": "AAAAAA=="}
                ],
                "latestLedger": 5841158,
                "latestLedgerCloseTime": 1763726400,
                "oldestLedger": 5720199,
                "oldestLedgerCloseTime": 1763121600,
                "cursor": "5841158-0000000001"
            }
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getTransactions"), 1).await;

    let check = find_check(&report, "getTransactions");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("getTransactions details");
    assert_eq!(details.get("start_ledger"), Some(&Value::from(5841158u64)));
    assert_eq!(
        details.get("transactions_returned"),
        Some(&Value::from(1u64))
    );
    assert_eq!(details.get("latest_ledger"), Some(&Value::from(5841158u64)));
    assert_eq!(details.get("oldest_ledger"), Some(&Value::from(5720199u64)));
    assert_eq!(details.get("has_cursor"), Some(&Value::Bool(true)));
    // The filter leaves the other built-ins skipped, not omitted.
    assert_eq!(report.summary.passed, 2); // httpConnectivity + getTransactions
    assert_eq!(report.summary.skipped, 11);
}

/// An empty page is still a Pass: the method works, the latest ledger
/// simply holds no transactions. No cursor is returned for an empty page.
#[tokio::test]
async fn test_get_transactions_empty_page_passes() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!("5841158"));
    mock_get_transactions_result(
        &mut server,
        json!({
            "transactions": [],
            "latestLedger": 5841158,
            "oldestLedger": 5720199
        }),
    );

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getTransactions"), 1).await;

    let check = find_check(&report, "getTransactions");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("getTransactions details");
    assert_eq!(
        details.get("transactions_returned"),
        Some(&Value::from(0u64))
    );
    assert_eq!(details.get("has_cursor"), Some(&Value::Bool(false)));
}

/// When `getLatestLedger` fails, `getTransactions` cannot resolve a
/// startLedger and must Fail (not silently probe with a guessed ledger).
#[tokio::test]
async fn test_get_transactions_fails_when_latest_ledger_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

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
            "error": {"code": -32603, "message": "Internal error"}
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getTransactions"), 1).await;

    let check = find_check(&report, "getTransactions");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::JsonRpcError),
        "underlying getLatestLedger JSON-RPC error must surface"
    );
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("getLatestLedger"),
        "evidence must name the failed dependency: {evidence}"
    );
}

/// A `getTransactions` result without a `transactions` array is malformed,
/// not a Pass.
#[tokio::test]
async fn test_get_transactions_missing_transactions_array_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));
    mock_get_transactions_result(&mut server, json!({"latestLedger": 5841158}));

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getTransactions"), 1).await;

    let check = find_check(&report, "getTransactions");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::MalformedResponse),
        "missing 'transactions' array must classify as MalformedResponse"
    );
    let message = check.error_message.as_deref().unwrap_or("");
    assert!(
        message.contains("transactions"),
        "error message must name the missing field: {message}"
    );
}

/// A JSON-RPC error from `getTransactions` itself (e.g. invalid params) is
/// a Fail with the server's error classification preserved.
#[tokio::test]
async fn test_get_transactions_json_rpc_error_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getTransactions""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {"code": -32602, "message": "Invalid params"}
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getTransactions"), 1).await;

    let check = find_check(&report, "getTransactions");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::JsonRpcError),
        "getTransactions JSON-RPC error must classify as JsonRpcError"
    );
}

/// `--compare` runs the full built-in set on the second endpoint, so the
/// new `getTransactions` check must be present (and passing) in both
/// reports that `compare_with` sees — not only in the primary one.
#[tokio::test]
async fn test_compare_covers_get_transactions() {
    async fn mock_compare_server(sequence: u64) -> (mockito::ServerGuard, String) {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("{}")
            .create();
        mock_latest_ledger(&mut server, json!(sequence));
        mock_get_transactions_result(
            &mut server,
            json!({
                "transactions": [],
                "latestLedger": sequence,
                "oldestLedger": sequence - 100
            }),
        );
        (server, url)
    }

    let (_server_a, url_a) = mock_compare_server(5841158).await;
    let (_server_b, url_b) = mock_compare_server(5841158).await;

    // Same calls `--compare` makes: full built-in set, no method filter.
    let mut primary = DiagnosticReport::run_all(&url_a, Duration::from_secs(5), None, 1).await;
    let other = DiagnosticReport::run_all(&url_b, Duration::from_secs(5), None, 1).await;

    for report in [&primary, &other] {
        let check = find_check(report, "getTransactions");
        assert_eq!(
            check.status,
            CheckStatus::Pass,
            "getTransactions must run and pass on the compare endpoint too"
        );
    }
    assert_eq!(primary.checks.len(), 13);
    assert_eq!(other.checks.len(), 13);

    // Comparing two identical reports must keep getTransactions present
    // and passing, with no divergence flagged on it.
    primary.compare_with(&other);
    let check = find_check(&primary, "getTransactions");
    assert_eq!(check.status, CheckStatus::Pass);
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        !evidence.contains("divergence"),
        "identical endpoints must not flag divergence: {evidence}"
    );
}

// ────────────────────────────
// getFeeStats built-in check
// ────────────────────────────

/// Mock a `getFeeStats` success returning the given `result` object.
fn mock_get_fee_stats_result(server: &mut mockito::Server, result: Value) {
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getFeeStats""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": result
        })))
        .create();
}

/// Full fee response (shape verified live against
/// soroban-testnet.stellar.org): two fee objects plus latestLedger.
fn fee_stats_result() -> Value {
    json!({
        "sorobanInclusionFee": {
            "max": 600, "min": 100, "mode": 100,
            "p10": 100, "p20": 100, "p30": 100, "p40": 100,
            "p50": 100, "p60": 100, "p70": 100, "p80": 100,
            "p90": 100, "p95": 200, "p99": 500,
            "transactionCount": 575, "ledgerCount": 50
        },
        "inclusionFee": {
            "max": 300, "min": 100, "mode": 100,
            "p10": 100, "p20": 100, "p30": 100, "p40": 100,
            "p50": 100, "p60": 100, "p70": 100, "p80": 100,
            "p90": 100, "p95": 100, "p99": 200,
            "transactionCount": 153, "ledgerCount": 10
        },
        "latestLedger": 5933141
    })
}

#[tokio::test]
async fn test_get_fee_stats_success() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_get_fee_stats_result(&mut server, fee_stats_result());

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getFeeStats"), 1).await;

    let check = find_check(&report, "getFeeStats");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("getFeeStats details");
    assert_eq!(
        details.get("soroban_inclusion_fee_mode"),
        Some(&Value::from(100u64))
    );
    assert_eq!(
        details.get("soroban_inclusion_fee_p95"),
        Some(&Value::from(200u64))
    );
    assert_eq!(
        details.get("soroban_inclusion_fee_transaction_count"),
        Some(&Value::from(575u64))
    );
    assert_eq!(
        details.get("inclusion_fee_mode"),
        Some(&Value::from(100u64))
    );
    assert_eq!(details.get("inclusion_fee_p95"), Some(&Value::from(100u64)));
    assert_eq!(details.get("latest_ledger"), Some(&Value::from(5933141u64)));
    // The filter leaves the other built-ins skipped, not omitted.
    assert_eq!(report.summary.passed, 2); // httpConnectivity + getFeeStats
    assert_eq!(report.summary.skipped, 11);
}

/// A `getFeeStats` result without either fee object is malformed, not a
/// Pass: the fee objects are the substance of the response.
#[tokio::test]
async fn test_get_fee_stats_missing_fee_objects_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_get_fee_stats_result(&mut server, json!({"latestLedger": 5933141}));

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getFeeStats"), 1).await;

    let check = find_check(&report, "getFeeStats");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::MalformedResponse),
        "missing fee objects must classify as MalformedResponse"
    );
}

/// A JSON-RPC error from `getFeeStats` itself (e.g. `-32601` on an
/// endpoint that does not implement the method) is a Fail — like every
/// other built-in method check — not a Skip.
#[tokio::test]
async fn test_get_fee_stats_json_rpc_error_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getFeeStats""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {"code": -32601, "message": "Method not found"}
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getFeeStats"), 1).await;

    let check = find_check(&report, "getFeeStats");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::UnsupportedMethod),
        "getFeeStats -32601 must classify as UnsupportedMethod"
    );
}

/// `--compare` runs the full built-in set on the second endpoint, so the
/// new `getFeeStats` check must be present (and passing) in both reports.
#[tokio::test]
async fn test_compare_covers_get_fee_stats() {
    async fn mock_compare_server() -> (mockito::ServerGuard, String) {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("{}")
            .create();
        mock_latest_ledger(&mut server, json!(5841158));
        mock_get_fee_stats_result(&mut server, fee_stats_result());
        (server, url)
    }

    let (_server_a, url_a) = mock_compare_server().await;
    let (_server_b, url_b) = mock_compare_server().await;

    let mut primary = DiagnosticReport::run_all(&url_a, Duration::from_secs(5), None, 1).await;
    let other = DiagnosticReport::run_all(&url_b, Duration::from_secs(5), None, 1).await;

    for report in [&primary, &other] {
        let check = find_check(report, "getFeeStats");
        assert_eq!(
            check.status,
            CheckStatus::Pass,
            "getFeeStats must run and pass on the compare endpoint too"
        );
    }
    assert_eq!(primary.checks.len(), 13);
    assert_eq!(other.checks.len(), 13);

    primary.compare_with(&other);
    let check = find_check(&primary, "getFeeStats");
    assert_eq!(check.status, CheckStatus::Pass);
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        !evidence.contains("divergence"),
        "identical endpoints must not flag divergence: {evidence}"
    );
}

// ────────────────────────────
// getTransaction (singular) built-in check
// ────────────────────────────

/// Mock a `getTransaction` success returning the given `result` object.
fn mock_get_transaction_result(server: &mut mockito::Server, result: Value) {
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getTransaction""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": result
        })))
        .create();
}

/// The check resolves `startLedger` from `getLatestLedger`, takes the
/// first hash from a one-page `getTransactions` result, and sends exactly
/// `{"hash": <that hash>}`. The exact-body matchers prove the request
/// shape, including that the hash comes from the page, not from input.
#[tokio::test]
async fn test_get_transaction_request_params_from_latest_ledger() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTransactions",
            "params": {"startLedger": 5841158, "pagination": {"limit": 2}}
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "transactions": [
                    {"status": "SUCCESS", "txHash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984", "ledger": 5841158, "createdAt": "1763726400", "applicationOrder": 1, "feeCharged": "100", "operationCount": 1, "envelopeXdr": "AAAAAQ==", "resultXdr": "AAAAAAAAAAGQAAAAAAAAAA", "resultMetaXdr": "AAAAAA=="}
                ],
                "latestLedger": 5841158,
                "oldestLedger": 5720199,
                "cursor": "5841158-0000000001"
            }
        })))
        .create();

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTransaction",
            "params": {"hash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984"}
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "SUCCESS",
                "txHash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984",
                "ledger": 5841158,
                "createdAt": "1763726400",
                "applicationOrder": 1,
                "feeCharged": "100",
                "operationCount": 1,
                "envelopeXdr": "AAAAAQ==",
                "resultXdr": "AAAAAAAAAAGQAAAAAAAAAA",
                "resultMetaXdr": "AAAAAA=="
            }
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getTransaction"), 1).await;

    let check = find_check(&report, "getTransaction");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("getTransaction details");
    assert_eq!(
        details.get("tx_hash"),
        Some(&Value::String(
            "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984".to_string()
        ))
    );
    assert_eq!(
        details.get("status"),
        Some(&Value::String("SUCCESS".to_string()))
    );
    assert_eq!(details.get("ledger"), Some(&Value::from(5841158u64)));
    assert_eq!(
        details.get("fee_charged"),
        Some(&Value::String("100".to_string()))
    );
    assert_eq!(details.get("has_envelope_xdr"), Some(&Value::Bool(true)));
    assert_eq!(details.get("has_result_xdr"), Some(&Value::Bool(true)));
    assert_eq!(details.get("has_result_meta_xdr"), Some(&Value::Bool(true)));
    // The filter leaves the other built-ins skipped, not omitted.
    assert_eq!(report.summary.passed, 2); // httpConnectivity + getTransaction
    assert_eq!(report.summary.skipped, 11);
}

/// An empty page is a Skip, not a Fail: there is no transaction to fetch,
/// so the method was never exercised.
#[tokio::test]
async fn test_get_transaction_empty_page_skips() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));
    mock_get_transactions_result(
        &mut server,
        json!({
            "transactions": [],
            "latestLedger": 5841158,
            "oldestLedger": 5720199
        }),
    );

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getTransaction"), 1).await;

    let check = find_check(&report, "getTransaction");
    assert_eq!(check.status, CheckStatus::Skip);
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("no transactions"),
        "skip evidence must say why there is nothing to fetch: {evidence}"
    );
    assert_eq!(report.summary.passed, 1); // httpConnectivity only
    assert_eq!(report.summary.skipped, 12);
}

/// Older RPC versions return the page hash as `hash` instead of
/// `txHash`; the check accepts both, preferring the live field.
#[tokio::test]
async fn test_get_transaction_accepts_legacy_hash_field() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));
    mock_get_transactions_result(
        &mut server,
        json!({
            "transactions": [
                {"status": "SUCCESS", "hash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984", "ledger": 5841158}
            ],
            "latestLedger": 5841158
        }),
    );
    mock_get_transaction_result(
        &mut server,
        json!({
            "status": "SUCCESS",
            "txHash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984",
            "ledger": 5841158,
            "feeCharged": "100",
            "envelopeXdr": "AAAAAQ==",
            "resultXdr": "AAAAAAAAAAGQAAAAAAAAAA",
            "resultMetaXdr": "AAAAAA=="
        }),
    );

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getTransaction"), 1).await;

    let check = find_check(&report, "getTransaction");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("getTransaction details");
    assert_eq!(
        details.get("tx_hash"),
        Some(&Value::String(
            "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984".to_string()
        ))
    );
}

/// When `getLatestLedger` fails, `getTransaction` cannot resolve a
/// startLedger and must Fail (not silently probe with a guessed ledger).
#[tokio::test]
async fn test_get_transaction_fails_when_latest_ledger_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

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
            "error": {"code": -32603, "message": "Internal error"}
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getTransaction"), 1).await;

    let check = find_check(&report, "getTransaction");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::JsonRpcError),
        "underlying getLatestLedger JSON-RPC error must surface"
    );
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("getLatestLedger"),
        "evidence must name the failed dependency: {evidence}"
    );
}

/// When the `getTransactions` page call fails, `getTransaction` cannot
/// resolve a hash and must Fail.
#[tokio::test]
async fn test_get_transaction_fails_when_page_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getTransactions""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {"code": -32603, "message": "Internal error"}
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getTransaction"), 1).await;

    let check = find_check(&report, "getTransaction");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::JsonRpcError),
        "underlying getTransactions JSON-RPC error must surface"
    );
}

/// A `getTransaction` result without a `status` string is malformed,
/// not a Pass.
#[tokio::test]
async fn test_get_transaction_missing_status_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));
    mock_get_transactions_result(
        &mut server,
        json!({
            "transactions": [
                {"status": "SUCCESS", "txHash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984", "ledger": 5841158}
            ],
            "latestLedger": 5841158
        }),
    );
    mock_get_transaction_result(&mut server, json!({"ledger": 5841158}));

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getTransaction"), 1).await;

    let check = find_check(&report, "getTransaction");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::MalformedResponse),
        "missing 'status' must classify as MalformedResponse"
    );
    let message = check.error_message.as_deref().unwrap_or("");
    assert!(
        message.contains("status"),
        "error message must name the missing field: {message}"
    );
}

/// A JSON-RPC error from `getTransaction` itself (e.g. invalid params)
/// is a Fail with the server's error classification preserved.
#[tokio::test]
async fn test_get_transaction_json_rpc_error_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(5841158));
    mock_get_transactions_result(
        &mut server,
        json!({
            "transactions": [
                {"status": "SUCCESS", "txHash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984", "ledger": 5841158}
            ],
            "latestLedger": 5841158
        }),
    );
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"getTransaction""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {"code": -32602, "message": "Invalid params"}
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getTransaction"), 1).await;

    let check = find_check(&report, "getTransaction");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::JsonRpcError),
        "getTransaction JSON-RPC error must classify as JsonRpcError"
    );
}

/// `--compare` runs the full built-in set on the second endpoint, so the
/// new `getTransaction` check must be present (and passing) in both
/// reports that `compare_with` sees — not only in the primary one.
#[tokio::test]
async fn test_compare_covers_get_transaction() {
    async fn mock_compare_server(sequence: u64) -> (mockito::ServerGuard, String) {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("{}")
            .create();
        mock_latest_ledger(&mut server, json!(sequence));
        mock_get_transactions_result(
            &mut server,
            json!({
                "transactions": [
                    {"status": "SUCCESS", "txHash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984", "ledger": sequence}
                ],
                "latestLedger": sequence,
                "oldestLedger": sequence - 100
            }),
        );
        mock_get_transaction_result(
            &mut server,
            json!({
                "status": "SUCCESS",
                "txHash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984",
                "ledger": sequence,
                "feeCharged": "100",
                "envelopeXdr": "AAAAAQ==",
                "resultXdr": "AAAAAAAAAAGQAAAAAAAAAA",
                "resultMetaXdr": "AAAAAA=="
            }),
        );
        (server, url)
    }

    let (_server_a, url_a) = mock_compare_server(5841158).await;
    let (_server_b, url_b) = mock_compare_server(5841158).await;

    // Same calls `--compare` makes: full built-in set, no method filter.
    let mut primary = DiagnosticReport::run_all(&url_a, Duration::from_secs(5), None, 1).await;
    let other = DiagnosticReport::run_all(&url_b, Duration::from_secs(5), None, 1).await;

    for report in [&primary, &other] {
        let check = find_check(report, "getTransaction");
        assert_eq!(
            check.status,
            CheckStatus::Pass,
            "getTransaction must run and pass on the compare endpoint too"
        );
    }
    assert_eq!(primary.checks.len(), 13);
    assert_eq!(other.checks.len(), 13);

    // Comparing two identical reports must keep getTransaction present
    // and passing, with no divergence flagged on it.
    primary.compare_with(&other);
    let check = find_check(&primary, "getTransaction");
    assert_eq!(check.status, CheckStatus::Pass);
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        !evidence.contains("divergence"),
        "identical endpoints must not flag divergence: {evidence}"
    );
}

// ────────────────────────────
// simulateTransaction built-in check
// ────────────────────────────

/// Mock a `getNetwork` success returning the given passphrase.
fn mock_get_network_passphrase(server: &mut mockito::Server, passphrase: &str) {
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
                "friendbotUrl": "https://friendbot.stellar.org",
                "passphrase": passphrase,
                "protocolVersion": 23
            }
        })))
        .create();
}

/// Mock a `simulateTransaction` success returning the given `result`.
fn mock_simulate_transaction_result(server: &mut mockito::Server, result: Value) {
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"simulateTransaction""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": result
        })))
        .create();
}

/// A successful simulation result (shape verified live against
/// soroban-testnet.stellar.org for the embedded probe transaction).
fn simulate_success_result() -> Value {
    json!({
        "latestLedger": 5933150,
        "minResourceFee": "31052",
        "cost": {"cpuInsns": "1036915", "memBytes": "51240"},
        "results": [{"auth": [], "retval": "AAAAAQAAAAA="}],
        "transactionData": "AAAAAQ==",
        "events": [],
        "stateChanges": []
    })
}

/// On Testnet the check replays the embedded probe envelope verbatim
/// (exact-body matcher pins the envelope bytes) and reports the
/// simulation result.
#[tokio::test]
async fn test_simulate_transaction_success_testnet() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_get_network_passphrase(&mut server, "Test SDF Network ; September 2015");

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "simulateTransaction",
            "params": {"transaction": miyo_rpc::diagnostics::SIMULATE_TRANSACTION_TESTNET_PROBE_ENVELOPE}
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": simulate_success_result()
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("simulateTransaction"), 1)
            .await;

    let check = find_check(&report, "simulateTransaction");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("simulateTransaction details");
    assert_eq!(
        details.get("tx_hash"),
        Some(&Value::String(
            "bbcc753be258d79ad3caa1c66ae150116ab1bc91103b98f7d52a2a4481c38d95".to_string()
        ))
    );
    assert_eq!(
        details.get("min_resource_fee"),
        Some(&Value::from(31052u64))
    );
    assert_eq!(details.get("results_count"), Some(&Value::from(1u64)));
    assert_eq!(details.get("latest_ledger"), Some(&Value::from(5933150u64)));
    assert_eq!(
        details.get("cost_cpu_insns"),
        Some(&Value::from(1036915u64))
    );
    assert_eq!(details.get("cost_mem_bytes"), Some(&Value::from(51240u64)));
    // The filter leaves the other built-ins skipped, not omitted.
    assert_eq!(report.summary.passed, 2); // httpConnectivity + simulateTransaction
    assert_eq!(report.summary.skipped, 11);
}

/// The probe transaction only exists on Testnet: on Mainnet the check
/// Skips with evidence naming the limitation, instead of failing an
/// otherwise healthy endpoint.
#[tokio::test]
async fn test_simulate_transaction_skips_on_mainnet() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_get_network_passphrase(
        &mut server,
        "Public Global Stellar Network ; September 2015",
    );

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("simulateTransaction"), 1)
            .await;

    let check = find_check(&report, "simulateTransaction");
    assert_eq!(check.status, CheckStatus::Skip);
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("mainnet"),
        "skip evidence must name the detected network: {evidence}"
    );
    assert!(
        evidence.contains("Testnet"),
        "skip evidence must say the probe is Testnet-specific: {evidence}"
    );
    assert_eq!(report.summary.passed, 1); // httpConnectivity only
    assert_eq!(report.summary.skipped, 12);
}

/// Same Skip as Mainnet, for Futurenet.
#[tokio::test]
async fn test_simulate_transaction_skips_on_futurenet() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_get_network_passphrase(&mut server, "Test SDF Future Network ; October 2022");

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("simulateTransaction"), 1)
            .await;

    let check = find_check(&report, "simulateTransaction");
    assert_eq!(check.status, CheckStatus::Skip);
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("futurenet"),
        "skip evidence must name the detected network: {evidence}"
    );
}

/// When `getNetwork` fails, the check cannot determine the network and
/// must Fail (not guess Testnet and simulate anyway).
#[tokio::test]
async fn test_simulate_transaction_fails_when_get_network_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

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
            "error": {"code": -32603, "message": "Internal error"}
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("simulateTransaction"), 1)
            .await;

    let check = find_check(&report, "simulateTransaction");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::JsonRpcError),
        "underlying getNetwork JSON-RPC error must surface"
    );
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("getNetwork"),
        "evidence must name the failed dependency: {evidence}"
    );
}

/// A result-level `error` means the probe invocation itself failed to
/// simulate (e.g. the probe contract is gone): the method answered, but
/// the check cannot prove it simulates a real invocation.
#[tokio::test]
async fn test_simulate_transaction_result_error_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_get_network_passphrase(&mut server, "Test SDF Network ; September 2015");
    mock_simulate_transaction_result(
        &mut server,
        json!({
            "latestLedger": 5933150,
            "error": "transaction contains unsupported operation type: OperationTypeManageData"
        }),
    );

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("simulateTransaction"), 1)
            .await;

    let check = find_check(&report, "simulateTransaction");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::UnsupportedMethod),
        "a simulation error must classify as UnsupportedMethod"
    );
    let message = check.error_message.as_deref().unwrap_or("");
    assert!(
        message.contains("OperationTypeManageData"),
        "error message must carry the server's simulation error: {message}"
    );
}

/// A `simulateTransaction` result with neither `results` nor `error`
/// is malformed, not a Pass.
#[tokio::test]
async fn test_simulate_transaction_missing_results_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_get_network_passphrase(&mut server, "Test SDF Network ; September 2015");
    mock_simulate_transaction_result(&mut server, json!({"latestLedger": 5933150}));

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("simulateTransaction"), 1)
            .await;

    let check = find_check(&report, "simulateTransaction");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::MalformedResponse),
        "missing 'results' must classify as MalformedResponse"
    );
}

/// A JSON-RPC error from `simulateTransaction` itself (e.g. `-32601` on
/// an endpoint that does not implement the method) is a Fail with the
/// server's error classification preserved.
#[tokio::test]
async fn test_simulate_transaction_json_rpc_error_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_get_network_passphrase(&mut server, "Test SDF Network ; September 2015");
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(
            r#""method":"simulateTransaction""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {"code": -32601, "message": "Method not found"}
        })))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("simulateTransaction"), 1)
            .await;

    let check = find_check(&report, "simulateTransaction");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::UnsupportedMethod),
        "simulateTransaction -32601 must classify as UnsupportedMethod"
    );
}

/// `--compare` runs the full built-in set on the second endpoint, so the
/// new `simulateTransaction` check must be present (and passing) in both
/// reports that `compare_with` sees — not only in the primary one.
#[tokio::test]
async fn test_compare_covers_simulate_transaction() {
    async fn mock_compare_server() -> (mockito::ServerGuard, String) {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("{}")
            .create();
        mock_get_network_passphrase(&mut server, "Test SDF Network ; September 2015");
        mock_latest_ledger(&mut server, json!(5933150));
        mock_simulate_transaction_result(&mut server, simulate_success_result());
        (server, url)
    }

    let (_server_a, url_a) = mock_compare_server().await;
    let (_server_b, url_b) = mock_compare_server().await;

    // Same calls `--compare` makes: full built-in set, no method filter.
    let mut primary = DiagnosticReport::run_all(&url_a, Duration::from_secs(5), None, 1).await;
    let other = DiagnosticReport::run_all(&url_b, Duration::from_secs(5), None, 1).await;

    for report in [&primary, &other] {
        let check = find_check(report, "simulateTransaction");
        assert_eq!(
            check.status,
            CheckStatus::Pass,
            "simulateTransaction must run and pass on the compare endpoint too"
        );
    }
    assert_eq!(primary.checks.len(), 13);
    assert_eq!(other.checks.len(), 13);

    // Comparing two identical reports must keep simulateTransaction
    // present and passing, with no divergence flagged on it.
    primary.compare_with(&other);
    let check = find_check(&primary, "simulateTransaction");
    assert_eq!(check.status, CheckStatus::Pass);
    let evidence = check.evidence.as_deref().unwrap_or("");
    assert!(
        !evidence.contains("divergence"),
        "identical endpoints must not flag divergence: {evidence}"
    );
}

// ────────────────────────────
// HTTP 429 rate-limit classification
// ────────────────────────────

/// A 429 response must classify as `RateLimited`, not `ClientError`.
#[tokio::test]
async fn test_http_429_classified_rate_limited() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("POST", "/")
        .with_status(429)
        .with_header("content-type", "text/plain")
        .with_body("Too Many Requests")
        .create_async()
        .await;

    let config = RpcClientConfig::new(Duration::from_secs(5));
    let client = RpcClient::new(&url, config).expect("should create client");

    let err = client
        .call::<Value>("getHealth", None)
        .await
        .expect_err("429 must fail");
    assert_eq!(err.category(), miyo_rpc::ErrorCategory::RateLimited);
}

/// A 429 with an integer `Retry-After` surfaces the raw value and its seconds.
#[tokio::test]
async fn test_http_429_retry_after_integer() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("POST", "/")
        .with_status(429)
        .with_header("retry-after", "30")
        .with_header("content-type", "application/json")
        .with_body("{\"error\":\"slow down\"}")
        .create_async()
        .await;

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getHealth"), 1).await;
    let check = report
        .checks
        .iter()
        .find(|c| c.name == "getHealth")
        .expect("getHealth check present");

    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(check.error, Some(miyo_rpc::IssueCategory::RateLimited));
    let details = check.details.as_ref().expect("details present");
    assert_eq!(details.get("retry_after"), Some(&json!("30")));
    assert_eq!(details.get("retry_after_secs"), Some(&json!(30)));
}

/// An HTTP-date `Retry-After` is preserved verbatim without panicking and
/// without inventing a seconds value.
#[tokio::test]
async fn test_http_429_retry_after_http_date() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    let date = "Wed, 21 Oct 2026 07:28:00 GMT";
    server
        .mock("POST", "/")
        .with_status(429)
        .with_header("retry-after", date)
        .with_header("content-type", "application/json")
        .with_body("{\"error\":\"slow down\"}")
        .create_async()
        .await;

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getHealth"), 1).await;
    let check = report
        .checks
        .iter()
        .find(|c| c.name == "getHealth")
        .expect("getHealth check present");

    assert_eq!(check.error, Some(miyo_rpc::IssueCategory::RateLimited));
    let details = check.details.as_ref().expect("details present");
    assert_eq!(details.get("retry_after"), Some(&json!(date)));
    assert!(
        details.get("retry_after_secs").is_none(),
        "non-numeric Retry-After must not produce retry_after_secs"
    );
}

/// A 429 without `Retry-After` still classifies correctly and adds no
/// retry detail keys — only the error shape.
#[tokio::test]
async fn test_http_429_without_retry_after() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("POST", "/")
        .with_status(429)
        .with_header("content-type", "application/json")
        .with_body("{\"error\":\"slow down\"}")
        .create_async()
        .await;

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getHealth"), 1).await;
    let check = report
        .checks
        .iter()
        .find(|c| c.name == "getHealth")
        .expect("getHealth check present");

    assert_eq!(check.error, Some(miyo_rpc::IssueCategory::RateLimited));
    let details = check.details.as_ref().expect("details present");
    assert_eq!(details.get("error_shape"), Some(&json!("rate_limited")));
    assert!(
        details.get("retry_after").is_none(),
        "no retry_after must be added when the header is absent"
    );
    assert!(
        details.get("retry_after_secs").is_none(),
        "no retry_after_secs must be added when the header is absent"
    );
}

/// JSON output must expose `rate_limited` and the retry fields.
#[tokio::test]
async fn test_http_429_json_output_fields() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("POST", "/")
        .with_status(429)
        .with_header("retry-after", "12")
        .with_header("content-type", "application/json")
        .with_body("{\"error\":\"slow down\"}")
        .create_async()
        .await;

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getHealth"), 1).await;
    let json_out = miyo_rpc::format_json(&report).expect("json serializes");
    let parsed: Value = serde_json::from_str(&json_out).expect("valid JSON");

    let check = parsed["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|c| c["name"] == "getHealth")
        .expect("getHealth in JSON");

    // NOTE: `format_json` JSON-encodes the IssueCategory into a string, so the
    // value is a quoted token (pre-existing behavior, unchanged here).
    assert_eq!(
        check["error"].as_str().unwrap().trim_matches('"'),
        "rate_limited"
    );
    assert_eq!(check["details"]["retry_after"], json!("12"));
    assert_eq!(check["details"]["retry_after_secs"], json!(12));
}

/// Batch diagnostics inherit the same classification per endpoint.
#[tokio::test]
async fn test_batch_429_classification_per_endpoint() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();
    server
        .mock("POST", "/")
        .with_status(429)
        .with_header("retry-after", "7")
        .with_header("content-type", "application/json")
        .with_body("{\"error\":\"slow down\"}")
        .create_async()
        .await;

    let dir = std::env::temp_dir().join(format!("miyo-batch-429-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join("endpoints.txt");
    std::fs::write(&file, format!("{base}/\n")).expect("write endpoints");

    let args = BatchArgs {
        file: file.to_string_lossy().to_string(),
        output: OutputFormat::Json,
        timeout_secs: 5,
        runs: 1,
        method: Some("getHealth".to_string()),
        concurrency: 1,
        expected_network: None,
    };

    let report = miyo_rpc::run_batch(&args).await;
    let entry = report.endpoints.first().expect("one entry");
    let value = entry.report.as_ref().expect("report present");
    let check = value["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|c| c["name"] == "getHealth")
        .expect("getHealth in batch JSON");

    assert_eq!(
        check["error"].as_str().unwrap().trim_matches('"'),
        "rate_limited"
    );
    assert_eq!(check["details"]["retry_after"], json!("7"));
    assert_eq!(check["details"]["retry_after_secs"], json!(7));

    let _ = std::fs::remove_dir_all(&dir);
}

// ────────────────────────────────
// Batch per-endpoint timeout (#15)
// ────────────────────────────────

/// A server that accepts connections but never responds. The batch timeout
/// must bound each endpoint's diagnosis: with `--timeout-secs 1` the run
/// finishes after one timeout per check instead of hanging, and the failing
/// checks report the configured timeout.
#[tokio::test]
async fn test_batch_timeout_applies_per_endpoint() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    // Hold every accepted connection open forever without responding.
    let server_task = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
        }
    });

    let mut path = std::env::temp_dir();
    path.push("miyo_batch_timeout.txt");
    std::fs::write(&path, format!("{url}\n")).unwrap();

    let args = BatchArgs {
        file: path.to_string_lossy().to_string(),
        output: OutputFormat::Human,
        timeout_secs: 1,
        runs: 1,
        method: Some("getHealth".to_string()),
        concurrency: 1,
        expected_network: None,
    };
    let started = std::time::Instant::now();
    let report = miyo_rpc::run_batch(&args).await;
    let elapsed = started.elapsed();
    server_task.abort();
    let _ = std::fs::remove_file(&path);

    // Without the timeout reaching the client, reqwest has no deadline and
    // this run would hang until the test harness kills it.
    assert!(
        elapsed < std::time::Duration::from_secs(15),
        "batch run must be bounded by the timeout, took {elapsed:?}"
    );
    assert_eq!(report.summary.total, 1);
    assert_eq!(report.summary.valid, 1);
    assert_eq!(report.summary.failed, 1);
    assert_eq!(report.summary.exit_code, 1);
    let json = miyo_rpc::format_batch_json(&report).unwrap();
    assert!(
        json.contains("timed out after 1s"),
        "failing checks must report the 1s timeout, got: {json}"
    );
}

// ────────────────────────────
// Provider error taxonomy (#35)
// ────────────────────────────

/// The `error_shape` recorded in a failed check's `details`, if any.
fn error_shape_of(report: &DiagnosticReport, name: &str) -> Option<String> {
    find_check(report, name)
        .details
        .as_ref()
        .and_then(|d| d.get("error_shape"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Mock any JSON-RPC call for `method` to fail with the given error code.
/// Also answers the plain-GET connectivity probe so httpConnectivity
/// passes and the report counts only the method failure.
fn mock_json_rpc_error(server: &mut mockito::Server, method: &str, code: i32) {
    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Regex(format!(r#""method":"{method}""#)))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": code, "message": "mocked provider error" }
        })))
        .create();
}

/// Mock any JSON-RPC call to fail with the given HTTP status and body.
/// Also answers the plain-GET connectivity probe so httpConnectivity
/// passes and the report counts only the method failure.
fn mock_http_error(server: &mut mockito::Server, status: usize, body: &str) {
    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();
    server
        .mock("POST", "/")
        .with_status(status)
        .with_header("content-type", "text/html")
        .with_body(body)
        .create();
}

#[tokio::test]
async fn test_error_shape_method_not_found() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    mock_json_rpc_error(&mut server, "getNetwork", -32601);

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getNetwork"), 1).await;

    let check = find_check(&report, "getNetwork");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        error_shape_of(&report, "getNetwork").as_deref(),
        Some("method_not_found")
    );
    // The coarse category is unchanged by the shape.
    assert_eq!(
        check.error,
        Some(miyo_rpc::IssueCategory::UnsupportedMethod)
    );
    assert_eq!(report.summary.failed, 1);
}

#[tokio::test]
async fn test_error_shape_invalid_params_and_internal_error() {
    for (code, shape) in [(-32602, "invalid_params"), (-32603, "internal_error")] {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        mock_json_rpc_error(&mut server, "getNetwork", code);

        let report =
            DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getNetwork"), 1).await;

        assert_eq!(
            error_shape_of(&report, "getNetwork").as_deref(),
            Some(shape),
            "code {code} must map to {shape}"
        );
    }
}

#[tokio::test]
async fn test_error_shape_unknown_json_rpc_code() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    mock_json_rpc_error(&mut server, "getNetwork", -32000);

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getNetwork"), 1).await;

    assert_eq!(
        error_shape_of(&report, "getNetwork").as_deref(),
        Some("json_rpc_error")
    );
}

#[tokio::test]
async fn test_error_shape_rate_limited_keeps_retry_after() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();
    server
        .mock("POST", "/")
        .with_status(429)
        .with_header("content-type", "application/json")
        .with_header("Retry-After", "120")
        .with_body("{\"error\": \"slow down\"}")
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getNetwork"), 1).await;

    assert_eq!(
        error_shape_of(&report, "getNetwork").as_deref(),
        Some("rate_limited")
    );
    let details = find_check(&report, "getNetwork")
        .details
        .as_ref()
        .expect("failure details");
    assert_eq!(
        details.get("retry_after"),
        Some(&Value::String("120".to_string()))
    );
    assert_eq!(details.get("retry_after_secs"), Some(&Value::from(120u64)));
}

#[tokio::test]
async fn test_error_shape_html_error_page() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    mock_http_error(
        &mut server,
        503,
        "<!DOCTYPE html><html><head><title>Error</title></head><body>blocked</body></html>",
    );

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getNetwork"), 1).await;

    assert_eq!(
        error_shape_of(&report, "getNetwork").as_deref(),
        Some("html_error_page")
    );
}

#[tokio::test]
async fn test_error_shape_http_server_and_client_errors() {
    for (status, shape) in [
        (500, "http_server_error"),
        (502, "http_server_error"),
        (404, "http_client_error"),
        (403, "http_client_error"),
    ] {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        mock_http_error(&mut server, status, "{\"error\": \"provider failure\"}");

        let report =
            DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getNetwork"), 1).await;

        assert_eq!(
            error_shape_of(&report, "getNetwork").as_deref(),
            Some(shape),
            "HTTP {status} must map to {shape}"
        );
    }
}

#[tokio::test]
async fn test_error_shape_non_json_response() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "text/plain")
        .with_body("this is not json at all")
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getNetwork"), 1).await;

    assert_eq!(
        error_shape_of(&report, "getNetwork").as_deref(),
        Some("non_json_response")
    );
}

#[tokio::test]
async fn test_error_shape_malformed_response() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({"jsonrpc": "2.0", "id": 1})))
        .create();

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("getNetwork"), 1).await;

    assert_eq!(
        error_shape_of(&report, "getNetwork").as_deref(),
        Some("malformed_response")
    );
}

#[tokio::test]
async fn test_error_shape_oversized_response() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();
    // A valid getNetwork response well over the 100-byte cap below.
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "friendbotUrl": "https://friendbot.stellar.org",
                "passphrase": "Test SDF Network ; September 2015",
                "protocolVersion": 23
            }
        })))
        .create();

    let config = RpcClientConfig::new(Duration::from_secs(5)).with_max_response_bytes(100);
    let report = DiagnosticReport::run_all_with_config(&url, config, Some("getNetwork"), 1).await;

    assert_eq!(
        error_shape_of(&report, "getNetwork").as_deref(),
        Some("oversized_response")
    );
}

#[tokio::test]
async fn test_error_shape_transport_error() {
    let report = DiagnosticReport::run_all(
        "http://127.0.0.1:1",
        Duration::from_secs(5),
        Some("getNetwork"),
        1,
    )
    .await;

    assert_eq!(
        error_shape_of(&report, "getNetwork").as_deref(),
        Some("transport_error")
    );
}

#[tokio::test]
async fn test_error_shape_configuration_error() {
    let report =
        DiagnosticReport::run_all("ftp://example.com/rpc", Duration::from_millis(100), None, 3)
            .await;

    assert_eq!(report.runs, 1);
    // The 11 RPC method checks fail at client build with the InvalidUrl
    // error; httpConnectivity does its own plain GET, which fails at
    // transport level instead.
    for check in &report.checks {
        assert_eq!(check.status, CheckStatus::Fail);
        let expected = if check.name == "httpConnectivity" {
            "transport_error"
        } else {
            "configuration_error"
        };
        assert_eq!(
            error_shape_of(&report, &check.name).as_deref(),
            Some(expected),
            "check {} must carry the {expected} shape",
            check.name
        );
    }
}

#[tokio::test]
async fn test_error_shape_unsupported_method_simulation() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_get_network_passphrase(&mut server, "Test SDF Network ; September 2015");
    mock_simulate_transaction_result(
        &mut server,
        json!({"error": "simulation failed: host function not found"}),
    );

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("simulateTransaction"), 1)
            .await;

    let check = find_check(&report, "simulateTransaction");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        error_shape_of(&report, "simulateTransaction").as_deref(),
        Some("unsupported_method")
    );
}

// ────────────────────────────
// contractTtl built-in check
// ────────────────────────────

/// Real Testnet contracts, verified live 2026-11-27 via `getLedgerEntries`:
/// a Wasm contract (with a code entry) and the native XLM Stellar Asset
/// Contract (no Wasm, so no code entry by design).
const TTL_WASM_CONTRACT: &str = "CBSOWXRDBW4BZ2WYGQWJN25U24BDXOTLAS4U7NURAMDTTBGB72CNSP5Y";
const TTL_SAC_CONTRACT: &str = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC";

/// Base64 XDR ledger keys for the contracts above, computed independently
/// (Python, from the XDR spec) — not via the implementation under test.
/// The SAC instance key is the exact value soroban-testnet.stellar.org
/// echoed back for that contract's instance entry.
const TTL_WASM_INSTANCE_KEY: &str =
    "AAAABgAAAAFk614jDbgc6tg0LJbrtNcCO7prBLlPtpEDBzmEwf6E2QAAABQAAAAB";
const TTL_WASM_CODE_KEY: &str = "AAAAB+Mro/P9SpcyOhrlGxDGwDJq/thQqX1rgRpQ6NIOZ5yp";
const TTL_SAC_INSTANCE_KEY: &str =
    "AAAABgAAAAHXkotywnA8z+r365/0701QSlWouXn8m0UOoshCtNHOYQAAABQAAAAB";

/// Base64 `LedgerEntryData` XDR for the contracts' instance entries, built
/// from the documented layout (discriminant 6, empty ext, CONTRACT address,
/// LEDGER_KEY_CONTRACT_INSTANCE, PERSISTENT, CONTRACT_INSTANCE, then the
/// executable: WASM + hash, or STELLAR_ASSET).
const TTL_WASM_INSTANCE_XDR: &str =
    "AAAABgAAAAAAAAABZOteIw24HOrYNCyW67TXAju6awS5T7aRAwc5hMH+hNkAAAAUAAAAAQAAABMAAAAA4yuj8/1KlzI6GuUbEMbAMmr+2FCpfWuBGlDo0g5nnKk=";
const TTL_SAC_INSTANCE_XDR: &str =
    "AAAABgAAAAAAAAAB15KLcsJwPM/q9+uf9O9NUEpVqLl5/JtFDqLIQrTRzmEAAAAUAAAAAQAAABMAAAAB";

/// The Wasm hash of the WASM contract above (from its instance entry,
/// verified live 2026-11-27).
const TTL_WASM_HASH_HEX: &str = "e32ba3f3fd4a97323a1ae51b10c6c0326afed850a97d6b811a50e8d20e679ca9";

/// Latest ledger used by all TTL fixtures.
const TTL_LATEST_LEDGER: u64 = 5_940_443;

/// Mock a `getLedgerEntries` call for exactly one ledger key, returning the
/// given entries. The exact-body matcher proves the check sends the key
/// the spec requires — a wrong key construction fails the mock.
fn mock_get_ledger_entries_key(
    server: &mut mockito::Server,
    key: &str,
    entries: Value,
) -> mockito::Mock {
    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLedgerEntries",
            "params": {"keys": [key]}
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"entries": entries, "latestLedger": TTL_LATEST_LEDGER}
        })))
        .create()
}

/// One `getLedgerEntries` entry: the key the RPC echoed, the entry's XDR,
/// and its expiry ledger. (The check does not parse a code entry's XDR,
/// so code fixtures use a placeholder value.)
fn ledger_entry(key: &str, xdr: &str, live_until: u64) -> Value {
    json!({
        "key": key,
        "xdr": xdr,
        "lastModifiedLedgerSeq": 5_940_000,
        "liveUntilLedgerSeq": live_until,
    })
}

/// Run only the `contractTtl` check against the mock server, for the given
/// contract strkey and threshold.
async fn run_contract_ttl(url: &str, contract: &str, threshold: u64) -> DiagnosticReport {
    let contract_id = miyo_rpc::decode_contract_id(contract).expect("test contract strkey");
    DiagnosticReport::run_all_with_config_and_contract(
        url,
        RpcClientConfig::new(Duration::from_secs(5)),
        Some("contractTtl"),
        1,
        Some(TtlCheckConfig {
            contract_id,
            threshold,
        }),
        None,
    )
    .await
}

/// The Wasm contract's live values (2026-11-27): instance live 6404639
/// (464196 remaining), code live 6402384 (461941 remaining) at latest
/// 5940443 — both far above the 17280 default threshold, so Pass.
#[tokio::test]
async fn test_contract_ttl_pass_wasm() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(TTL_LATEST_LEDGER));
    let instance_mock = mock_get_ledger_entries_key(
        &mut server,
        TTL_WASM_INSTANCE_KEY,
        json!([ledger_entry(
            TTL_WASM_INSTANCE_KEY,
            TTL_WASM_INSTANCE_XDR,
            6_404_639
        )]),
    );
    let code_mock = mock_get_ledger_entries_key(
        &mut server,
        TTL_WASM_CODE_KEY,
        json!([ledger_entry(TTL_WASM_CODE_KEY, "AAAAAA==", 6_402_384)]),
    );

    let report = run_contract_ttl(&url, TTL_WASM_CONTRACT, 17_280).await;

    let check = find_check(&report, "contractTtl");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("contractTtl details");
    assert_eq!(
        details.get("instance_remaining_ledgers"),
        Some(&Value::from(464_196u64))
    );
    assert_eq!(
        details.get("code_remaining_ledgers"),
        Some(&Value::from(461_941u64))
    );
    assert_eq!(
        details.get("latest_ledger"),
        Some(&Value::from(TTL_LATEST_LEDGER))
    );
    assert_eq!(
        details.get("code_executable"),
        Some(&Value::String("wasm".to_string()))
    );
    assert_eq!(
        details.get("code_wasm_hash"),
        Some(&Value::String(TTL_WASM_HASH_HEX.to_string()))
    );
    assert_eq!(details.get("ttl_threshold"), Some(&Value::from(17_280u64)));
    assert_eq!(report.summary.passed, 2); // httpConnectivity + contractTtl
    assert_eq!(report.summary.warned, 0);
    assert_eq!(report.summary.skipped, 12);

    // The exact-body matchers prove both keys were sent as specified.
    instance_mock.assert_async().await;
    code_mock.assert_async().await;
}

/// Remaining ledgers below the threshold warn.
#[tokio::test]
async fn test_contract_ttl_warn_below_threshold() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(TTL_LATEST_LEDGER));
    mock_get_ledger_entries_key(
        &mut server,
        TTL_WASM_INSTANCE_KEY,
        json!([ledger_entry(
            TTL_WASM_INSTANCE_KEY,
            TTL_WASM_INSTANCE_XDR,
            TTL_LATEST_LEDGER + 100
        )]),
    );
    mock_get_ledger_entries_key(
        &mut server,
        TTL_WASM_CODE_KEY,
        json!([ledger_entry(
            TTL_WASM_CODE_KEY,
            "AAAAAA==",
            TTL_LATEST_LEDGER + 500
        )]),
    );

    let report = run_contract_ttl(&url, TTL_WASM_CONTRACT, 17_280).await;

    let check = find_check(&report, "contractTtl");
    assert_eq!(check.status, CheckStatus::Warn);
    let details = check.details.as_ref().expect("contractTtl details");
    assert_eq!(
        details.get("instance_remaining_ledgers"),
        Some(&Value::from(100u64))
    );
    assert_eq!(
        details.get("code_remaining_ledgers"),
        Some(&Value::from(500u64))
    );
    // Warn counts in `ran`, not in `passed`; the exit code is unchanged.
    assert_eq!(report.summary.warned, 1);
    assert_eq!(report.summary.passed, 1); // httpConnectivity only
}

/// The threshold boundary is inclusive: remaining == threshold warns.
#[tokio::test]
async fn test_contract_ttl_warn_at_threshold() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(TTL_LATEST_LEDGER));
    mock_get_ledger_entries_key(
        &mut server,
        TTL_WASM_INSTANCE_KEY,
        json!([ledger_entry(
            TTL_WASM_INSTANCE_KEY,
            TTL_WASM_INSTANCE_XDR,
            TTL_LATEST_LEDGER + 17_280
        )]),
    );
    mock_get_ledger_entries_key(
        &mut server,
        TTL_WASM_CODE_KEY,
        json!([ledger_entry(
            TTL_WASM_CODE_KEY,
            "AAAAAA==",
            TTL_LATEST_LEDGER + 20_000
        )]),
    );

    let report = run_contract_ttl(&url, TTL_WASM_CONTRACT, 17_280).await;

    let check = find_check(&report, "contractTtl");
    assert_eq!(check.status, CheckStatus::Warn);
    let details = check.details.as_ref().expect("contractTtl details");
    assert_eq!(
        details.get("instance_remaining_ledgers"),
        Some(&Value::from(17_280u64))
    );
    assert_eq!(report.summary.warned, 1);
}

/// A Stellar Asset Contract has no Wasm and therefore no code entry:
/// the check passes on the instance entry alone and reports the code
/// remaining as null. Only one `getLedgerEntries` call is made.
#[tokio::test]
async fn test_contract_ttl_sac_no_code_entry_passes() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(TTL_LATEST_LEDGER));
    let instance_mock = mock_get_ledger_entries_key(
        &mut server,
        TTL_SAC_INSTANCE_KEY,
        json!([ledger_entry(
            TTL_SAC_INSTANCE_KEY,
            TTL_SAC_INSTANCE_XDR,
            8_837_665
        )]),
    );

    let report = run_contract_ttl(&url, TTL_SAC_CONTRACT, 17_280).await;

    let check = find_check(&report, "contractTtl");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("contractTtl details");
    assert_eq!(
        details.get("instance_remaining_ledgers"),
        Some(&Value::from(2_897_222u64))
    );
    assert_eq!(details.get("code_remaining_ledgers"), Some(&Value::Null));
    assert_eq!(
        details.get("code_executable"),
        Some(&Value::String("stellar_asset".to_string()))
    );

    instance_mock.assert_async().await;
}

/// A missing instance entry (archived / expired / never deployed) fails
/// with the `missing_field` shape — it is a finding, not a Skip.
#[tokio::test]
async fn test_contract_ttl_missing_instance_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(TTL_LATEST_LEDGER));
    mock_get_ledger_entries_key(&mut server, TTL_WASM_INSTANCE_KEY, json!([]));

    let report = run_contract_ttl(&url, TTL_WASM_CONTRACT, 17_280).await;

    let check = find_check(&report, "contractTtl");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        error_shape_of(&report, "contractTtl").as_deref(),
        Some("missing_field")
    );
    assert_eq!(report.summary.failed, 1);
}

/// A Wasm contract whose code entry is missing fails as well.
#[tokio::test]
async fn test_contract_ttl_missing_code_fails() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(TTL_LATEST_LEDGER));
    mock_get_ledger_entries_key(
        &mut server,
        TTL_WASM_INSTANCE_KEY,
        json!([ledger_entry(
            TTL_WASM_INSTANCE_KEY,
            TTL_WASM_INSTANCE_XDR,
            TTL_LATEST_LEDGER + 100_000
        )]),
    );
    mock_get_ledger_entries_key(&mut server, TTL_WASM_CODE_KEY, json!([]));

    let report = run_contract_ttl(&url, TTL_WASM_CONTRACT, 17_280).await;

    let check = find_check(&report, "contractTtl");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        error_shape_of(&report, "contractTtl").as_deref(),
        Some("missing_field")
    );
}

/// With no `--contract`, the check is present when explicitly requested
/// but reports `Skip` with evidence pointing at the flag.
#[tokio::test]
async fn test_contract_ttl_skip_without_contract() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    let report = DiagnosticReport::run_all_with_config_and_contract(
        &url,
        RpcClientConfig::new(Duration::from_secs(5)),
        Some("contractTtl"),
        1,
        None,
        None,
    )
    .await;

    let check = find_check(&report, "contractTtl");
    assert_eq!(check.status, CheckStatus::Skip);
    assert!(
        check
            .evidence
            .as_deref()
            .unwrap_or("")
            .contains("--contract"),
        "skip evidence must name the --contract flag"
    );
    assert_eq!(report.summary.passed, 1); // httpConnectivity only
    assert_eq!(report.summary.skipped, 13);
}

/// Default runs (no `--contract`) keep exactly the 13 checks they had
/// before the TTL check existed.
#[tokio::test]
async fn test_contract_ttl_absent_from_default_run() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    let report = DiagnosticReport::run_all(&url, Duration::from_secs(5), None, 1).await;

    assert_eq!(report.checks.len(), 13);
    assert!(
        report.checks.iter().all(|c| c.name != "contractTtl"),
        "default run must not include contractTtl"
    );
}

/// Latest ledger used by the `retentionWindow` tests; chosen so the
/// retained counts below are exact.
const RETENTION_LATEST: u64 = 1_000_000;

/// Mock `getLatestLedger` (sequence) and one page of `getLedgers` with
/// the given `latestLedger` / `oldestLedger` window. `oldest` of `None`
/// omits `oldestLedger` from the response (the Skip case).
fn mock_retention_ledgers(server: &mut mockito::Server, latest: u64, oldest: Option<u64>) {
    mock_latest_ledger(server, json!(latest));

    let mut result = json!({
        "ledgers": [
            {"hash": "9f3308826b666c11df41693b17eddbb68aea9758be0f67fd5df81f6cf5c48984", "sequence": latest, "ledgerCloseTime": "1765223032"}
        ],
        "latestLedger": latest,
        "latestLedgerCloseTime": 1765223032,
        "cursor": ""
    });
    if let Some(oldest) = oldest {
        result["oldestLedger"] = json!(oldest);
        result["oldestLedgerCloseTime"] = json!(1765223032);
    }

    server
        .mock("POST", "/")
        .match_body(mockito::Matcher::Json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLedgers",
            "params": {"startLedger": latest, "pagination": {"limit": 2}}
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json_body(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": result
        })))
        .create();
}

/// Run only the `retentionWindow` check against the mock server, with
/// the given threshold.
async fn run_retention_window(url: &str, threshold: u64) -> DiagnosticReport {
    DiagnosticReport::run_all_with_config_and_contract(
        url,
        RpcClientConfig::new(Duration::from_secs(5)),
        Some("retentionWindow"),
        1,
        None,
        Some(threshold),
    )
    .await
}

/// Retained ledgers above the threshold pass: 20,000 retained (> 17,280).
#[tokio::test]
async fn test_retention_window_pass() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_retention_ledgers(&mut server, RETENTION_LATEST, Some(980_000));

    let report = run_retention_window(&url, 17_280).await;

    let check = find_check(&report, "retentionWindow");
    assert_eq!(check.status, CheckStatus::Pass);
    let details = check.details.as_ref().expect("retentionWindow details");
    assert_eq!(
        details.get("latest_ledger"),
        Some(&Value::from(RETENTION_LATEST))
    );
    assert_eq!(details.get("oldest_ledger"), Some(&Value::from(980_000u64)));
    assert_eq!(
        details.get("retained_ledgers"),
        Some(&Value::from(20_000u64))
    );
    assert_eq!(
        details.get("retained_days_approx"),
        Some(&Value::from(1.16f64))
    );
    assert_eq!(details.get("min_retention"), Some(&Value::from(17_280u64)));
    assert_eq!(report.summary.passed, 2); // httpConnectivity + retentionWindow
    assert_eq!(report.summary.skipped, 11);
}

/// Retained ledgers below the threshold warn.
#[tokio::test]
async fn test_retention_window_warn_below_threshold() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_retention_ledgers(&mut server, RETENTION_LATEST, Some(990_000));

    let report = run_retention_window(&url, 17_280).await;

    let check = find_check(&report, "retentionWindow");
    assert_eq!(check.status, CheckStatus::Warn);
    let details = check.details.as_ref().expect("retentionWindow details");
    assert_eq!(
        details.get("retained_ledgers"),
        Some(&Value::from(10_000u64))
    );
    assert_eq!(
        details.get("retained_days_approx"),
        Some(&Value::from(0.58f64))
    );
    // Warn counts in `ran`, not in `passed`; the exit code is unchanged.
    assert_eq!(report.summary.warned, 1);
    assert_eq!(report.summary.passed, 1); // httpConnectivity only
}

/// The threshold boundary is inclusive: retained == threshold warns.
#[tokio::test]
async fn test_retention_window_warn_at_threshold() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_retention_ledgers(&mut server, RETENTION_LATEST, Some(982_720));

    let report = run_retention_window(&url, 17_280).await;

    let check = find_check(&report, "retentionWindow");
    assert_eq!(check.status, CheckStatus::Warn);
    let details = check.details.as_ref().expect("retentionWindow details");
    assert_eq!(
        details.get("retained_ledgers"),
        Some(&Value::from(17_280u64))
    );
}

/// A response with no `oldestLedger` cannot be judged: Skip, not Fail.
#[tokio::test]
async fn test_retention_window_skip_without_oldest_ledger() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_retention_ledgers(&mut server, RETENTION_LATEST, None);

    let report = run_retention_window(&url, 17_280).await;

    let check = find_check(&report, "retentionWindow");
    assert_eq!(check.status, CheckStatus::Skip);
    assert!(
        check
            .evidence
            .as_deref()
            .unwrap_or("")
            .contains("oldestLedger"),
        "skip evidence must name the missing field"
    );
}

/// A JSON-RPC error from `getLedgers` is a Fail with an error shape.
#[tokio::test]
async fn test_retention_window_fails_on_rpc_error() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_latest_ledger(&mut server, json!(RETENTION_LATEST));
    mock_json_rpc_error(&mut server, "getLedgers", -32602);

    let report = run_retention_window(&url, 17_280).await;

    let check = find_check(&report, "retentionWindow");
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        error_shape_of(&report, "retentionWindow").as_deref(),
        Some("invalid_params")
    );
}

/// The default threshold (17280) applies when no override is given:
/// 10,000 retained warns with the default too.
#[tokio::test]
async fn test_retention_window_default_threshold() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_retention_ledgers(&mut server, RETENTION_LATEST, Some(990_000));

    let report =
        DiagnosticReport::run_all(&url, Duration::from_secs(5), Some("retentionWindow"), 1).await;

    let check = find_check(&report, "retentionWindow");
    assert_eq!(check.status, CheckStatus::Warn);
    let details = check.details.as_ref().expect("retentionWindow details");
    assert_eq!(details.get("min_retention"), Some(&Value::from(17_280u64)));
}

/// Default runs include `retentionWindow` (13 checks total) and it
/// passes on a well-retained endpoint.
#[tokio::test]
async fn test_retention_window_present_in_default_run() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();

    mock_retention_ledgers(&mut server, RETENTION_LATEST, Some(980_000));

    let report = DiagnosticReport::run_all(&url, Duration::from_secs(5), None, 1).await;

    assert_eq!(report.checks.len(), 13);
    let check = find_check(&report, "retentionWindow");
    assert_eq!(check.status, CheckStatus::Pass);
}

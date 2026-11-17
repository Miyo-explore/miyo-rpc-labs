//! Integration tests for the Miyo RPC Labs CLI.
//!
//! These tests exercise the CLI through `std::process::Command`.
//! They validate help/version output and argument validation.
//! No network access is required.

use std::process::Command;

/// Path to the compiled `miyo-rpc` binary, provided by cargo's test harness.
const BIN: &str = env!("CARGO_BIN_EXE_miyo-rpc");

#[test]
fn test_version_flag() {
    let output = Command::new(BIN)
        .arg("--version")
        .output()
        .expect("failed to run --version");
    assert!(output.status.success(), "version command should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("miyo-rpc 0.1.0"),
        "version output should contain 'miyo-rpc 0.1.0', got: {stdout}"
    );
}

#[test]
fn test_help_flag() {
    let output = Command::new(BIN)
        .arg("--help")
        .output()
        .expect("failed to run --help");
    assert!(output.status.success(), "help command should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Miyo RPC Labs"),
        "help output should contain tool name, got: {stdout}"
    );
}

#[test]
fn test_help_flag_lists_checks() {
    let output = Command::new(BIN)
        .arg("--help")
        .output()
        .expect("failed to run --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("check"),
        "help should mention 'check' subcommand"
    );
}

#[test]
fn test_check_missing_url() {
    let output = Command::new(BIN)
        .arg("check")
        .output()
        .expect("failed to execute miyo-rpc check");
    // Missing required --url should be a failure
    assert!(!output.status.success(), "check without --url should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required") || stderr.contains("url") || stderr.contains("USAGE"),
        "stderr should mention url requirement, got: {stderr}"
    );
}

#[test]
fn test_check_invalid_url() {
    let output = Command::new(BIN)
        .args(["check", "--url", "not-a-url"])
        .output()
        .expect("failed to execute miyo-rpc check");
    assert!(
        !output.status.success(),
        "check with invalid URL should fail"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("FAIL"),
        "output should show FAIL for invalid URL"
    );
}

#[test]
fn test_check_invalid_scheme_rejected() {
    let output = Command::new(BIN)
        .args(["check", "--url", "ftp://example.com/rpc"])
        .output()
        .expect("failed to execute miyo-rpc check");
    assert!(!output.status.success(), "ftp scheme should be rejected");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("FAIL"),
        "output should show FAIL for invalid scheme"
    );
}

#[test]
fn test_check_empty_method_rejected() {
    // An empty --method is a usage error (exit code 2) rejected before any
    // network request is made. The URL is unreachable on purpose: the guard
    // must fire before the client is even constructed.
    let output = Command::new(BIN)
        .args(["check", "--url", "http://127.0.0.1:1", "--method", ""])
        .output()
        .expect("failed to execute miyo-rpc check");
    assert_eq!(
        output.status.code(),
        Some(2),
        "empty --method should exit with code 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("empty"),
        "stderr should mention 'empty', got: {stderr}"
    );
}

#[test]
fn test_check_whitespace_method_rejected() {
    let output = Command::new(BIN)
        .args(["check", "--url", "http://127.0.0.1:1", "--method", "   "])
        .output()
        .expect("failed to execute miyo-rpc check");
    assert_eq!(
        output.status.code(),
        Some(2),
        "whitespace-only --method should exit with code 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("empty"),
        "stderr should mention 'empty', got: {stderr}"
    );
}

#[test]
fn test_check_valid_method_not_rejected_with_code_2() {
    // A valid built-in method must not trigger the empty-method guard.
    // The loopback URL refuses the connection immediately, so the run
    // fails with code 1 (failed checks) — never code 2 — and no live
    // endpoint is contacted.
    let output = Command::new(BIN)
        .args([
            "check",
            "--url",
            "http://127.0.0.1:1",
            "--method",
            "getHealth",
        ])
        .output()
        .expect("failed to execute miyo-rpc check");
    assert_ne!(
        output.status.code(),
        Some(2),
        "valid --method must not exit with code 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("must not be empty"),
        "valid --method must not trigger the empty-method guard, got: {stderr}"
    );
}

#[test]
fn test_check_max_response_bytes_zero_rejected() {
    // A zero cap is a usage error (exit code 2) rejected before any
    // network request is made. The URL is unreachable on purpose: the
    // guard must fire before the client is even constructed.
    let output = Command::new(BIN)
        .args([
            "check",
            "--url",
            "http://127.0.0.1:1",
            "--max-response-bytes",
            "0",
        ])
        .output()
        .expect("failed to execute miyo-rpc check");
    assert_eq!(
        output.status.code(),
        Some(2),
        "zero --max-response-bytes should exit with code 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("greater than 0"),
        "stderr should mention the bound, got: {stderr}"
    );
}

#[test]
fn test_check_max_response_bytes_non_numeric_rejected() {
    // clap rejects a non-numeric value for the usize flag with exit code 2.
    let output = Command::new(BIN)
        .args([
            "check",
            "--url",
            "http://127.0.0.1:1",
            "--max-response-bytes",
            "abc",
        ])
        .output()
        .expect("failed to execute miyo-rpc check");
    assert_eq!(
        output.status.code(),
        Some(2),
        "non-numeric --max-response-bytes should exit with code 2"
    );
}

#[test]
fn test_check_user_agent_flag_accepted() {
    // The flag must parse cleanly. The loopback URL refuses the connection
    // immediately, so the run fails with code 1 (failed checks) — never
    // code 2 — and no live endpoint is contacted.
    let output = Command::new(BIN)
        .args([
            "check",
            "--url",
            "http://127.0.0.1:1",
            "--method",
            "getHealth",
            "--user-agent",
            "custom/1.0",
        ])
        .output()
        .expect("failed to execute miyo-rpc check");
    assert_ne!(
        output.status.code(),
        Some(2),
        "--user-agent should be accepted, not rejected as a usage error"
    );
}

/// Local server that 302-redirects `/` to `/target`, which answers with a
/// healthy JSON-RPC result. Used to prove `--no-follow-redirects` end to
/// end (loopback only; no live endpoint is contacted).
fn redirect_mock_server() -> mockito::ServerGuard {
    let mut server = mockito::Server::new();
    let target = format!("{}/target", server.url());
    server
        .mock("POST", "/")
        .with_status(302)
        .with_header("location", target.as_str())
        .create();
    server
        .mock("GET", "/")
        .with_status(302)
        .with_header("location", target.as_str())
        .create();
    // reqwest converts POST to GET when following a 302, so the target
    // mock answers GET. The same 200 also satisfies the httpConnectivity
    // check (any status < 500 passes).
    server
        .mock("GET", "/target")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","id":1,"result":{"status":"healthy","latestLedger":51583040,"oldestLedger":51565760,"ledgerRetentionWindow":17281}}"#)
        .create();
    server
}

#[test]
fn test_check_follows_redirects_by_default() {
    let server = redirect_mock_server();
    let url = server.url();
    let output = Command::new(BIN)
        .args(["check", "--url", &url, "--method", "getHealth"])
        .output()
        .expect("failed to execute miyo-rpc check");
    assert!(
        output.status.success(),
        "redirect should be followed by default, got exit {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn test_check_no_follow_redirects_reports_302() {
    // With the flag set, the getHealth check fails on the 302 (exit 1)
    // and the 3xx status is reported in the output. The httpConnectivity
    // check uses its own client and still follows the redirect, so it
    // passes in both modes.
    let server = redirect_mock_server();
    let url = server.url();
    let output = Command::new(BIN)
        .args([
            "check",
            "--url",
            &url,
            "--method",
            "getHealth",
            "--no-follow-redirects",
        ])
        .output()
        .expect("failed to execute miyo-rpc check");
    assert_eq!(
        output.status.code(),
        Some(1),
        "unfollowed 302 should fail the check with exit 1"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("302"),
        "output should report the 302 status, got: {stdout}"
    );
}

/// Local mock server answering a healthy getHealth (GET + POST).
/// Used for batch output-format tests (loopback only; no live endpoint
/// is contacted).
fn healthy_batch_mock_server() -> mockito::ServerGuard {
    let mut server = mockito::Server::new();
    server.mock("GET", "/").with_status(200).create();
    server
        .mock("POST", "/")
        .match_header("content-type", "application/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","id":1,"result":{"status":"healthy","latestLedger":51583040,"oldestLedger":51565760,"ledgerRetentionWindow":17281}}"#)
        .create();
    server
}

fn write_endpoints_file(name: &str, content: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(name);
    std::fs::write(&path, content).unwrap();
    path
}

#[test]
fn test_batch_output_human_is_not_json() {
    // #14: `--output human` must produce human-readable output, not JSON.
    let server = healthy_batch_mock_server();
    let path = write_endpoints_file("miyo_batch_cli_human.txt", &format!("{}\n", server.url()));
    let output = Command::new(BIN)
        .args([
            "batch",
            "--file",
            path.to_str().unwrap(),
            "--output",
            "human",
            "--method",
            "getHealth",
        ])
        .output()
        .expect("failed to execute miyo-rpc batch");
    let _ = std::fs::remove_file(&path);
    assert!(
        output.status.success(),
        "batch with a healthy endpoint should exit 0, got {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("[PASS]"),
        "human output should mark the endpoint PASS, got: {stdout}"
    );
    assert!(
        stdout.contains("Summary:"),
        "human output should end with an aggregate summary, got: {stdout}"
    );
    assert!(
        serde_json::from_str::<serde_json::Value>(&stdout).is_err(),
        "human output must not parse as JSON: {stdout}"
    );
}

#[test]
fn test_batch_output_json_is_json() {
    // #14: `--output json` must keep producing the aggregate JSON document.
    let server = healthy_batch_mock_server();
    let path = write_endpoints_file("miyo_batch_cli_json.txt", &format!("{}\n", server.url()));
    let output = Command::new(BIN)
        .args([
            "batch",
            "--file",
            path.to_str().unwrap(),
            "--output",
            "json",
            "--method",
            "getHealth",
        ])
        .output()
        .expect("failed to execute miyo-rpc batch");
    let _ = std::fs::remove_file(&path);
    assert!(
        output.status.success(),
        "batch with a healthy endpoint should exit 0, got {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("json output must parse as JSON");
    assert_eq!(
        parsed.get("schema_version").and_then(|v| v.as_str()),
        Some("1.0"),
        "json output must carry schema_version 1.0"
    );
    assert_eq!(
        parsed
            .get("summary")
            .and_then(|s| s.get("exit_code"))
            .and_then(|v| v.as_u64()),
        Some(0),
        "healthy batch must report exit_code 0"
    );
}

#[test]
fn test_batch_timeout_secs_flag_accepted() {
    // #15: `--timeout-secs` must parse on `batch`. The loopback URL refuses
    // the connection immediately, so the run fails with code 1 (failed
    // checks) — never code 2 — and no live endpoint is contacted.
    let path = write_endpoints_file("miyo_batch_cli_timeout.txt", "http://127.0.0.1:1\n");
    let output = Command::new(BIN)
        .args([
            "batch",
            "--file",
            path.to_str().unwrap(),
            "--timeout-secs",
            "30",
            "--method",
            "getHealth",
        ])
        .output()
        .expect("failed to execute miyo-rpc batch");
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        output.status.code(),
        Some(1),
        "--timeout-secs should be accepted (exit 1 from failed checks, not 2)"
    );
}

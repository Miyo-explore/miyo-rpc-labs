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

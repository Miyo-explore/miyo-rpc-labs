//! Output formatting and URL sanitization.
//!
//! Provides:
//! - [`sanitize_url`]: redact query parameters whose names suggest secrets.
//! - [`format_human`]: colored terminal output.
//! - [`format_json`]: machine-readable JSON report.

use crate::diagnostics::{DiagnosticReport, ReportSummary};
use serde::Serialize;

/// Keys in a URL query string that are treated as sensitive and redacted.
///
/// Matching is case-insensitive on the key name.
static SENSITIVE_KEYS: &[&str] = &[
    "key",
    "token",
    "secret",
    "password",
    "pass",
    "auth",
    "authorization",
    "apikey",
    "api_key",
    "secret_key",
    "access_token",
    "refresh_token",
    "private_key",
    "bearer",
];

/// Sanitize a URL by redacting any query parameters whose keys match
/// [`SENSITIVE_KEYS`] and any userinfo (username:password) component.
///
/// Returns the full URL with sensitive values replaced by `REDACTED`.
/// This prevents credentials embedded in the URL (e.g.
/// `https://user:secret@host/rpc?api_key=x`) from leaking into output or
/// logs.
/// If the URL cannot be parsed, a best-effort redaction is attempted:
/// credential-like userinfo, sensitive query parameters, raw key=value
/// pairs, and bare secret tokens are redacted so credential-like patterns
/// never reach output or error messages.
pub fn sanitize_url(url: &str) -> String {
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => {
            // The string did not parse as a URL. Still attempt to redact
            // credential-like userinfo and sensitive query parameters
            // (e.g. `https://user:pass@host?api_key=x`) so secrets are
            // never echoed in error messages or output.
            return redact_unparseable_url(url);
        }
    };

    let mut new_url = parsed;
    let mut changed = false;

    // Redact userinfo (username:password@host) to prevent credential leakage.
    if !new_url.username().is_empty() || new_url.password().is_some() {
        // set_username returns Ok even if it replaces the existing username.
        if new_url.set_username("REDACTED").is_ok() {
            changed = true;
        }
        // set_password requires the URL to have a username; we set it first.
        if new_url.set_password(Some("REDACTED")).is_ok() {
            changed = true;
        }
    }

    // Collect query pairs before mutating, since Url borrows.
    let pairs: Vec<(String, String)> = new_url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    if !pairs.is_empty() {
        let mut new_query_parts: Vec<String> = Vec::new();

        for (k, v) in &pairs {
            if is_sensitive_composite_key(k) {
                new_query_parts.push(format!("{}=REDACTED", urlencoding::encode(k)));
                changed = true;
            } else {
                new_query_parts.push(format!(
                    "{}={}",
                    urlencoding::encode(k),
                    urlencoding::encode(v)
                ));
            }
        }

        if changed {
            let query_str = new_query_parts.join("&");
            new_url.set_query(Some(&query_str));
        }
    }

    if changed {
        new_url.to_string()
    } else {
        url.to_string()
    }
}

/// Best-effort redaction of credential-like patterns in strings that
/// [`url::Url::parse`] rejects (e.g. because of an embedded space or other
/// invalid character).
///
/// Handles three cases:
/// 1. **Userinfo** — `scheme://user:pass@host` → `scheme://REDACTED:REDACTED@host`
/// 2. **Sensitive query params** — `?api_key=secret` → `?api_key=REDACTED`
/// 3. **Raw key=value pairs** (non-URL strings) — `secret_key=value` → `secret_key=REDACTED`
///
/// If none of the above patterns are present, the string is returned unchanged.
fn redact_unparseable_url(url: &str) -> String {
    let mut result = url.to_string();

    // 1. Redact userinfo: scheme://user:pass@host → scheme://REDACTED:REDACTED@host
    if let Some(scheme_end) = result.find("://") {
        let after_scheme = &result[scheme_end + 3..];
        if let Some(at_pos) = after_scheme.find('@') {
            let scheme = &result[..scheme_end];
            let host_and_rest = &after_scheme[at_pos + 1..];
            result = format!("{}://REDACTED:REDACTED@{}", scheme, host_and_rest);
        }
    }

    // 2. Redact sensitive query parameters: ?api_key=secret → ?api_key=REDACTED
    if let Some(q_pos) = result.find('?') {
        let (before, query_body) = result.split_at(q_pos);
        let query = &query_body[1..]; // skip '?'
        let redacted = redact_sensitive_query_params(query);
        result = format!("{}?{}", before, redacted);
    }

    // 3. For non-URL strings (no scheme://) that contain key=value pairs,
    // redact values whose keys are sensitive. This catches raw credential
    // strings that don't parse as URLs (e.g. "secret_key=sk-abc123") which
    // would otherwise pass through unchanged.
    if result.find('=').is_some() && result.find("://").is_none() {
        result = redact_raw_key_value_pairs(&result);
    }

    // 4. If no URL structure is present and the string looks like a bare
    // secret token (e.g. "sk-abc123", "0x1234..."), redact it entirely.
    // This catches raw API keys or tokens pasted as endpoint lines.
    if result.find("://").is_none()
        && result.find('?').is_none()
        && result.find('=').is_none()
        && looks_like_secret(url)
    {
        return "REDACTED".to_string();
    }

    result
}

/// Replace the *values* of sensitive query parameters with `REDACTED`.
/// Parameter names are URL-decoded before checking against
/// [`SENSITIVE_KEYS`] (so `api%5fkey` matches `api_key`), then matched
/// as a composite key whose underscore/dash/dot-separated components
/// include a sensitive key (e.g. `my_secret` → component `secret`).
/// Non-sensitive pairs are preserved verbatim.
fn redact_sensitive_query_params(query: &str) -> String {
    query
        .split('&')
        .map(|p| {
            if let Some(eq_pos) = p.find('=') {
                let key = &p[..eq_pos];
                let is_sensitive = urlencoding::decode(key)
                    .as_ref()
                    .map(|d| is_sensitive_composite_key(d))
                    .unwrap_or(false);
                if is_sensitive {
                    format!("{key}=REDACTED")
                } else {
                    p.to_string()
                }
            } else {
                let is_sensitive = urlencoding::decode(p)
                    .as_ref()
                    .map(|d| is_sensitive_composite_key(d))
                    .unwrap_or(false);
                if is_sensitive {
                    format!("{p}=REDACTED")
                } else {
                    p.to_string()
                }
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Check whether a key is sensitive — exact match against [`SENSITIVE_KEYS`]
/// (case-insensitive) or any underscore/dash/dot-separated component is a
/// sensitive key (e.g. `my_secret` → component `secret` is sensitive).
fn is_sensitive_composite_key(key: &str) -> bool {
    SENSITIVE_KEYS.iter().any(|s| s.eq_ignore_ascii_case(key))
        || key.split(['_', '-', '.']).any(|part| {
            !part.is_empty() && SENSITIVE_KEYS.iter().any(|s| s.eq_ignore_ascii_case(part))
        })
}

/// Redact the values of `key=value` pairs in a non-URL string where the
/// key matches [`SENSITIVE_KEYS`]. Pairs are delimited by `&`.
///
/// This catches raw credential strings like `secret_key=sk-abc123` that
/// don't parse as URLs and therefore bypass the standard URL-sanitization
/// logic.
fn redact_raw_key_value_pairs(input: &str) -> String {
    input
        .split('&')
        .map(|p| {
            if let Some(eq_pos) = p.find('=') {
                let key = &p[..eq_pos];
                if is_sensitive_composite_key(key) {
                    format!("{key}=REDACTED")
                } else {
                    p.to_string()
                }
            } else {
                p.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Check if a bare string (no URL structure) looks like a secret token
/// based on common prefixes or format patterns.
fn looks_like_secret(s: &str) -> bool {
    // Common secret key prefixes (Stripe, Privy, etc.) — require minimum
    // length to avoid redacting short strings.
    let has_prefix = s.starts_with("sk-")
        || s.starts_with("sk_")
        || s.starts_with("pk-")
        || s.starts_with("pk_")
        || s.starts_with("0x");
    let long_enough = s.len() >= 10;

    // Long hex string without dots (e.g. Ethereum key, API token).
    let is_long_hex =
        s.len() >= 32 && s.chars().all(|c: char| c.is_ascii_hexdigit()) && !s.contains('.');

    (has_prefix && long_enough) || is_long_hex
}

// ────────────────────────────────
// Human output
// ────────────────────────────────

/// Human-readable formatting of a diagnostic report.
pub fn format_human(report: &DiagnosticReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("Miyo RPC Labs v{}\n", report.tool_version));
    out.push_str(&format!("Endpoint: {}\n\n", report.endpoint));

    for check in &report.checks {
        let status_str = match check.status {
            crate::diagnostics::CheckStatus::Pass => "\x1b[32mPASS\x1b[0m",
            crate::diagnostics::CheckStatus::Fail => "\x1b[31mFAIL\x1b[0m",
            crate::diagnostics::CheckStatus::Skip => "\x1b[33mSKIP\x1b[0m",
        };

        let duration = check
            .duration_ms
            .map(|d| format!(" ({d}ms)"))
            .unwrap_or_default();

        out.push_str(&format!("  {} {} {}\n", status_str, check.name, duration));

        if let Some(details) = &check.details {
            for (k, v) in details.iter() {
                out.push_str(&format!("    {}: {}\n", k, v));
            }
        }

        if let Some(err) = &check.error {
            out.push_str(&format!(
                "    error: {} ({:?})\n",
                check.error_message.as_deref().unwrap_or("unknown error"),
                err
            ));
        }

        if let Some(ev) = &check.evidence {
            out.push_str(&format!("    evidence: {}\n", ev));
        }
        out.push('\n');
    }

    if report.runs > 1 {
        out.push_str(&format!("Runs: {} total\n", report.runs));
    }
    out.push_str(&format!(
        "Summary: {} passed, {} failed, {} skipped ({} ran)\n",
        report.summary.passed, report.summary.failed, report.summary.skipped, report.summary.ran
    ));

    out
}

// ────────────────────────────────
// JSON output
// ────────────────────────────────

/// JSON-compatible serializable wrapper for the report.
pub fn format_json(report: &DiagnosticReport) -> Result<String, serde_json::Error> {
    #[derive(Serialize)]
    struct JsonReport {
        schema_version: String,
        tool_version: String,
        endpoint: String,
        runs: usize,
        checks: Vec<CheckResultJson>,
        summary: ReportSummary,
    }

    #[derive(Serialize)]
    struct CheckResultJson {
        name: String,
        status: String,
        timestamp: String,
        duration_ms: Option<u64>,
        http_status: Option<u16>,
        endpoint: String,
        details: Option<serde_json::Map<String, serde_json::Value>>,
        error: Option<String>,
        error_message: Option<String>,
        evidence: Option<String>,
    }

    let checks: Vec<_> = report
        .checks
        .iter()
        .map(|c| CheckResultJson {
            name: c.name.clone(),
            status: match c.status {
                crate::diagnostics::CheckStatus::Pass => "pass".to_string(),
                crate::diagnostics::CheckStatus::Fail => "fail".to_string(),
                crate::diagnostics::CheckStatus::Skip => "skip".to_string(),
            },
            timestamp: c.timestamp.clone(),
            duration_ms: c.duration_ms,
            http_status: c.http_status,
            endpoint: c.endpoint.clone(),
            details: c.details.clone(),
            error: c
                .error
                .as_ref()
                .map(|e| serde_json::to_string(e).unwrap_or_default()),
            error_message: c.error_message.clone(),
            evidence: c.evidence.clone(),
        })
        .collect();

    let j = JsonReport {
        schema_version: crate::SCHEMA_VERSION.to_string(),
        tool_version: report.tool_version.clone(),
        endpoint: report.endpoint.clone(),
        runs: report.runs,
        checks,
        summary: report.summary.clone(),
    };

    serde_json::to_string_pretty(&j)
}

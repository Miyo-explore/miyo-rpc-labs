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
    /// The check completed, but the result is suspicious enough to flag
    /// without failing the run: the endpoint answered, yet the observed
    /// state (e.g. a ledger that does not advance across `--runs N`)
    /// suggests it is stale or inconsistent. Warn never changes the exit
    /// code — only `Fail` does.
    Warn,
    /// The check could not be performed (e.g., a prior check failed and this
    /// check depends on it, or the method is not applicable).
    Skip,
}

/// Determine the worst (highest-precedence) status from a set of results.
///
/// Precedence: Fail > Warn > Skip > Pass. A `Fail` in any run must never be
/// hidden by `Warn`, `Skip`, or `Pass` in another run, and a `Warn` must
/// never be hidden by `Skip` or `Pass` — but a `Warn` is not a failure, so
/// it never outranks `Fail` and never changes the exit code.
pub fn worst_status(statuses: &[CheckStatus]) -> CheckStatus {
    if statuses.contains(&CheckStatus::Fail) {
        CheckStatus::Fail
    } else if statuses.contains(&CheckStatus::Warn) {
        CheckStatus::Warn
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
    /// HTTP 429: the endpoint is reachable but is rate-limiting this client.
    RateLimited,
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
            ErrorCategory::RateLimited => IssueCategory::RateLimited,
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
    /// Number of checks that passed with a warning (counted in `ran`,
    /// not in `passed`). Warn never changes the exit code.
    pub warned: u32,
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
                    // for fields that are short and non-sensitive. A huge
                    // result must not bloat the report — and must not drop
                    // the short fields along with the blobs: getLatestLedger
                    // returns hundred-kilobyte headerXdr/metadataXdr, and
                    // an all-or-nothing size cap used to discard the whole
                    // result, leaving details without `id` — which made
                    // the ledger-health analysis (sequence + hash) blind
                    // on live endpoints.
                    let result_str = serde_json::to_string(&result);
                    if let Ok(s) = &result_str {
                        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(s) {
                            for (k, v) in map {
                                let short = if s.len() < 2048 {
                                    true
                                } else {
                                    serde_json::to_string(&v)
                                        .map(|t| t.len())
                                        .unwrap_or(usize::MAX)
                                        < 256
                                };
                                if short {
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

/// Maximum number of events fetched per page by the built-in `getEvents`
/// check. The Stellar RPC limit is 1–10000 (default 100); 10 keeps the
/// diagnostic response small — a check proves the method works, it does not
/// stream history.
pub const GET_EVENTS_PAGE_LIMIT: u64 = 10;

/// Maximum number of ledgers fetched per page by the built-in `getLedgers`
/// check. The Stellar RPC limit is 1–200 (default 50); 2 keeps the
/// diagnostic response small — each ledger carries base64 XDR header +
/// metadata, so a check proves the method works, it does not stream history.
pub const GET_LEDGERS_PAGE_LIMIT: u64 = 2;

/// Maximum number of transactions fetched per page by the built-in
/// `getTransactions` check. The Stellar RPC limit is 1–200 (default 50);
/// 2 keeps the diagnostic response small — each transaction carries base64
/// XDR envelope + result + metadata, so a check proves the method works,
/// it does not stream history.
pub const GET_TRANSACTIONS_PAGE_LIMIT: u64 = 2;

/// Default TTL threshold in ledgers for the `contractTtl` check
/// (17280 ≈ 1 day at ~5 s per ledger), following the evergreen
/// convention. Remaining ledgers at or below this threshold report
/// `Warn`; the boundary is inclusive.
pub const DEFAULT_TTL_THRESHOLD: u64 = 17280;

/// Base64-encoded XDR of the `LedgerKey` probed by the built-in
/// `getLedgerEntries` check: an Account key for the all-zero ed25519 key
/// (union discriminant 0 = ACCOUNT, key type 0 = ED25519, 32 zero bytes).
///
/// A fixed synthetic key keeps the check network-agnostic: it needs no
/// account that exists on every network and invents no wallet address.
/// Verified live 2026-11-21 against soroban-testnet.stellar.org: the RPC
/// accepts the key and returns the entry (`lastModifiedLedgerSeq`
/// 5629649); on a network where the account does not exist it returns an
/// empty `entries` array, which is also a Pass — the check proves the
/// method works, it does not assert that the account exists.
pub const GET_LEDGER_ENTRIES_PROBE_KEY: &str =
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

/// Parse a u64 out of a result field that Stellar RPC serializes as a JSON
/// string in some responses and as a number in others; accept both, and
/// reject anything else (missing field, wrong type, unparseable string).
fn parse_u64_field(result: &Value, field: &str) -> Option<u64> {
    match result.get(field) {
        Some(Value::String(s)) => s.parse::<u64>().ok(),
        Some(Value::Number(n)) => n.as_u64(),
        _ => None,
    }
}

/// Parse a ledger sequence out of a `getLatestLedger` result.
///
/// Stellar RPC serializes the sequence as a JSON string in some responses
/// and as a number in others; accept both, and reject anything else
/// (missing field, wrong type, unparseable string).
fn parse_ledger_sequence(result: &Value) -> Option<u64> {
    parse_u64_field(result, "sequence")
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes as standard base64 (with `=` padding).
///
/// Implemented by hand: the crate has no base64 dependency, and the only
/// base64 values the tool handles are XDR ledger keys / entry values.
fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(BASE64_ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(BASE64_ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            BASE64_ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64_ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Decode standard base64 (with or without `=` padding) into bytes.
///
/// Returns `None` on any character outside the alphabet, on bad padding,
/// or on a non-canonical trailing bit pattern.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut padding = 0usize;
    for c in input.bytes() {
        if c == b'=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return None; // data after padding
        }
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    if padding > 2 || bits >= 6 || (bits > 0 && (acc & ((1u32 << bits) - 1)) != 0) {
        return None;
    }
    Some(out)
}

/// A minimal big-endian XDR reader: the `contractTtl` check only needs to
/// walk one known struct shape, not a general XDR decoder.
struct XdrReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> XdrReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn read_i32(&mut self) -> Option<i32> {
        let b = self.bytes.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(i32::from_be_bytes(b.try_into().ok()?))
    }

    fn read_bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let b = self.bytes.get(self.pos..self.pos + n)?;
        self.pos += n;
        Some(b)
    }
}

/// What a contract instance entry's executable is.
///
/// The `ContractCode` entry only exists for Wasm executables: a Stellar
/// Asset Contract (or an external reference) has no Wasm hash and
/// therefore no code entry to check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstanceExecutable {
    /// Wasm code: the 32-byte hash identifies the `ContractCode` entry.
    Wasm([u8; 32]),
    /// Stellar Asset Contract: no Wasm, no `ContractCode` entry exists.
    StellarAsset,
    /// External reference: no retrievable `ContractCode` entry.
    ExternalRef,
}

/// Build the base64 XDR `LedgerKey` for a contract's instance entry:
/// `ContractData` (discriminant 6) for the contract address, with the
/// `LedgerKeyContractInstance` key (SCVal discriminant 20) and
/// `PERSISTENT` durability.
///
/// Verified live 2026-11-27 against soroban-testnet.stellar.org: the RPC
/// echoes this exact key back for the contract's instance entry.
pub fn contract_instance_ledger_key(contract_id: &[u8; 32]) -> String {
    let mut xdr = Vec::with_capacity(48);
    xdr.extend_from_slice(&6i32.to_be_bytes()); // LedgerEntryType CONTRACT_DATA
    xdr.extend_from_slice(&1i32.to_be_bytes()); // SCAddress CONTRACT
    xdr.extend_from_slice(contract_id);
    xdr.extend_from_slice(&20i32.to_be_bytes()); // SCVal LEDGER_KEY_CONTRACT_INSTANCE
    xdr.extend_from_slice(&1i32.to_be_bytes()); // ContractDataDurability PERSISTENT
    base64_encode(&xdr)
}

/// Build the base64 XDR `LedgerKey` for a contract's code entry:
/// `ContractCode` (discriminant 7) for the Wasm hash.
pub fn contract_code_ledger_key(wasm_hash: &[u8; 32]) -> String {
    let mut xdr = Vec::with_capacity(36);
    xdr.extend_from_slice(&7i32.to_be_bytes()); // LedgerEntryType CONTRACT_CODE
    xdr.extend_from_slice(wasm_hash);
    base64_encode(&xdr)
}

/// Parse the executable out of a contract instance entry's `xdr` value
/// (base64-encoded `LedgerEntryData`).
///
/// Layout (verified live 2026-11-27 and against the Stellar XDR spec):
/// `LedgerEntryData` discriminant 6 (`CONTRACT_DATA`), `ContractDataEntry`
/// `ext` (ExtensionPoint, 0 = empty), `SCAddress` discriminant 1
/// (`CONTRACT`) + 32-byte contract ID, `SCVal` discriminant 20
/// (`LEDGER_KEY_CONTRACT_INSTANCE`), durability, `SCVal` discriminant 19
/// (`CONTRACT_INSTANCE`), then the `ContractExecutable` discriminant:
/// 0 = WASM (followed by the 32-byte Wasm hash), 1 = STELLAR_ASSET,
/// 2 = EXTERNAL_REF. Returns `None` when the bytes do not parse as a
/// contract instance entry.
fn parse_instance_executable(entry_xdr_b64: &str) -> Option<InstanceExecutable> {
    let bytes = base64_decode(entry_xdr_b64)?;
    let mut r = XdrReader::new(&bytes);
    if r.read_i32()? != 6 {
        return None; // not a CONTRACT_DATA entry
    }
    if r.read_i32()? != 0 {
        return None; // non-empty ExtensionPoint
    }
    if r.read_i32()? != 1 {
        return None; // not a CONTRACT SCAddress
    }
    r.read_bytes(32)?; // contract ID
    if r.read_i32()? != 20 {
        return None; // not a LEDGER_KEY_CONTRACT_INSTANCE key
    }
    let _durability = r.read_i32()?;
    if r.read_i32()? != 19 {
        return None; // not a CONTRACT_INSTANCE value
    }
    match r.read_i32()? {
        0 => {
            let hash = r.read_bytes(32)?;
            let mut wasm_hash = [0u8; 32];
            wasm_hash.copy_from_slice(hash);
            Some(InstanceExecutable::Wasm(wasm_hash))
        }
        1 => Some(InstanceExecutable::StellarAsset),
        2 => Some(InstanceExecutable::ExternalRef),
        _ => None,
    }
}

/// A built-in check for the Stellar RPC `getEvents` method.
///
/// Unlike the no-parameter checks, `getEvents` requires a `startLedger`
/// inside the endpoint's retention window, so the check first asks
/// `getLatestLedger` for the latest sequence and then requests a single
/// page (`pagination.limit` = [`GET_EVENTS_PAGE_LIMIT`], no filters — all
/// event types, all contracts) starting at that ledger. The response
/// `cursor` is reported as `has_cursor` (a continuation page exists) but
/// is never followed: a diagnostics check fetches one page, it does not
/// stream events. No user-supplied params are involved — `--params` is
/// still ignored for built-in methods.
#[derive(Debug, Clone, Copy, Default)]
pub struct GetEventsCheck;

impl RpcCheck for GetEventsCheck {
    fn name(&self) -> &str {
        "getEvents"
    }

    fn run<'a>(
        &'a self,
        client: &'a RpcClient,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult> + Send + 'a>> {
        Box::pin(async move {
            let endpoint = client.sanitized_url();
            let timestamp = chrono::Utc::now().to_rfc3339();
            let method = "getEvents";

            // 1. Resolve startLedger from getLatestLedger: the request is
            // rejected when startLedger is outside the retained window, so
            // a hardcoded ledger would rot. The latest ledger is always in
            // range (startLedger is inclusive).
            let sequence = match client.call::<Value>("getLatestLedger", None).await {
                Ok((_, result)) => match parse_ledger_sequence(&result) {
                    Some(seq) => seq,
                    None => {
                        return fail_result(
                            method,
                            &endpoint,
                            timestamp,
                            &MiyoError::MalformedStructure(
                                "getLatestLedger result has no usable 'sequence' field".to_string(),
                            ),
                            "Call getLatestLedger to resolve the startLedger for getEvents."
                                .to_string(),
                        );
                    }
                },
                Err(e) => {
                    return fail_result(
                        method,
                        &endpoint,
                        timestamp,
                        &e,
                        "Call getLatestLedger to resolve the startLedger for getEvents before calling getEvents."
                            .to_string(),
                    );
                }
            };

            // 2. Fetch one page of events starting at the latest ledger.
            let params = serde_json::json!({
                "startLedger": sequence,
                "pagination": { "limit": GET_EVENTS_PAGE_LIMIT },
            });
            let evidence = format!(
                "Sent JSON-RPC 2.0 POST to {endpoint} with method 'getEvents' and params {{startLedger: {sequence}, pagination: {{limit: {GET_EVENTS_PAGE_LIMIT}}}}} (startLedger resolved from getLatestLedger; no filters)."
            );
            match client.call::<Value>(method, Some(params)).await {
                Ok((info, result)) => {
                    let events = match result.get("events").and_then(Value::as_array) {
                        Some(events) => events,
                        None => {
                            return fail_result(
                                method,
                                &endpoint,
                                timestamp,
                                &MiyoError::MalformedStructure(
                                    "getEvents result has no 'events' array".to_string(),
                                ),
                                evidence,
                            );
                        }
                    };
                    let duration_ms = info.duration.as_millis() as u64;
                    let mut details = serde_json::Map::new();
                    details.insert(
                        "description".to_string(),
                        Value::String(
                            "Contract events in the latest ledger (one page, no filters)."
                                .to_string(),
                        ),
                    );
                    details.insert(
                        "http_status".to_string(),
                        Value::Number(serde_json::Number::from(info.status)),
                    );
                    details.insert(
                        "start_ledger".to_string(),
                        Value::Number(serde_json::Number::from(sequence)),
                    );
                    details.insert(
                        "events_returned".to_string(),
                        Value::Number(serde_json::Number::from(events.len())),
                    );
                    if let Some(v) = result.get("latestLedger") {
                        details.insert("latest_ledger".to_string(), v.clone());
                    }
                    if let Some(v) = result.get("oldestLedger") {
                        details.insert("oldest_ledger".to_string(), v.clone());
                    }
                    details.insert(
                        "has_cursor".to_string(),
                        Value::Bool(
                            result
                                .get("cursor")
                                .and_then(Value::as_str)
                                .is_some_and(|c| !c.is_empty()),
                        ),
                    );

                    CheckResult {
                        name: method.to_string(),
                        status: CheckStatus::Pass,
                        timestamp,
                        duration_ms: Some(duration_ms),
                        http_status: Some(info.status),
                        endpoint: endpoint.clone(),
                        details: Some(details),
                        error: None,
                        error_message: None,
                        evidence: Some(evidence),
                    }
                }
                Err(e) => fail_result(method, &endpoint, timestamp, &e, evidence),
            }
        })
    }
}

/// A built-in check for the Stellar RPC `getLedgerEntries` method.
///
/// The method needs at least one ledger key, so the check sends the fixed
/// synthetic probe key [`GET_LEDGER_ENTRIES_PROBE_KEY`] — an Account key
/// for the all-zero ed25519 key, which no real wallet owns. No
/// `getLatestLedger` round-trip is needed: keys are absolute, not
/// ledger-relative. Details carry `entries_returned` and `latest_ledger`;
/// an empty `entries` array is a Pass (the account simply does not exist
/// on this network). No user-supplied params are involved — `--params` is
/// still ignored for built-in methods.
#[derive(Debug, Clone, Copy, Default)]
pub struct GetLedgerEntriesCheck;

impl RpcCheck for GetLedgerEntriesCheck {
    fn name(&self) -> &str {
        "getLedgerEntries"
    }

    fn run<'a>(
        &'a self,
        client: &'a RpcClient,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult> + Send + 'a>> {
        Box::pin(async move {
            let endpoint = client.sanitized_url();
            let timestamp = chrono::Utc::now().to_rfc3339();
            let method = "getLedgerEntries";

            let params = serde_json::json!({
                "keys": [GET_LEDGER_ENTRIES_PROBE_KEY],
            });
            let evidence = format!(
                "Sent JSON-RPC 2.0 POST to {endpoint} with method 'getLedgerEntries' and params {{keys: [<probe LedgerKey>]}} (fixed synthetic all-zero Account key; no account is assumed to exist)."
            );
            match client.call::<Value>(method, Some(params)).await {
                Ok((info, result)) => {
                    let entries = match result.get("entries").and_then(Value::as_array) {
                        Some(entries) => entries,
                        None => {
                            return fail_result(
                                method,
                                &endpoint,
                                timestamp,
                                &MiyoError::MalformedStructure(
                                    "getLedgerEntries result has no 'entries' array".to_string(),
                                ),
                                evidence,
                            );
                        }
                    };
                    let duration_ms = info.duration.as_millis() as u64;
                    let mut details = serde_json::Map::new();
                    details.insert(
                        "description".to_string(),
                        Value::String(
                            "Ledger entries for a fixed probe key (read-only).".to_string(),
                        ),
                    );
                    details.insert(
                        "http_status".to_string(),
                        Value::Number(serde_json::Number::from(info.status)),
                    );
                    details.insert(
                        "entries_returned".to_string(),
                        Value::Number(serde_json::Number::from(entries.len())),
                    );
                    if let Some(latest) = parse_u64_field(&result, "latestLedger") {
                        details.insert(
                            "latest_ledger".to_string(),
                            Value::Number(serde_json::Number::from(latest)),
                        );
                    }

                    CheckResult {
                        name: method.to_string(),
                        status: CheckStatus::Pass,
                        timestamp,
                        duration_ms: Some(duration_ms),
                        http_status: Some(info.status),
                        endpoint: endpoint.clone(),
                        details: Some(details),
                        error: None,
                        error_message: None,
                        evidence: Some(evidence),
                    }
                }
                Err(e) => fail_result(method, &endpoint, timestamp, &e, evidence),
            }
        })
    }
}

/// A built-in check for the Stellar RPC `getLedgers` method.
///
/// Like `getEvents`, `getLedgers` requires a `startLedger` inside the
/// endpoint's retention window, so the check first asks `getLatestLedger`
/// for the latest sequence and then requests a single page
/// (`pagination.limit` = [`GET_LEDGERS_PAGE_LIMIT`], starting at that
/// ledger — the latest ledger itself is always in range). The response
/// `cursor` is reported as `has_cursor` but is never followed: a
/// diagnostics check fetches one page, it does not stream history.
/// Details carry `start_ledger`, `ledgers_returned`, `latest_ledger`,
/// `oldest_ledger`, and `has_cursor`; an empty page is a Pass. If
/// `getLatestLedger` fails, `getLedgers` fails instead of probing with a
/// guessed ledger. No user-supplied params are involved — `--params` is
/// still ignored for built-in methods.
#[derive(Debug, Clone, Copy, Default)]
pub struct GetLedgersCheck;

impl RpcCheck for GetLedgersCheck {
    fn name(&self) -> &str {
        "getLedgers"
    }

    fn run<'a>(
        &'a self,
        client: &'a RpcClient,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult> + Send + 'a>> {
        Box::pin(async move {
            let endpoint = client.sanitized_url();
            let timestamp = chrono::Utc::now().to_rfc3339();
            let method = "getLedgers";

            // 1. Resolve startLedger from getLatestLedger: the request is
            // rejected when startLedger is outside the retained window, so
            // a hardcoded ledger would rot. The latest ledger is always in
            // range (startLedger is inclusive).
            let sequence = match client.call::<Value>("getLatestLedger", None).await {
                Ok((_, result)) => match parse_ledger_sequence(&result) {
                    Some(seq) => seq,
                    None => {
                        return fail_result(
                            method,
                            &endpoint,
                            timestamp,
                            &MiyoError::MalformedStructure(
                                "getLatestLedger result has no usable 'sequence' field".to_string(),
                            ),
                            "Call getLatestLedger to resolve the startLedger for getLedgers."
                                .to_string(),
                        );
                    }
                },
                Err(e) => {
                    return fail_result(
                        method,
                        &endpoint,
                        timestamp,
                        &e,
                        "Call getLatestLedger to resolve the startLedger for getLedgers before calling getLedgers."
                            .to_string(),
                    );
                }
            };

            // 2. Fetch one page of ledgers starting at the latest ledger.
            let params = serde_json::json!({
                "startLedger": sequence,
                "pagination": { "limit": GET_LEDGERS_PAGE_LIMIT },
            });
            let evidence = format!(
                "Sent JSON-RPC 2.0 POST to {endpoint} with method 'getLedgers' and params {{startLedger: {sequence}, pagination: {{limit: {GET_LEDGERS_PAGE_LIMIT}}}}} (startLedger resolved from getLatestLedger)."
            );
            match client.call::<Value>(method, Some(params)).await {
                Ok((info, result)) => {
                    let ledgers = match result.get("ledgers").and_then(Value::as_array) {
                        Some(ledgers) => ledgers,
                        None => {
                            return fail_result(
                                method,
                                &endpoint,
                                timestamp,
                                &MiyoError::MalformedStructure(
                                    "getLedgers result has no 'ledgers' array".to_string(),
                                ),
                                evidence,
                            );
                        }
                    };
                    let duration_ms = info.duration.as_millis() as u64;
                    let mut details = serde_json::Map::new();
                    details.insert(
                        "description".to_string(),
                        Value::String(
                            "Ledger headers starting at the latest ledger (one page).".to_string(),
                        ),
                    );
                    details.insert(
                        "http_status".to_string(),
                        Value::Number(serde_json::Number::from(info.status)),
                    );
                    details.insert(
                        "start_ledger".to_string(),
                        Value::Number(serde_json::Number::from(sequence)),
                    );
                    details.insert(
                        "ledgers_returned".to_string(),
                        Value::Number(serde_json::Number::from(ledgers.len())),
                    );
                    if let Some(latest) = parse_u64_field(&result, "latestLedger") {
                        details.insert(
                            "latest_ledger".to_string(),
                            Value::Number(serde_json::Number::from(latest)),
                        );
                    }
                    if let Some(oldest) = parse_u64_field(&result, "oldestLedger") {
                        details.insert(
                            "oldest_ledger".to_string(),
                            Value::Number(serde_json::Number::from(oldest)),
                        );
                    }
                    details.insert(
                        "has_cursor".to_string(),
                        Value::Bool(
                            result
                                .get("cursor")
                                .and_then(Value::as_str)
                                .is_some_and(|c| !c.is_empty()),
                        ),
                    );

                    CheckResult {
                        name: method.to_string(),
                        status: CheckStatus::Pass,
                        timestamp,
                        duration_ms: Some(duration_ms),
                        http_status: Some(info.status),
                        endpoint: endpoint.clone(),
                        details: Some(details),
                        error: None,
                        error_message: None,
                        evidence: Some(evidence),
                    }
                }
                Err(e) => fail_result(method, &endpoint, timestamp, &e, evidence),
            }
        })
    }
}

/// A built-in check for the Stellar RPC `getTransactions` method.
///
/// Like `getEvents` and `getLedgers`, `getTransactions` requires a
/// `startLedger` inside the endpoint's retention window, so the check first
/// asks `getLatestLedger` for the latest sequence and then requests a single
/// page (`pagination.limit` = [`GET_TRANSACTIONS_PAGE_LIMIT`], starting at
/// that ledger — the latest ledger itself is always in range). No
/// `includeFailed` is sent, so the server default applies (failed
/// transactions included). The response `cursor` is reported as `has_cursor`
/// but is never followed: a diagnostics check fetches one page, it does not
/// stream history. Details carry `start_ledger`, `transactions_returned`,
/// `latest_ledger`, `oldest_ledger`, and `has_cursor`; an empty page is a
/// Pass. If `getLatestLedger` fails, `getTransactions` fails instead of
/// probing with a guessed ledger. No user-supplied params are involved —
/// `--params` is still ignored for built-in methods.
#[derive(Debug, Clone, Copy, Default)]
pub struct GetTransactionsCheck;

impl RpcCheck for GetTransactionsCheck {
    fn name(&self) -> &str {
        "getTransactions"
    }

    fn run<'a>(
        &'a self,
        client: &'a RpcClient,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult> + Send + 'a>> {
        Box::pin(async move {
            let endpoint = client.sanitized_url();
            let timestamp = chrono::Utc::now().to_rfc3339();
            let method = "getTransactions";

            // 1. Resolve startLedger from getLatestLedger: the request is
            // rejected when startLedger is outside the retained window, so
            // a hardcoded ledger would rot. The latest ledger is always in
            // range (startLedger is inclusive).
            let sequence = match client.call::<Value>("getLatestLedger", None).await {
                Ok((_, result)) => match parse_ledger_sequence(&result) {
                    Some(seq) => seq,
                    None => {
                        return fail_result(
                            method,
                            &endpoint,
                            timestamp,
                            &MiyoError::MalformedStructure(
                                "getLatestLedger result has no usable 'sequence' field".to_string(),
                            ),
                            "Call getLatestLedger to resolve the startLedger for getTransactions."
                                .to_string(),
                        );
                    }
                },
                Err(e) => {
                    return fail_result(
                        method,
                        &endpoint,
                        timestamp,
                        &e,
                        "Call getLatestLedger to resolve the startLedger for getTransactions before calling getTransactions."
                            .to_string(),
                    );
                }
            };

            // 2. Fetch one page of transactions starting at the latest ledger.
            let params = serde_json::json!({
                "startLedger": sequence,
                "pagination": { "limit": GET_TRANSACTIONS_PAGE_LIMIT },
            });
            let evidence = format!(
                "Sent JSON-RPC 2.0 POST to {endpoint} with method 'getTransactions' and params {{startLedger: {sequence}, pagination: {{limit: {GET_TRANSACTIONS_PAGE_LIMIT}}}}} (startLedger resolved from getLatestLedger)."
            );
            match client.call::<Value>(method, Some(params)).await {
                Ok((info, result)) => {
                    let transactions = match result.get("transactions").and_then(Value::as_array) {
                        Some(transactions) => transactions,
                        None => {
                            return fail_result(
                                method,
                                &endpoint,
                                timestamp,
                                &MiyoError::MalformedStructure(
                                    "getTransactions result has no 'transactions' array"
                                        .to_string(),
                                ),
                                evidence,
                            );
                        }
                    };
                    let duration_ms = info.duration.as_millis() as u64;
                    let mut details = serde_json::Map::new();
                    details.insert(
                        "description".to_string(),
                        Value::String("Transactions in the latest ledger (one page).".to_string()),
                    );
                    details.insert(
                        "http_status".to_string(),
                        Value::Number(serde_json::Number::from(info.status)),
                    );
                    details.insert(
                        "start_ledger".to_string(),
                        Value::Number(serde_json::Number::from(sequence)),
                    );
                    details.insert(
                        "transactions_returned".to_string(),
                        Value::Number(serde_json::Number::from(transactions.len())),
                    );
                    if let Some(latest) = parse_u64_field(&result, "latestLedger") {
                        details.insert(
                            "latest_ledger".to_string(),
                            Value::Number(serde_json::Number::from(latest)),
                        );
                    }
                    if let Some(oldest) = parse_u64_field(&result, "oldestLedger") {
                        details.insert(
                            "oldest_ledger".to_string(),
                            Value::Number(serde_json::Number::from(oldest)),
                        );
                    }
                    details.insert(
                        "has_cursor".to_string(),
                        Value::Bool(
                            result
                                .get("cursor")
                                .and_then(Value::as_str)
                                .is_some_and(|c| !c.is_empty()),
                        ),
                    );

                    CheckResult {
                        name: method.to_string(),
                        status: CheckStatus::Pass,
                        timestamp,
                        duration_ms: Some(duration_ms),
                        http_status: Some(info.status),
                        endpoint: endpoint.clone(),
                        details: Some(details),
                        error: None,
                        error_message: None,
                        evidence: Some(evidence),
                    }
                }
                Err(e) => fail_result(method, &endpoint, timestamp, &e, evidence),
            }
        })
    }
}

/// Built-in check for `getFeeStats`.
///
/// Sends a `getFeeStats` JSON-RPC request with no params and surfaces the
/// fee statistics in `details`: the Soroban inclusion-fee mode / p95 /
/// transaction count, the classic inclusion-fee mode / p95, and the
/// `latestLedger` the statistics were computed over.
///
/// Response shape (verified live against soroban-testnet.stellar.org):
/// `result` carries `sorobanInclusionFee`, `inclusionFee`, and
/// `latestLedger`; each fee object holds percentiles (`max`, `min`,
/// `mode`, `p10` … `p99`) plus `transactionCount` and `ledgerCount`.
/// There is no `feeCharged` percentile and no `ledgerCapacityUsage` field
/// in the current response — older descriptions of this method mention
/// them, but the live wire format does not return them.
///
/// A JSON-RPC error (including `-32601` on an endpoint that does not
/// implement the method) is a Fail with
/// [`IssueCategory::UnsupportedMethod`], like the other built-in method
/// checks — no built-in check Skips on `-32601` today.
#[derive(Debug, Clone, Copy, Default)]
pub struct GetFeeStatsCheck;

impl RpcCheck for GetFeeStatsCheck {
    fn name(&self) -> &str {
        "getFeeStats"
    }

    fn run<'a>(
        &'a self,
        client: &'a RpcClient,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult> + Send + 'a>> {
        Box::pin(async move {
            let endpoint = client.sanitized_url();
            let timestamp = chrono::Utc::now().to_rfc3339();
            let method = "getFeeStats";
            let evidence = format!(
                "Sent JSON-RPC 2.0 POST to {endpoint} with method 'getFeeStats' and no params."
            );

            match client.call::<Value>(method, None).await {
                Ok((info, result)) => {
                    // Read one percentile out of a nested fee object, e.g.
                    // result["sorobanInclusionFee"]["mode"]. Both number
                    // and numeric-string forms are accepted.
                    let fee_stat = |fee_key: &str, stat_key: &str| -> Option<u64> {
                        result
                            .get(fee_key)
                            .and_then(|fee| parse_u64_field(fee, stat_key))
                    };
                    let soroban_mode = fee_stat("sorobanInclusionFee", "mode");
                    let soroban_p95 = fee_stat("sorobanInclusionFee", "p95");
                    let soroban_tx_count = fee_stat("sorobanInclusionFee", "transactionCount");
                    let inclusion_mode = fee_stat("inclusionFee", "mode");
                    let inclusion_p95 = fee_stat("inclusionFee", "p95");

                    // The fee objects are the substance of the response:
                    // a result without either of them is malformed, not a
                    // Pass. `latestLedger` is optional context, not proof.
                    if soroban_mode.is_none() && inclusion_mode.is_none() {
                        return fail_result(
                            method,
                            &endpoint,
                            timestamp,
                            &MiyoError::MalformedStructure(
                                "getFeeStats result has neither 'sorobanInclusionFee' nor 'inclusionFee' fee object"
                                    .to_string(),
                            ),
                            evidence,
                        );
                    }

                    let duration_ms = info.duration.as_millis() as u64;
                    let mut details = serde_json::Map::new();
                    details.insert(
                        "description".to_string(),
                        Value::String(
                            "Fee statistics for recent ledgers (inclusion fee percentiles)."
                                .to_string(),
                        ),
                    );
                    details.insert(
                        "http_status".to_string(),
                        Value::Number(serde_json::Number::from(info.status)),
                    );
                    if let Some(mode) = soroban_mode {
                        details.insert(
                            "soroban_inclusion_fee_mode".to_string(),
                            Value::Number(serde_json::Number::from(mode)),
                        );
                    }
                    if let Some(p95) = soroban_p95 {
                        details.insert(
                            "soroban_inclusion_fee_p95".to_string(),
                            Value::Number(serde_json::Number::from(p95)),
                        );
                    }
                    if let Some(count) = soroban_tx_count {
                        details.insert(
                            "soroban_inclusion_fee_transaction_count".to_string(),
                            Value::Number(serde_json::Number::from(count)),
                        );
                    }
                    if let Some(mode) = inclusion_mode {
                        details.insert(
                            "inclusion_fee_mode".to_string(),
                            Value::Number(serde_json::Number::from(mode)),
                        );
                    }
                    if let Some(p95) = inclusion_p95 {
                        details.insert(
                            "inclusion_fee_p95".to_string(),
                            Value::Number(serde_json::Number::from(p95)),
                        );
                    }
                    if let Some(latest) = parse_u64_field(&result, "latestLedger") {
                        details.insert(
                            "latest_ledger".to_string(),
                            Value::Number(serde_json::Number::from(latest)),
                        );
                    }

                    CheckResult {
                        name: method.to_string(),
                        status: CheckStatus::Pass,
                        timestamp,
                        duration_ms: Some(duration_ms),
                        http_status: Some(info.status),
                        endpoint: endpoint.clone(),
                        details: Some(details),
                        error: None,
                        error_message: None,
                        evidence: Some(evidence),
                    }
                }
                Err(e) => fail_result(method, &endpoint, timestamp, &e, evidence),
            }
        })
    }
}

/// Built-in check for `getTransaction` (singular).
///
/// Distinct from the `getTransactions` (plural) check: `getTransaction`
/// fetches one transaction by hash, so this check first resolves the
/// latest ledger, fetches one page of transactions from it
/// (`pagination.limit` = [`GET_TRANSACTIONS_PAGE_LIMIT`]), and calls
/// `getTransaction` with the first returned hash. Details carry
/// `tx_hash`, `status`, `ledger`, `fee_charged`, and XDR-presence flags
/// (`has_envelope_xdr`, `has_result_xdr`, `has_result_meta_xdr`).
///
/// An empty page is a Skip, not a Fail: there is no transaction to
/// fetch, so the method was never exercised. If `getLatestLedger` or
/// `getTransactions` fails, the check fails instead of probing with a
/// guessed ledger or hash. No user-supplied params are involved —
/// `--params` is still ignored for built-in methods.
#[derive(Debug, Clone, Copy, Default)]
pub struct GetTransactionCheck;

impl RpcCheck for GetTransactionCheck {
    fn name(&self) -> &str {
        "getTransaction"
    }

    fn run<'a>(
        &'a self,
        client: &'a RpcClient,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult> + Send + 'a>> {
        Box::pin(async move {
            let endpoint = client.sanitized_url();
            let timestamp = chrono::Utc::now().to_rfc3339();
            let method = "getTransaction";

            // 1. Resolve startLedger from getLatestLedger (same as the
            // getTransactions check: a hardcoded ledger would rot).
            let sequence = match client.call::<Value>("getLatestLedger", None).await {
                Ok((_, result)) => match parse_ledger_sequence(&result) {
                    Some(seq) => seq,
                    None => {
                        return fail_result(
                            method,
                            &endpoint,
                            timestamp,
                            &MiyoError::MalformedStructure(
                                "getLatestLedger result has no usable 'sequence' field".to_string(),
                            ),
                            "Call getLatestLedger to resolve the startLedger for getTransaction."
                                .to_string(),
                        );
                    }
                },
                Err(e) => {
                    return fail_result(
                        method,
                        &endpoint,
                        timestamp,
                        &e,
                        "Call getLatestLedger to resolve the startLedger for getTransaction before calling getTransactions."
                            .to_string(),
                    );
                }
            };

            // 2. Fetch one page of transactions starting at the latest
            // ledger and take the first hash.
            let page_params = serde_json::json!({
                "startLedger": sequence,
                "pagination": { "limit": GET_TRANSACTIONS_PAGE_LIMIT },
            });
            let page_evidence = format!(
                "Sent JSON-RPC 2.0 POST to {endpoint} with method 'getTransactions' and params {{startLedger: {sequence}, pagination: {{limit: {GET_TRANSACTIONS_PAGE_LIMIT}}}}} (startLedger resolved from getLatestLedger)."
            );
            let tx_hash = match client
                .call::<Value>("getTransactions", Some(page_params))
                .await
            {
                Ok((_, result)) => match result.get("transactions").and_then(Value::as_array) {
                    Some(transactions) => match transactions.first() {
                        // Live `getTransactions` pages carry the hash as
                        // `txHash`; older RPC versions returned `hash`.
                        // Accept both, preferring the live field.
                        Some(tx) => match tx
                            .get("txHash")
                            .or_else(|| tx.get("hash"))
                            .and_then(Value::as_str)
                        {
                            Some(hash) => hash.to_string(),
                            None => {
                                return fail_result(
                                    method,
                                    &endpoint,
                                    timestamp,
                                    &MiyoError::MalformedStructure(
                                        "getTransactions page has a transaction with no 'txHash' (or legacy 'hash') string"
                                            .to_string(),
                                    ),
                                    page_evidence,
                                );
                            }
                        },
                        None => {
                            // No transaction exists to fetch: the method
                            // was never exercised, so Skip, not Fail.
                            return CheckResult {
                                name: method.to_string(),
                                status: CheckStatus::Skip,
                                timestamp,
                                duration_ms: None,
                                http_status: None,
                                endpoint: endpoint.clone(),
                                details: None,
                                error: None,
                                error_message: None,
                                evidence: Some(format!(
                                    "{page_evidence} The latest ledger ({sequence}) holds no transactions, so there is no hash to call getTransaction with."
                                )),
                            };
                        }
                    },
                    None => {
                        return fail_result(
                            method,
                            &endpoint,
                            timestamp,
                            &MiyoError::MalformedStructure(
                                "getTransactions result has no 'transactions' array".to_string(),
                            ),
                            page_evidence,
                        );
                    }
                },
                Err(e) => {
                    return fail_result(method, &endpoint, timestamp, &e, page_evidence);
                }
            };

            // 3. Fetch the single transaction by hash.
            let params = serde_json::json!({ "hash": tx_hash });
            let evidence = format!(
                "Sent JSON-RPC 2.0 POST to {endpoint} with method 'getTransaction' and params {{hash: {tx_hash}}} (hash taken from the first transaction in the latest ledger, {sequence})."
            );
            match client.call::<Value>(method, Some(params)).await {
                Ok((info, result)) => {
                    let status = result.get("status").and_then(Value::as_str);
                    let fee_charged = match result.get("feeCharged") {
                        Some(Value::String(s)) => Some(s.clone()),
                        Some(Value::Number(n)) => Some(n.to_string()),
                        _ => None,
                    };
                    // The status is the substance of the response: a
                    // result without it is malformed, not a Pass.
                    let Some(status) = status else {
                        return fail_result(
                            method,
                            &endpoint,
                            timestamp,
                            &MiyoError::MalformedStructure(
                                "getTransaction result has no 'status' string".to_string(),
                            ),
                            evidence,
                        );
                    };

                    let duration_ms = info.duration.as_millis() as u64;
                    let mut details = serde_json::Map::new();
                    details.insert(
                        "description".to_string(),
                        Value::String(
                            "Single transaction by hash (first transaction in the latest ledger)."
                                .to_string(),
                        ),
                    );
                    details.insert(
                        "http_status".to_string(),
                        Value::Number(serde_json::Number::from(info.status)),
                    );
                    details.insert("tx_hash".to_string(), Value::String(tx_hash));
                    details.insert("status".to_string(), Value::String(status.to_string()));
                    if let Some(ledger) = parse_u64_field(&result, "ledger") {
                        details.insert(
                            "ledger".to_string(),
                            Value::Number(serde_json::Number::from(ledger)),
                        );
                    }
                    if let Some(fee) = fee_charged {
                        details.insert("fee_charged".to_string(), Value::String(fee));
                    }
                    details.insert(
                        "has_envelope_xdr".to_string(),
                        Value::Bool(
                            result
                                .get("envelopeXdr")
                                .and_then(Value::as_str)
                                .is_some_and(|x| !x.is_empty()),
                        ),
                    );
                    details.insert(
                        "has_result_xdr".to_string(),
                        Value::Bool(
                            result
                                .get("resultXdr")
                                .and_then(Value::as_str)
                                .is_some_and(|x| !x.is_empty()),
                        ),
                    );
                    details.insert(
                        "has_result_meta_xdr".to_string(),
                        Value::Bool(
                            result
                                .get("resultMetaXdr")
                                .and_then(Value::as_str)
                                .is_some_and(|x| !x.is_empty()),
                        ),
                    );

                    CheckResult {
                        name: method.to_string(),
                        status: CheckStatus::Pass,
                        timestamp,
                        duration_ms: Some(duration_ms),
                        http_status: Some(info.status),
                        endpoint: endpoint.clone(),
                        details: Some(details),
                        error: None,
                        error_message: None,
                        evidence: Some(evidence),
                    }
                }
                Err(e) => fail_result(method, &endpoint, timestamp, &e, evidence),
            }
        })
    }
}

/// Base64 transaction envelope of a real `InvokeHostFunction`
/// transaction observed on Testnet (hash
/// `bbcc753be258d79ad3caa1c66ae150116ab1bc91103b98f7d52a2a4481c38d95`,
/// ledger ~5933150, 2026-11-26), used as the probe for the
/// `simulateTransaction` built-in check.
///
/// Simulation ignores signatures and sequence numbers, so replaying an
/// already-applied transaction is valid: the RPC re-executes the
/// invocation against current ledger state. Verified live twice on
/// 2026-11-26 (`minResourceFee` 31052, 1 result, no error).
///
/// The probe is Testnet-specific: the invoked contract only exists on
/// Testnet. If the contract is ever archived or the invocation becomes
/// invalid, the check Fails with the simulation error — replace this
/// envelope with a fresh single-`InvokeHostFunction` transaction from
/// Testnet (find one by scanning recent `getTransactions` pages and
/// replaying candidates through `simulateTransaction` until one
/// succeeds).
pub const SIMULATE_TRANSACTION_TESTNET_PROBE_ENVELOPE: &str = "AAAAAgAAAAAoMR/fQTNR+Z1hGiMJ0xWlOgbb00KDntm6j4ULYR7mPwAAaj4AVd21AAC7xQAAAAEAAAAAAAAAAAAAAABrCAunAAAAAAAAAAEAAAAAAAAAGAAAAAAAAAABK5CmUiNmzKbbVp8feLML0zyF8TO28ZXVThh0TKeb1FEAAAAed2l0aGRyYXdfY29sbGF0ZXJhbF9hbmRfcmVkZWVtAAAAAAAFAAAAAwAAAAQAAAASAAAAAAAAAAAoMR/fQTNR+Z1hGiMJ0xWlOgbb00KDntm6j4ULYR7mPwAAAAMAAAAAAAAABQAAAAAAJyasAAAAEQAAAAEAAAAEAAAADwAAAAJpZAAAAAAAAwAAAAAAAAAPAAAABXNlZWQxAAAAAAAAAQAAAA8AAAAFc2VlZDIAAAAAAAABAAAADwAAAAN0YWcAAAAAAwAAAAAAAAABAAAAAAAAAAAAAAABK5CmUiNmzKbbVp8feLML0zyF8TO28ZXVThh0TKeb1FEAAAAed2l0aGRyYXdfY29sbGF0ZXJhbF9hbmRfcmVkZWVtAAAAAAAFAAAAAwAAAAQAAAASAAAAAAAAAAAoMR/fQTNR+Z1hGiMJ0xWlOgbb00KDntm6j4ULYR7mPwAAAAMAAAAAAAAABQAAAAAAJyasAAAAEQAAAAEAAAAEAAAADwAAAAJpZAAAAAAAAwAAAAAAAAAPAAAABXNlZWQxAAAAAAAAAQAAAA8AAAAFc2VlZDIAAAAAAAABAAAADwAAAAN0YWcAAAAAAwAAAAAAAAAAAAAAAQAAAAAAAAALAAAABgAAAAEJcVc0EHxTYVT2AEollRLl/knyhqcyOWnXwbF4XtfdBwAAABQAAAABAAAABgAAAAErkKZSI2bMpttWnx94swvTPIXxM7bxldVOGHRMp5vUUQAAABAAAAABAAAAAgAAAA8AAAAGTWFya2V0AAAAAAADAAAABAAAAAEAAAAGAAAAASuQplIjZsym21afH3izC9M8hfEztvGV1U4YdEynm9RRAAAAFAAAAAEAAAAGAAAAAbq2z1n9DVxz3G/hTFn6uCqSHdk2rnqqOlDjZbZOk5EFAAAAEAAAAAEAAAACAAAADwAAAAVBbGlhcwAAAAAAAA4AAAAHRVRIL1VTRAAAAAABAAAABgAAAAG6ts9Z/Q1cc9xv4UxZ+rgqkh3ZNq56qjpQ42W2TpORBQAAABAAAAABAAAAAgAAAA8AAAAFQWxpYXMAAAAAAAAOAAAADFRXQVBfRVRIL1VTRAAAAAEAAAAGAAAAAbq2z1n9DVxz3G/hTFn6uCqSHdk2rnqqOlDjZbZOk5EFAAAAEAAAAAEAAAACAAAADwAAAAVFbnRyeQAAAAAAABAAAAABAAAAAgAAAA8AAAAJUHl0aExhemVyAAAAAAAAAwAAA/QAAAABAAAABgAAAAG6ts9Z/Q1cc9xv4UxZ+rgqkh3ZNq56qjpQ42W2TpORBQAAABAAAAABAAAAAgAAAA8AAAAFRW50cnkAAAAAAAAQAAAAAQAAAAIAAAAPAAAACVB5dGhMYXplcgAAAAAAAAMAAAP1AAAAAQAAAAYAAAABurbPWf0NXHPcb+FMWfq4KpId2Taueqo6UONltk6TkQUAAAAUAAAAAQAAAAdzROCNmezEx0NPq+BHlVRmLQf4+LsndEEW+Blp5lCcngAAAAejx/X74Yiz/+1CG+Xi4tfIb19t2PjhXGHuEA9ajoVRgAAAAAel9rBsqPsQp/PRK6X64zYfBKJCBUdXCAzxTDdbiOHImAAAAAYAAAABAAAAACgxH99BM1H5nWEaIwnTFaU6BtvTQoOe2bqPhQthHuY/AAAAAUVUSAAAAAAADVyxw34LOi0VljcTFANAurLxa5N39vGFsZue/ngN16UAAAAGAAAAAQlxVzQQfFNhVPYASiWVEuX+SfKGpzI5adfBsXhe190HAAAAEAAAAAEAAAACAAAADwAAAAdCYWxhbmNlAAAAABIAAAABK5CmUiNmzKbbVp8feLML0zyF8TO28ZXVThh0TKeb1FEAAAABAAAABgAAAAErkKZSI2bMpttWnx94swvTPIXxM7bxldVOGHRMp5vUUQAAABAAAAABAAAABwAAAA8AAAAKT2JsaWdhdGlvbgAAAAAAAwAAAAQAAAASAAAAAAAAAAAoMR/fQTNR+Z1hGiMJ0xWlOgbb00KDntm6j4ULYR7mPwAAAAMAAAAAAAAAAwAAAAAAAAABAAAAAQAAAAEAAAAGAAAAASuQplIjZsym21afH3izC9M8hfEztvGV1U4YdEynm9RRAAAAEAAAAAEAAAADAAAADwAAAAdSZXNlcnZlAAAAAAMAAAAEAAAAAwAAAAAAAAABAAAABgAAAAE8M1OBUWiRW0+Y9VWvytr16SELTYfCVdmM2xT8q2pr0AAAABAAAAABAAAAAgAAAA8AAAAHQmFsYW5jZQAAAAASAAAAASuQplIjZsym21afH3izC9M8hfEztvGV1U4YdEynm9RRAAAAAQAAAAYAAAABPDNTgVFokVtPmPVVr8ra9ekhC02HwlXZjNsU/Ktqa9AAAAAUAAAAAQCqZ+UAAAB0AAAd/AAAAAAAAGnaAAAAAWEe5j8AAABAm/ufPCAw3H9aZU36sI8fdaIsvpXJy7PQtzmcRqpZUZzLBZ7v4UiwhzigInEPKLf8swNGPdV/2Flryxxz4kTOAg==";

/// Hash of the transaction whose envelope
/// [`SIMULATE_TRANSACTION_TESTNET_PROBE_ENVELOPE`] replays, for evidence.
pub const SIMULATE_TRANSACTION_TESTNET_PROBE_TX_HASH: &str =
    "bbcc753be258d79ad3caa1c66ae150116ab1bc91103b98f7d52a2a4481c38d95";

/// Built-in check for `simulateTransaction`.
///
/// `simulateTransaction` needs a transaction envelope, and a diagnostics
/// check cannot invent a valid Soroban invocation: the envelope must
/// reference a real contract on the endpoint's network. This check
/// therefore resolves the network via `getNetwork` and replays the
/// embedded Testnet probe envelope
/// ([`SIMULATE_TRANSACTION_TESTNET_PROBE_ENVELOPE`]) — but only on
/// Testnet. On any other network it Skips with evidence explaining that
/// the probe is Testnet-specific (no invoke activity was found on
/// Futurenet in a 5000-ledger scan on 2026-11-26, and no single-invoke
/// transaction was found on Mainnet in a 250-transaction scan the same
/// day, so no probe envelope exists for those networks). To simulate on
/// another network, probe with your own transaction via the arbitrary
/// method path (`--method simulateTransaction --params`).
///
/// A successful simulation (a `results` array, no `error`) is a Pass;
/// details carry `min_resource_fee`, `results_count`, `latest_ledger`,
/// and the probe `tx_hash`. A result-level `error` means the probe
/// invocation no longer simulates — a Fail, since the check cannot prove
/// the method works. If `getNetwork` fails, the check fails instead of
/// guessing the network. No user-supplied params are involved —
/// `--params` is still ignored for built-in methods. Read-only by
/// design: simulation never submits a transaction (`sendTransaction`
/// stays out of scope, see `ROADMAP.md`).
#[derive(Debug, Clone, Copy, Default)]
pub struct SimulateTransactionCheck;

impl RpcCheck for SimulateTransactionCheck {
    fn name(&self) -> &str {
        "simulateTransaction"
    }

    fn run<'a>(
        &'a self,
        client: &'a RpcClient,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult> + Send + 'a>> {
        Box::pin(async move {
            let endpoint = client.sanitized_url();
            let timestamp = chrono::Utc::now().to_rfc3339();
            let method = "simulateTransaction";

            // 1. Resolve the network: the probe envelope is only valid
            // on Testnet.
            let passphrase = match client.call::<Value>("getNetwork", None).await {
                Ok((_, result)) => match result.get("passphrase").and_then(Value::as_str) {
                    Some(passphrase) => passphrase.to_string(),
                    None => {
                        return fail_result(
                            method,
                            &endpoint,
                            timestamp,
                            &MiyoError::MalformedStructure(
                                "getNetwork result has no 'passphrase' string".to_string(),
                            ),
                            "Call getNetwork to determine the endpoint's network before simulating the Testnet probe transaction."
                                .to_string(),
                        );
                    }
                },
                Err(e) => {
                    return fail_result(
                        method,
                        &endpoint,
                        timestamp,
                        &e,
                        "Call getNetwork to determine the endpoint's network before simulating the Testnet probe transaction."
                            .to_string(),
                    );
                }
            };
            let testnet_passphrase =
                crate::network::network_passphrase("testnet").expect("testnet is a known network");
            if passphrase != testnet_passphrase {
                let network_name = crate::network::NETWORK_PASSPHRASES
                    .iter()
                    .find(|(_, p)| *p == passphrase)
                    .map(|(n, _)| *n)
                    .unwrap_or("unrecognized");
                return CheckResult {
                    name: method.to_string(),
                    status: CheckStatus::Skip,
                    timestamp,
                    duration_ms: None,
                    http_status: None,
                    endpoint: endpoint.clone(),
                    details: None,
                    error: None,
                    error_message: None,
                    evidence: Some(format!(
                        "Endpoint is on network '{network_name}', not Testnet: the simulateTransaction probe replays a real InvokeHostFunction transaction observed on Testnet (hash {SIMULATE_TRANSACTION_TESTNET_PROBE_TX_HASH}), which references a contract that only exists there. To simulate on this network, probe with your own transaction via --method simulateTransaction --params."
                    )),
                };
            }

            // 2. Replay the embedded Testnet probe envelope.
            let params = serde_json::json!({
                "transaction": SIMULATE_TRANSACTION_TESTNET_PROBE_ENVELOPE,
            });
            let evidence = format!(
                "Sent JSON-RPC 2.0 POST to {endpoint} with method 'simulateTransaction' and the embedded Testnet probe transaction (hash {SIMULATE_TRANSACTION_TESTNET_PROBE_TX_HASH}, replayed — simulation ignores signatures and sequence numbers)."
            );
            match client.call::<Value>(method, Some(params)).await {
                Ok((info, result)) => {
                    // A result-level `error` means the invocation itself
                    // failed (e.g. the probe contract is gone): the
                    // method answered, but the check cannot prove it
                    // simulates a real invocation.
                    if let Some(error) = result.get("error").and_then(Value::as_str) {
                        return fail_result(
                            method,
                            &endpoint,
                            timestamp,
                            &MiyoError::UnsupportedMethod(
                                method.to_string(),
                                format!(
                                    "simulation of the Testnet probe transaction failed: {error}"
                                ),
                            ),
                            evidence,
                        );
                    }
                    let results = match result.get("results").and_then(Value::as_array) {
                        Some(results) => results,
                        None => {
                            return fail_result(
                                method,
                                &endpoint,
                                timestamp,
                                &MiyoError::MalformedStructure(
                                    "simulateTransaction result has neither 'results' nor 'error'"
                                        .to_string(),
                                ),
                                evidence,
                            );
                        }
                    };

                    let duration_ms = info.duration.as_millis() as u64;
                    let mut details = serde_json::Map::new();
                    details.insert(
                        "description".to_string(),
                        Value::String(
                            "Simulated a real InvokeHostFunction transaction (Testnet probe, replayed)."
                                .to_string(),
                        ),
                    );
                    details.insert(
                        "http_status".to_string(),
                        Value::Number(serde_json::Number::from(info.status)),
                    );
                    details.insert(
                        "tx_hash".to_string(),
                        Value::String(SIMULATE_TRANSACTION_TESTNET_PROBE_TX_HASH.to_string()),
                    );
                    if let Some(fee) = parse_u64_field(&result, "minResourceFee") {
                        details.insert(
                            "min_resource_fee".to_string(),
                            Value::Number(serde_json::Number::from(fee)),
                        );
                    }
                    details.insert(
                        "results_count".to_string(),
                        Value::Number(serde_json::Number::from(results.len())),
                    );
                    if let Some(latest) = parse_u64_field(&result, "latestLedger") {
                        details.insert(
                            "latest_ledger".to_string(),
                            Value::Number(serde_json::Number::from(latest)),
                        );
                    }
                    if let Some(cost) = result.get("cost") {
                        if let Some(cpu) = parse_u64_field(cost, "cpuInsns") {
                            details.insert(
                                "cost_cpu_insns".to_string(),
                                Value::Number(serde_json::Number::from(cpu)),
                            );
                        }
                        if let Some(mem) = parse_u64_field(cost, "memBytes") {
                            details.insert(
                                "cost_mem_bytes".to_string(),
                                Value::Number(serde_json::Number::from(mem)),
                            );
                        }
                    }

                    CheckResult {
                        name: method.to_string(),
                        status: CheckStatus::Pass,
                        timestamp,
                        duration_ms: Some(duration_ms),
                        http_status: Some(info.status),
                        endpoint: endpoint.clone(),
                        details: Some(details),
                        error: None,
                        error_message: None,
                        evidence: Some(evidence),
                    }
                }
                Err(e) => fail_result(method, &endpoint, timestamp, &e, evidence),
            }
        })
    }
}

/// Configuration for the `contractTtl` check: which contract to check
/// and the Warn threshold in ledgers.
#[derive(Debug, Clone, Copy)]
pub struct TtlCheckConfig {
    /// The contract to check, as 32 raw ID bytes.
    pub contract_id: [u8; 32],
    /// Warn threshold in ledgers (inclusive).
    pub threshold: u64,
}

/// A built-in check for a contract's TTL (time-to-live) on the ledger.
///
/// Reads the contract's instance entry and its Wasm code entry via
/// `getLedgerEntries` and reports how many ledgers remain until each
/// expires: `liveUntilLedgerSeq` minus the latest ledger sequence
/// (resolved via `getLatestLedger`, like the other derived checks).
///
/// Key construction (both keys are base64 XDR, see
/// [`contract_instance_ledger_key`] / [`contract_code_ledger_key`]):
/// the instance key is `ContractData` for the contract address with the
/// `LedgerKeyContractInstance` key and `PERSISTENT` durability; the code
/// key is `ContractCode` for the Wasm hash taken from the instance
/// entry's executable. The Wasm hash is parsed out of the instance
/// entry's `xdr` value (see [`parse_instance_executable`]).
///
/// Verdicts: a missing instance entry is a `Fail` (the contract is
/// archived, expired, or never deployed); a missing code entry is a
/// `Fail` as well. A Stellar Asset Contract (or an external reference)
/// has no Wasm and therefore no code entry by design: the check then
/// reports `code_remaining_ledgers` as `null` and judges by the instance
/// entry alone. Remaining ledgers at or below `--ttl-threshold`
/// (default [`DEFAULT_TTL_THRESHOLD`]) report `Warn` — the boundary is
/// inclusive, consistent with the Warn tier: counted in `ran`, not in
/// `passed`, exit code unchanged.
///
/// The check only runs when a contract was supplied (`--contract`): with
/// no contract it reports `Skip`. It makes three RPC calls per run
/// (`getLatestLedger` + two `getLedgerEntries`), all read-only.
#[derive(Debug, Clone, Copy)]
pub struct ContractTtlCheck {
    /// The contract to check. `None` means no `--contract` was given:
    /// the check reports `Skip`.
    pub contract_id: Option<[u8; 32]>,
    /// Warn threshold in ledgers (inclusive).
    pub threshold: u64,
}

impl RpcCheck for ContractTtlCheck {
    fn name(&self) -> &str {
        "contractTtl"
    }

    fn run<'a>(
        &'a self,
        client: &'a RpcClient,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult> + Send + 'a>> {
        Box::pin(async move {
            let endpoint = client.sanitized_url();
            let timestamp = chrono::Utc::now().to_rfc3339();
            let method = "contractTtl";

            let contract_id = match self.contract_id {
                Some(id) => id,
                None => {
                    return CheckResult {
                        name: method.to_string(),
                        status: CheckStatus::Skip,
                        timestamp,
                        duration_ms: None,
                        http_status: None,
                        endpoint: endpoint.clone(),
                        details: None,
                        error: None,
                        error_message: None,
                        evidence: Some(
                            "No --contract given: pass --contract <C...> to check a contract's instance and code TTL."
                                .to_string(),
                        ),
                    };
                }
            };
            let contract_strkey = crate::network::encode_contract_id(&contract_id);

            // 1. Resolve the latest ledger sequence.
            let sequence = match client.call::<Value>("getLatestLedger", None).await {
                Ok((_, result)) => match parse_ledger_sequence(&result) {
                    Some(seq) => seq,
                    None => {
                        return fail_result(
                            method,
                            &endpoint,
                            timestamp,
                            &MiyoError::MalformedStructure(
                                "getLatestLedger result has no usable 'sequence' field".to_string(),
                            ),
                            "Call getLatestLedger to resolve the latest ledger for contractTtl."
                                .to_string(),
                        );
                    }
                },
                Err(e) => {
                    return fail_result(
                        method,
                        &endpoint,
                        timestamp,
                        &e,
                        "Call getLatestLedger to resolve the latest ledger for contractTtl before calling getLedgerEntries."
                            .to_string(),
                    );
                }
            };

            // 2. Fetch the instance entry.
            let instance_key = contract_instance_ledger_key(&contract_id);
            let instance_params = serde_json::json!({
                "keys": [instance_key],
            });
            let instance_evidence = format!(
                "Sent JSON-RPC 2.0 POST to {endpoint} with method 'getLedgerEntries' and params {{keys: [<LedgerKey ContractData for contract {contract_strkey}, instance, persistent>]}}."
            );
            let (info, result) = match client
                .call::<Value>("getLedgerEntries", Some(instance_params))
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    return fail_result(method, &endpoint, timestamp, &e, instance_evidence);
                }
            };
            let entries = match result.get("entries").and_then(Value::as_array) {
                Some(entries) => entries,
                None => {
                    return fail_result(
                        method,
                        &endpoint,
                        timestamp,
                        &MiyoError::MalformedStructure(
                            "getLedgerEntries result has no 'entries' array".to_string(),
                        ),
                        instance_evidence,
                    );
                }
            };
            let instance = entries
                .iter()
                .find(|e| e.get("key").and_then(Value::as_str) == Some(instance_key.as_str()));
            let instance = match instance {
                Some(entry) => entry,
                None => {
                    return fail_result(
                        method,
                        &endpoint,
                        timestamp,
                        &MiyoError::MissingField(format!(
                            "contract instance entry not found for {contract_strkey} (contract may be archived, expired, or never deployed)"
                        )),
                        instance_evidence,
                    );
                }
            };
            let instance_live = match parse_u64_field(instance, "liveUntilLedgerSeq") {
                Some(live) => live,
                None => {
                    return fail_result(
                        method,
                        &endpoint,
                        timestamp,
                        &MiyoError::MalformedStructure(
                            "contract instance entry has no usable 'liveUntilLedgerSeq' field"
                                .to_string(),
                        ),
                        instance_evidence,
                    );
                }
            };
            let instance_remaining = instance_live.saturating_sub(sequence);
            let instance_xdr = instance.get("xdr").and_then(Value::as_str).unwrap_or("");
            let executable = match parse_instance_executable(instance_xdr) {
                Some(executable) => executable,
                None => {
                    return fail_result(
                        method,
                        &endpoint,
                        timestamp,
                        &MiyoError::MalformedStructure(
                            "contract instance entry 'xdr' does not parse as a contract instance entry"
                                .to_string(),
                        ),
                        instance_evidence,
                    );
                }
            };

            // 3. Fetch the code entry — only Wasm executables have one.
            let mut duration_ms = info.duration.as_millis() as u64;
            let mut http_status = info.status;
            let (code_remaining, code_live, code_wasm_hash, code_executable) = match executable {
                InstanceExecutable::Wasm(wasm_hash) => {
                    let code_key = contract_code_ledger_key(&wasm_hash);
                    let code_params = serde_json::json!({
                        "keys": [code_key],
                    });
                    let code_evidence = format!(
                            "Sent JSON-RPC 2.0 POST to {endpoint} with method 'getLedgerEntries' and params {{keys: [<LedgerKey ContractCode for wasm hash {}>]}}.",
                            wasm_hash.iter().map(|b| format!("{b:02x}")).collect::<String>()
                        );
                    let (code_info, code_result) = match client
                        .call::<Value>("getLedgerEntries", Some(code_params))
                        .await
                    {
                        Ok(v) => v,
                        Err(e) => {
                            return fail_result(method, &endpoint, timestamp, &e, code_evidence);
                        }
                    };
                    duration_ms = code_info.duration.as_millis() as u64;
                    http_status = code_info.status;
                    let code_entries = match code_result.get("entries").and_then(Value::as_array) {
                        Some(entries) => entries,
                        None => {
                            return fail_result(
                                method,
                                &endpoint,
                                timestamp,
                                &MiyoError::MalformedStructure(
                                    "getLedgerEntries result has no 'entries' array".to_string(),
                                ),
                                code_evidence,
                            );
                        }
                    };
                    let code = code_entries
                        .iter()
                        .find(|e| e.get("key").and_then(Value::as_str) == Some(code_key.as_str()));
                    let code = match code {
                        Some(entry) => entry,
                        None => {
                            return fail_result(
                                method,
                                &endpoint,
                                timestamp,
                                &MiyoError::MissingField(format!(
                                    "contract code entry not found for wasm hash {}",
                                    wasm_hash
                                        .iter()
                                        .map(|b| format!("{b:02x}"))
                                        .collect::<String>()
                                )),
                                code_evidence,
                            );
                        }
                    };
                    let code_live = match parse_u64_field(code, "liveUntilLedgerSeq") {
                        Some(live) => live,
                        None => {
                            return fail_result(
                                method,
                                &endpoint,
                                timestamp,
                                &MiyoError::MalformedStructure(
                                    "contract code entry has no usable 'liveUntilLedgerSeq' field"
                                        .to_string(),
                                ),
                                code_evidence,
                            );
                        }
                    };
                    (
                        Some(code_live.saturating_sub(sequence)),
                        Some(code_live),
                        Some(
                            wasm_hash
                                .iter()
                                .map(|b| format!("{b:02x}"))
                                .collect::<String>(),
                        ),
                        "wasm",
                    )
                }
                InstanceExecutable::StellarAsset => (None, None, None, "stellar_asset"),
                InstanceExecutable::ExternalRef => (None, None, None, "external_ref"),
            };

            // 4. Verdict: the smallest remaining value decides; the
            // threshold boundary is inclusive (remaining <= threshold
            // warns), consistent with the Warn tier.
            let min_remaining = [Some(instance_remaining), code_remaining]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(u64::MAX);
            let status = if min_remaining <= self.threshold {
                CheckStatus::Warn
            } else {
                CheckStatus::Pass
            };

            let mut details = serde_json::Map::new();
            details.insert(
                "description".to_string(),
                Value::String(
                    "Contract instance and code TTL (remaining ledgers until expiry).".to_string(),
                ),
            );
            details.insert(
                "http_status".to_string(),
                Value::Number(serde_json::Number::from(http_status)),
            );
            details.insert(
                "contract".to_string(),
                Value::String(contract_strkey.clone()),
            );
            details.insert(
                "latest_ledger".to_string(),
                Value::Number(serde_json::Number::from(sequence)),
            );
            details.insert(
                "ttl_threshold".to_string(),
                Value::Number(serde_json::Number::from(self.threshold)),
            );
            details.insert(
                "instance_live_ledger".to_string(),
                Value::Number(serde_json::Number::from(instance_live)),
            );
            details.insert(
                "instance_remaining_ledgers".to_string(),
                Value::Number(serde_json::Number::from(instance_remaining)),
            );
            details.insert(
                "code_executable".to_string(),
                Value::String(code_executable.to_string()),
            );
            match (code_live, code_remaining) {
                (Some(live), Some(remaining)) => {
                    details.insert(
                        "code_live_ledger".to_string(),
                        Value::Number(serde_json::Number::from(live)),
                    );
                    details.insert(
                        "code_remaining_ledgers".to_string(),
                        Value::Number(serde_json::Number::from(remaining)),
                    );
                }
                _ => {
                    details.insert("code_remaining_ledgers".to_string(), Value::Null);
                }
            }
            if let Some(hash) = &code_wasm_hash {
                details.insert("code_wasm_hash".to_string(), Value::String(hash.clone()));
            }

            let evidence = match (code_live, code_remaining) {
                (Some(live), Some(remaining)) => format!(
                    "Checked contract {contract_strkey} TTL: getLatestLedger (latest {sequence}), getLedgerEntries with LedgerKey ContractData (instance, persistent; liveUntilLedgerSeq {instance_live}, {instance_remaining} ledgers remaining) and LedgerKey ContractCode (liveUntilLedgerSeq {live}, {remaining} ledgers remaining)."
                ),
                _ => format!(
                    "Checked contract {contract_strkey} TTL: getLatestLedger (latest {sequence}), getLedgerEntries with LedgerKey ContractData (instance, persistent; liveUntilLedgerSeq {instance_live}, {instance_remaining} ledgers remaining); no ContractCode entry exists for a {code_executable} executable, judged by the instance entry alone."
                ),
            };

            CheckResult {
                name: method.to_string(),
                status,
                timestamp,
                duration_ms: Some(duration_ms),
                http_status: Some(http_status),
                endpoint: endpoint.clone(),
                details: Some(details),
                error: None,
                error_message: None,
                evidence: Some(evidence),
            }
        })
    }
}

/// Check raw HTTP connectivity and status code — independent of JSON-RPC.
///
/// `follow_redirects` mirrors the `RpcClient` policy: follow by default, but
/// when false (the `--no-follow-redirects` flag) a 3xx response is reported
/// as a redirect failure instead of being followed to a different host.
pub async fn http_connectivity_check(
    url: &str,
    timeout: Duration,
    user_agent: &str,
    follow_redirects: bool,
) -> CheckResult {
    let endpoint = sanitize_url(url);
    let timestamp = chrono::Utc::now().to_rfc3339();
    let method = "httpConnectivity";

    // We use a plain reqwest client here (not RpcClient) to check raw HTTP.
    let client = match reqwest::Client::builder()
        .timeout(timeout)
        .redirect(if follow_redirects {
            reqwest::redirect::Policy::default()
        } else {
            reqwest::redirect::Policy::none()
        })
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

            // With redirect following disabled, a 3xx is a failure, not a
            // silently-followed hop: the probe never reached the endpoint
            // itself, so it cannot vouch for the endpoint's reachability.
            if !follow_redirects && (300..400).contains(&status) {
                let location = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("<no Location header>")
                    .to_string();

                let mut details = serde_json::Map::new();
                details.insert("endpoint_reachable".to_string(), Value::Bool(true));
                details.insert(
                    "http_status".to_string(),
                    Value::Number(serde_json::Number::from(status)),
                );
                details.insert(
                    "redirect_location".to_string(),
                    Value::String(location.clone()),
                );

                return CheckResult {
                    name: method.to_string(),
                    status: CheckStatus::Fail,
                    timestamp,
                    duration_ms: Some(duration.as_millis() as u64),
                    http_status: Some(status),
                    endpoint: endpoint.clone(),
                    details: Some(details),
                    error: Some(IssueCategory::HttpError),
                    error_message: Some(format!(
                        "HTTP {status} — redirect not followed (--no-follow-redirects)"
                    )),
                    evidence: Some(format!(
                        "Issued HTTP GET to {endpoint} with timeout {timeout:?} and User-Agent '{user_agent}'; server answered HTTP {status} with Location: {location} — redirect not followed because --no-follow-redirects is set."
                    )),
                };
            }

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

    // Every failure carries its provider error shape in `details`
    // (see `classify_error_shape`): the coarse `IssueCategory` says
    // *what kind* of failure it is, the shape says *how* the provider
    // failed. For a rate-limited response, the server-supplied
    // `Retry-After` is also surfaced verbatim plus its numeric seconds
    // form when it parses as an integer. Nothing is synthesised when
    // the header is absent or non-numeric.
    let mut details = serde_json::Map::new();
    details.insert(
        "error_shape".to_string(),
        Value::String(crate::error::classify_error_shape(err).to_string()),
    );
    if let MiyoError::HttpError {
        status: 429,
        retry_after: Some(raw),
        ..
    } = err
    {
        details.insert("retry_after".to_string(), Value::String(raw.clone()));
        if let Ok(secs) = raw.trim().parse::<u64>() {
            details.insert("retry_after_secs".to_string(), Value::from(secs));
        }
    }

    CheckResult {
        name: name.to_string(),
        status: CheckStatus::Fail,
        timestamp,
        duration_ms: None,
        http_status: None,
        endpoint: endpoint.to_string(),
        details: Some(details),
        error: Some(category),
        error_message: Some(message),
        evidence: Some(evidence),
    }
}

/// All built-in checks that the `check` command runs by default.
///
/// Four are no-parameter method checks; `getEvents`, `getLedgers`, and
/// `getTransactions` derive their own `startLedger` from `getLatestLedger`
/// and fetch one page each (see [`GetEventsCheck`], [`GetLedgersCheck`],
/// and [`GetTransactionsCheck`]); `getLedgerEntries` probes a fixed
/// synthetic key (see [`GetLedgerEntriesCheck`]); `getFeeStats` sends no
/// params (see [`GetFeeStatsCheck`]); `getTransaction` fetches the first
/// transaction of the latest ledger by hash (see [`GetTransactionCheck`]);
/// `simulateTransaction` replays an embedded Testnet probe transaction
/// (see [`SimulateTransactionCheck`]).
///
/// `contractTtl` is NOT in this list: it only runs when a contract was
/// supplied (`--contract`) or explicitly requested (`--method
/// contractTtl`) — see [`all_checks_with_ttl`]. Default runs therefore
/// keep exactly the 12 checks they had before the TTL check existed.
pub fn all_checks() -> Vec<Box<dyn RpcCheck + Send + Sync>> {
    vec![
        Box::new(SimpleMethodCheck {
            method_name: "getHealth",
            description: "Node health status and ledger window bounds.",
            primary_field: Some("status"),
        }),
        Box::new(SimpleMethodCheck {
            method_name: "getNetwork",
            description: "Network passphrase and protocol version.",
            primary_field: Some("passphrase"),
        }),
        Box::new(SimpleMethodCheck {
            method_name: "getLatestLedger",
            description: "Latest known ledger sequence, hash, and close time.",
            primary_field: Some("sequence"),
        }),
        Box::new(SimpleMethodCheck {
            method_name: "getVersionInfo",
            description: "RPC server version and Captive Core version.",
            primary_field: Some("version"),
        }),
        Box::new(GetEventsCheck),
        Box::new(GetLedgerEntriesCheck),
        Box::new(GetLedgersCheck),
        Box::new(GetTransactionsCheck),
        Box::new(GetFeeStatsCheck),
        Box::new(GetTransactionCheck),
        Box::new(SimulateTransactionCheck),
    ]
}

/// The default check list plus the `contractTtl` check, when it applies.
///
/// The TTL check is appended when a contract was supplied (`ttl` is
/// `Some`) or when it was explicitly requested (`method_filter` is
/// `Some("contractTtl")` — in which case it runs unconfigured and
/// reports `Skip`). Otherwise the list is exactly [`all_checks`], so
/// default runs keep their 12 checks.
pub fn all_checks_with_ttl(
    ttl: Option<TtlCheckConfig>,
    method_filter: Option<&str>,
) -> Vec<Box<dyn RpcCheck + Send + Sync>> {
    let mut checks = all_checks();
    if ttl.is_some() || method_filter == Some("contractTtl") {
        checks.push(Box::new(ContractTtlCheck {
            contract_id: ttl.map(|t| t.contract_id),
            threshold: ttl.map(|t| t.threshold).unwrap_or(DEFAULT_TTL_THRESHOLD),
        }));
    }
    checks
}

/// Names of the methods that have a dedicated built-in check.
pub const BUILTIN_METHODS: &[&str] = &[
    "getHealth",
    "getNetwork",
    "getLatestLedger",
    "getVersionInfo",
    "getEvents",
    "getLedgerEntries",
    "getLedgers",
    "getTransactions",
    "getFeeStats",
    "getTransaction",
    "simulateTransaction",
    "contractTtl",
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
        // The probe is always paired with the internal `httpConnectivity` check.
        // If the user's method name matches it, suffix the report name so
        // `aggregate()` (which matches by name) keeps both checks separate.
        if self.method_name == "httpConnectivity" {
            "httpConnectivity (probe)"
        } else {
            &self.method_name
        }
    }

    fn run<'a>(
        &'a self,
        client: &'a RpcClient,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult> + Send + 'a>> {
        // Use a distinct report name when the probe method would otherwise
        // collide with the internal `httpConnectivity` check that always runs
        // alongside it. The wire request still uses the original method name.
        let report_name = if self.method_name == "httpConnectivity" {
            "httpConnectivity (probe)".to_string()
        } else {
            self.method_name.clone()
        };
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
                        name: report_name.clone(),
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
                    &report_name,
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
        Self::run_all_with_config(url, RpcClientConfig::new(timeout), method_filter, runs).await
    }

    /// Run all diagnostic checks with an explicit [`RpcClientConfig`].
    ///
    /// Same as [`run_all`](Self::run_all) but honors the config's
    /// `user_agent` and `max_response_bytes` overrides (used by the
    /// `--user-agent` / `--max-response-bytes` CLI flags).
    pub async fn run_all_with_config(
        url: &str,
        config: RpcClientConfig,
        method_filter: Option<&str>,
        runs: usize,
    ) -> Self {
        Self::run_all_with_config_and_contract(url, config, method_filter, runs, None).await
    }

    /// Run all diagnostic checks with an explicit [`RpcClientConfig`] and
    /// an optional TTL check configuration.
    ///
    /// Same as [`run_all_with_config`](Self::run_all_with_config), but
    /// when `ttl` is `Some` the `contractTtl` check runs against that
    /// contract (see [`all_checks_with_ttl`]).
    pub async fn run_all_with_config_and_contract(
        url: &str,
        config: RpcClientConfig,
        method_filter: Option<&str>,
        runs: usize,
        ttl: Option<TtlCheckConfig>,
    ) -> Self {
        let endpoint = sanitize_url(url);
        let timeout = config.timeout;
        let user_agent = config.user_agent.clone();
        let follow_redirects = config.follow_redirects;
        let client = match RpcClient::new(url, config) {
            Ok(c) => c,
            Err(e) => {
                // Can't even build a client — every check fails.
                let timestamp = chrono::Utc::now().to_rfc3339();

                let mut checks: Vec<CheckResult> = all_checks_with_ttl(ttl, method_filter)
                    .iter()
                    .map(|c| {
                        fail_result(
                            c.name(),
                            &endpoint,
                            timestamp.clone(),
                            &e,
                            format!("Build RpcClient for {endpoint}."),
                        )
                    })
                    .collect();

                // Also add HTTP connectivity as a check.
                checks.insert(
                    0,
                    http_connectivity_check(url, timeout, &user_agent, follow_redirects).await,
                );

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

        // Run diagnostics `runs` times, collecting raw results for aggregation.
        let mut per_run: Vec<Vec<CheckResult>> = Vec::with_capacity(runs);
        for _ in 0..runs {
            let mut run_results: Vec<CheckResult> =
                vec![http_connectivity_check(url, timeout, &user_agent, follow_redirects).await];

            let checks = all_checks_with_ttl(ttl, method_filter);
            for check in &checks {
                if let Some(m) = method_filter {
                    if m != check.name() {
                        run_results.push(CheckResult {
                            name: check.name().to_string(),
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
        Self::run_arbitrary_probe_with_config(
            url,
            RpcClientConfig::new(timeout),
            method,
            params,
            runs,
        )
        .await
    }

    /// Run a single arbitrary-method probe with an explicit
    /// [`RpcClientConfig`].
    ///
    /// Same as [`run_arbitrary_probe`](Self::run_arbitrary_probe) but honors
    /// the config's `user_agent` and `max_response_bytes` overrides.
    pub async fn run_arbitrary_probe_with_config(
        url: &str,
        config: RpcClientConfig,
        method: &str,
        params: Option<Value>,
        runs: usize,
    ) -> Self {
        // Guard against a false "success" on an empty report: a probe must run
        // at least once. The CLI already clamps to [1, 10]; this keeps the
        // library API safe for any caller.
        let runs = runs.max(1);
        let endpoint = sanitize_url(url);
        let timeout = config.timeout;
        let user_agent = config.user_agent.clone();
        let follow_redirects = config.follow_redirects;
        let client = match RpcClient::new(url, config) {
            Ok(c) => c,
            Err(e) => {
                let timestamp = chrono::Utc::now().to_rfc3339();
                let mut checks: Vec<CheckResult> = vec![
                    http_connectivity_check(url, timeout, &user_agent, follow_redirects).await,
                ];
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

        let mut per_run: Vec<Vec<CheckResult>> = Vec::with_capacity(runs);
        for _ in 0..runs {
            let mut run_results =
                vec![http_connectivity_check(url, timeout, &user_agent, follow_redirects).await];
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
        warned: 0,
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
            CheckStatus::Warn => {
                s.ran += 1;
                s.warned += 1;
            }
            CheckStatus::Skip => s.skipped += 1,
        }
    }
    s
}

/// Aggregate results from multiple runs into a single summary per check.
///
/// The status of each check is the "worst" across all runs (Fail > Warn > Skip > Pass),
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

        // Worst status wins (Fail > Warn > Skip > Pass).
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
///
/// When at least two runs were performed and the ledger is stagnant or
/// inconsistent, the aggregated `getLatestLedger` check is downgraded from
/// `Pass` to [`CheckStatus::Warn`] so a frozen endpoint no longer looks
/// healthy in CI. The downgrade only applies to a check that aggregated to
/// `Pass` (a `Fail` from any run stays `Fail`; a `Skip` stays `Skip`), and
/// a single run stays inconclusive (no false Warn). Warn does not change
/// the exit code — only `Fail` exits `1`.
fn analyze_ledger_health(per_run: &[Vec<CheckResult>], checks: &mut [CheckResult]) {
    let ledger = match checks.iter_mut().find(|c| c.name == "getLatestLedger") {
        Some(c) => c,
        None => return,
    };

    // Collect sequence + hash from every run that produced a result.
    // Stellar RPC serializes the sequence as a JSON string in some
    // responses and as a number in others — live endpoints return a
    // number — so accept both, normalizing to a string for comparison.
    // (Reading only the string form silently dropped every live run,
    // which made this analysis a no-op on real endpoints.)
    let mut seqs: Vec<String> = Vec::new();
    let mut hashes: Vec<String> = Vec::new();
    for run in per_run {
        if let Some(lr) = run.iter().find(|c| c.name == "getLatestLedger") {
            if let Some(details) = &lr.details {
                let seq = match details.get("sequence") {
                    Some(Value::String(s)) => s.parse::<u64>().ok(),
                    Some(Value::Number(n)) => n.as_u64(),
                    _ => None,
                };
                if let Some(seq) = seq {
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
    // Whether the aggregated check is downgraded Pass -> Warn: only for a
    // stagnant or inconsistent ledger seen across at least two runs.
    // A single run is inconclusive; an advancing ledger is healthy.
    let (verdict, downgrade_to_warn) = if seqs.len() == 1 {
        let hash = hashes.first().and_then(|h| h.get(..8)).unwrap_or("?");
        (
            format!(
                " Ledger sequence {}, hash {}: self-run freshness inconclusive — use --runs > 1.",
                seqs[0], hash
            ),
            false,
        )
    } else {
        let unique_seqs: std::collections::HashSet<&str> =
            seqs.iter().map(String::as_str).collect();
        let unique_hashes: std::collections::HashSet<&str> =
            hashes.iter().map(String::as_str).collect();
        if unique_seqs.len() > 1 {
            (
                format!(
                    " Ledger advanced from {} to {} across {} runs — healthy.",
                    seqs.first().unwrap(),
                    seqs.last().unwrap(),
                    seqs.len()
                ),
                false,
            )
        } else if unique_hashes.len() <= 1 {
            // Same sequence across runs, and every observed hash agrees
            // (or no hash was observed at all — the hash comparison is
            // unavailable, not a contradiction): possibly stale.
            let hash = hashes.first().and_then(|h| h.get(..8)).unwrap_or("?");
            (
                format!(
                    " Ledger sequence {} (hash {}:) unchanged across {} consecutive runs — possibly stale endpoint.",
                    seqs[0], hash, seqs.len()
                ),
                true,
            )
        } else {
            (
                format!(
                    " Ledger sequence {} stable but hash changed across {} runs — possible ledger inconsistency.",
                    seqs[0], seqs.len()
                ),
                true,
            )
        }
    };
    if downgrade_to_warn && ledger.status == CheckStatus::Pass {
        ledger.status = CheckStatus::Warn;
    }
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
        // Sequence is a JSON string in some responses and a number in
        // others (live endpoints return a number); accept both, as in
        // `analyze_ledger_health`.
        let seq_of = |d: &serde_json::Map<String, Value>| -> Option<String> {
            match d.get("sequence") {
                Some(Value::String(s)) => s.parse::<u64>().ok().map(|n| n.to_string()),
                Some(Value::Number(n)) => n.as_u64().map(|n| n.to_string()),
                _ => None,
            }
        };
        let p_seq = seq_of(p);
        let o_seq = seq_of(o);
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
        assert_eq!(
            worst_status(&[CheckStatus::Warn, CheckStatus::Fail]),
            CheckStatus::Fail
        );
        // Warn beats Skip and Pass (but not Fail)
        assert_eq!(
            worst_status(&[CheckStatus::Pass, CheckStatus::Warn]),
            CheckStatus::Warn
        );
        assert_eq!(
            worst_status(&[CheckStatus::Skip, CheckStatus::Warn]),
            CheckStatus::Warn
        );
        // Skip beats Pass (but not Warn or Fail)
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
            worst_status(&[CheckStatus::Warn, CheckStatus::Warn]),
            CheckStatus::Warn
        );
        assert_eq!(
            worst_status(&[CheckStatus::Fail, CheckStatus::Fail]),
            CheckStatus::Fail
        );
    }

    #[test]
    fn test_aggregate_warn_beats_skip_and_pass() {
        let per_run = vec![
            vec![make_result("x", CheckStatus::Pass, Some(100))],
            vec![make_result("x", CheckStatus::Warn, Some(100))],
            vec![make_result("x", CheckStatus::Skip, None)],
        ];
        let result = aggregate(&per_run);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, CheckStatus::Warn);
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

    #[test]
    fn test_summarize_counts_warned_in_ran_not_passed() {
        let checks = vec![
            make_result("a", CheckStatus::Pass, Some(100)),
            make_result("b", CheckStatus::Warn, Some(100)),
            make_result("c", CheckStatus::Fail, None),
            make_result("d", CheckStatus::Skip, None),
        ];
        let summary = summarize(&checks, 2);
        assert_eq!(summary.ran, 3);
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.warned, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.total_runs, 2);
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
        ledger_check_from(per_run)
            .and_then(|c| c.evidence.clone())
            .unwrap_or_default()
    }

    /// Same as [`ledger_evidence_from`] but returns the aggregated
    /// `getLatestLedger` check itself, so tests can assert its status.
    fn ledger_check_from(per_run: &[Vec<CheckResult>]) -> Option<CheckResult> {
        let mut checks = aggregate(per_run);
        analyze_ledger_health(per_run, &mut checks);
        checks.into_iter().find(|c| c.name == "getLatestLedger")
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
        assert_eq!(
            ledger_check_from(&per_run).map(|c| c.status),
            Some(CheckStatus::Pass),
            "a single run must not Warn"
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
        assert_eq!(
            ledger_check_from(&per_run).map(|c| c.status),
            Some(CheckStatus::Pass),
            "an advancing ledger stays Pass"
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
        assert_eq!(
            ledger_check_from(&per_run).map(|c| c.status),
            Some(CheckStatus::Warn),
            "a stagnant ledger across runs must Warn, not Pass"
        );
    }

    #[test]
    fn test_ledger_health_inconsistent_hash() {
        let per_run = vec![
            vec![make_ledger_result("100", "aaa")],
            vec![make_ledger_result("100", "bbb")],
        ];
        let evidence = ledger_evidence_from(&per_run);
        assert!(evidence.contains("inconsistency"), "evidence: {evidence}");
        assert_eq!(
            ledger_check_from(&per_run).map(|c| c.status),
            Some(CheckStatus::Warn),
            "an inconsistent ledger across runs must Warn, not Pass"
        );
    }

    #[test]
    fn test_ledger_health_numeric_sequence_stagnant_warns() {
        // Live endpoints return `sequence` as a JSON number, not a
        // string; the analysis must accept both forms (a string-only
        // read silently dropped every live run).
        fn numeric_result(seq: u64, hash: &str) -> CheckResult {
            let mut details = serde_json::Map::new();
            details.insert("sequence".to_string(), Value::from(seq));
            details.insert("id".to_string(), Value::String(hash.to_string()));
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
        let per_run = vec![
            vec![numeric_result(100, "deadbeef")],
            vec![numeric_result(100, "deadbeef")],
        ];
        let check = ledger_check_from(&per_run).expect("getLatestLedger present");
        assert_eq!(
            check.status,
            CheckStatus::Warn,
            "a stagnant numeric-sequence ledger must Warn"
        );
        assert!(
            check
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains("possibly stale"),
            "evidence: {:?}",
            check.evidence
        );
    }

    #[test]
    fn test_ledger_health_failed_check_stays_failed() {
        // A getLatestLedger that Failed in one run must stay Fail after
        // the health analysis — Warn never overwrites a failure.
        let mut failed = make_ledger_result("100", "deadbeef");
        failed.status = CheckStatus::Fail;
        let per_run = vec![vec![make_ledger_result("100", "deadbeef")], vec![failed]];
        assert_eq!(
            ledger_check_from(&per_run).map(|c| c.status),
            Some(CheckStatus::Fail),
            "a failed ledger check must stay Fail"
        );
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
                warned: 0,
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
            "getEvents",
            "getLedgerEntries",
            "getLedgers",
            "getTransactions",
            "getFeeStats",
            "getTransaction",
            "simulateTransaction",
        ] {
            assert!(is_builtin_method(m), "{m} should be built-in");
        }
        for m in ["sendTransaction", ""] {
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

    #[test]
    fn test_parse_ledger_sequence_string_and_number() {
        // Stellar RPC serializes the sequence as a string in some responses
        // and as a number in others — both must resolve.
        assert_eq!(
            parse_ledger_sequence(&serde_json::json!({"sequence": "2002985"})),
            Some(2002985)
        );
        assert_eq!(
            parse_ledger_sequence(&serde_json::json!({"sequence": 2002985})),
            Some(2002985)
        );
        // Missing, wrong-typed, or unparseable sequences are rejected.
        assert_eq!(parse_ledger_sequence(&serde_json::json!({})), None);
        assert_eq!(
            parse_ledger_sequence(&serde_json::json!({"sequence": true})),
            None
        );
        assert_eq!(
            parse_ledger_sequence(&serde_json::json!({"sequence": "not-a-number"})),
            None
        );
        assert_eq!(
            parse_ledger_sequence(&serde_json::json!({"sequence": -1})),
            None
        );
    }

    #[test]
    fn test_base64_round_trip() {
        for bytes in [
            vec![],
            vec![0u8],
            vec![0xff; 32],
            (0u8..=255).collect::<Vec<u8>>(),
        ] {
            assert_eq!(base64_decode(&base64_encode(&bytes)).unwrap(), bytes);
        }
        assert_eq!(base64_encode(&[0xfb, 0xff, 0xfe]), "+//+");
        assert_eq!(base64_encode(&[0xfb, 0xff]), "+/8=");
        assert_eq!(base64_encode(&[0xfb]), "+w==");
        assert!(base64_decode("not!base64").is_none());
        assert!(base64_decode("AB=C").is_none());
    }

    /// The native XLM Stellar Asset Contract on Testnet, verified live
    /// 2026-11-27. Its instance key is the exact value the RPC echoed
    /// back for this contract's instance entry — an independent check of
    /// the key construction, not a self-comparison.
    const SAC_CONTRACT_ID: [u8; 32] = [
        0xd7, 0x92, 0x8b, 0x72, 0xc2, 0x70, 0x3c, 0xcf, 0xea, 0xf7, 0xeb, 0x9f, 0xf4, 0xef, 0x4d,
        0x50, 0x4a, 0x55, 0xa8, 0xb9, 0x79, 0xfc, 0x9b, 0x45, 0x0e, 0xa2, 0xc8, 0x42, 0xb4, 0xd1,
        0xce, 0x61,
    ];

    /// A real Wasm contract on Testnet, verified live 2026-11-27, with its
    /// Wasm hash from the instance entry's executable.
    const WASM_CONTRACT_ID: [u8; 32] = [
        0x64, 0xeb, 0x5e, 0x23, 0x0d, 0xb8, 0x1c, 0xea, 0xd8, 0x34, 0x2c, 0x96, 0xeb, 0xb4, 0xd7,
        0x02, 0x3b, 0xba, 0x6b, 0x04, 0xb9, 0x4f, 0xb6, 0x91, 0x03, 0x07, 0x39, 0x84, 0xc1, 0xfe,
        0x84, 0xd9,
    ];
    const WASM_HASH: [u8; 32] = [
        0xe3, 0x2b, 0xa3, 0xf3, 0xfd, 0x4a, 0x97, 0x32, 0x3a, 0x1a, 0xe5, 0x1b, 0x10, 0xc6, 0xc0,
        0x32, 0x6a, 0xfe, 0xd8, 0x50, 0xa9, 0x7d, 0x6b, 0x81, 0x1a, 0x50, 0xe8, 0xd2, 0x0e, 0x67,
        0x9c, 0xa9,
    ];

    #[test]
    fn test_contract_instance_ledger_key_matches_live_echo() {
        // The RPC echoed this exact key for the SAC's instance entry.
        assert_eq!(
            contract_instance_ledger_key(&SAC_CONTRACT_ID),
            "AAAABgAAAAHXkotywnA8z+r365/0701QSlWouXn8m0UOoshCtNHOYQAAABQAAAAB"
        );
        assert_eq!(
            contract_instance_ledger_key(&WASM_CONTRACT_ID),
            "AAAABgAAAAFk614jDbgc6tg0LJbrtNcCO7prBLlPtpEDBzmEwf6E2QAAABQAAAAB"
        );
    }

    #[test]
    fn test_contract_code_ledger_key() {
        assert_eq!(
            contract_code_ledger_key(&WASM_HASH),
            "AAAAB+Mro/P9SpcyOhrlGxDGwDJq/thQqX1rgRpQ6NIOZ5yp"
        );
    }

    /// Build an instance entry's `xdr` value from the documented layout
    /// (used as a fixture for the parser tests, not by the check itself).
    fn instance_entry_xdr(contract_id: &[u8; 32], executable: InstanceExecutable) -> String {
        let mut xdr = Vec::new();
        xdr.extend_from_slice(&6i32.to_be_bytes());
        xdr.extend_from_slice(&0i32.to_be_bytes());
        xdr.extend_from_slice(&1i32.to_be_bytes());
        xdr.extend_from_slice(contract_id);
        xdr.extend_from_slice(&20i32.to_be_bytes());
        xdr.extend_from_slice(&1i32.to_be_bytes());
        xdr.extend_from_slice(&19i32.to_be_bytes());
        match executable {
            InstanceExecutable::Wasm(hash) => {
                xdr.extend_from_slice(&0i32.to_be_bytes());
                xdr.extend_from_slice(&hash);
            }
            InstanceExecutable::StellarAsset => xdr.extend_from_slice(&1i32.to_be_bytes()),
            InstanceExecutable::ExternalRef => xdr.extend_from_slice(&2i32.to_be_bytes()),
        }
        base64_encode(&xdr)
    }

    #[test]
    fn test_parse_instance_executable() {
        assert_eq!(
            parse_instance_executable(&instance_entry_xdr(
                &WASM_CONTRACT_ID,
                InstanceExecutable::Wasm(WASM_HASH)
            )),
            Some(InstanceExecutable::Wasm(WASM_HASH))
        );
        assert_eq!(
            parse_instance_executable(&instance_entry_xdr(
                &SAC_CONTRACT_ID,
                InstanceExecutable::StellarAsset
            )),
            Some(InstanceExecutable::StellarAsset)
        );
        assert_eq!(
            parse_instance_executable(&instance_entry_xdr(
                &SAC_CONTRACT_ID,
                InstanceExecutable::ExternalRef
            )),
            Some(InstanceExecutable::ExternalRef)
        );
        // Wrong discriminant, truncated bytes, and non-base64 are rejected.
        assert_eq!(parse_instance_executable("AAAABw=="), None);
        assert_eq!(parse_instance_executable("AAAAAA=="), None);
        assert_eq!(parse_instance_executable("not-base64!"), None);
        assert_eq!(parse_instance_executable(""), None);
    }

    #[test]
    fn test_get_events_check_name_and_page_limit() {
        assert_eq!(GetEventsCheck.name(), "getEvents");
        assert_eq!(GET_EVENTS_PAGE_LIMIT, 10);
        assert!(is_builtin_method("getEvents"));
        assert_eq!(BUILTIN_METHODS.len(), 12);
        let checks = all_checks();
        let names: Vec<&str> = checks.iter().map(|c| c.name()).collect();
        assert_eq!(
            names,
            [
                "getHealth",
                "getNetwork",
                "getLatestLedger",
                "getVersionInfo",
                "getEvents",
                "getLedgerEntries",
                "getLedgers",
                "getTransactions",
                "getFeeStats",
                "getTransaction",
                "simulateTransaction"
            ]
        );
    }

    #[test]
    fn test_get_transactions_check_name_and_page_limit() {
        assert_eq!(GetTransactionsCheck.name(), "getTransactions");
        assert_eq!(GET_TRANSACTIONS_PAGE_LIMIT, 2);
        assert!(is_builtin_method("getTransactions"));
    }

    #[test]
    fn test_get_ledger_entries_and_get_ledgers_check_names_and_limits() {
        assert_eq!(GetLedgerEntriesCheck.name(), "getLedgerEntries");
        assert_eq!(GetLedgersCheck.name(), "getLedgers");
        assert_eq!(GET_LEDGERS_PAGE_LIMIT, 2);
        assert!(is_builtin_method("getLedgerEntries"));
        assert!(is_builtin_method("getLedgers"));
        // The probe key is the base64 XDR of an Account LedgerKey for the
        // all-zero ed25519 key: discriminant 0 (ACCOUNT), key type 0
        // (ED25519), 32 zero bytes — 40 bytes total.
        assert_eq!(
            GET_LEDGER_ENTRIES_PROBE_KEY,
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
        );
    }
}

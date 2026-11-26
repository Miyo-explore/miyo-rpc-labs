# Examples

← [Back to README](../README.md)

## Examples

### Human-readable output (single run)

```text
Miyo RPC Labs v0.5.0
Endpoint: https://soroban-testnet.stellar.org

  PASS httpConnectivity  (142ms)
    endpoint_reachable: true
    http_status: 200
    evidence: Issued HTTP GET to https://soroban-testnet.stellar.org with timeout 15s and User-Agent 'miyo-rpc/0.5.0'.

  PASS getHealth  (156ms)
    description: "Node health status and ledger window bounds."
    http_status: 200
    status: "healthy"
    evidence: Sent JSON-RPC 2.0 POST to https://soroban-testnet.stellar.org with method 'getHealth' and no params.

  PASS getNetwork  (168ms)
    description: "Network passphrase and protocol version."
    http_status: 200
    passphrase: "Public Global Stellarium..."
    evidence: Sent JSON-RPC 2.0 POST to https://soroban-testnet.stellar.org with method 'getNetwork' and no params.

  PASS getLatestLedger  (178ms)
    description: "Latest known ledger sequence, hash, and close time."
    http_status: 200
    sequence: 51583040
    evidence: Sent JSON-RPC 2.0 POST to https://soroban-testnet.stellar.org with method 'getLatestLedger' and no params.

  PASS getVersionInfo  (52ms)
    description: "RPC server version and Captive Core version."
    http_status: 200
    version: "2.0.0"
    evidence: Sent JSON-RPC 2.0 POST to https://soroban-testnet.stellar.org with method 'getVersionInfo' and no params.

  PASS getEvents  (61ms)
    description: "Contract events in the latest ledger (one page, no filters)."
    http_status: 200
    start_ledger: 51583040
    events_returned: 0
    has_cursor: false
    evidence: Sent JSON-RPC 2.0 POST to https://soroban-testnet.stellar.org with method 'getEvents' and params {startLedger: 51583040, pagination: {limit: 10}} (startLedger resolved from getLatestLedger; no filters).

  PASS getLedgerEntries  (58ms)
    description: "Ledger entries for a fixed probe key (read-only)."
    http_status: 200
    entries_returned: 1
    latest_ledger: 51583040
    evidence: Sent JSON-RPC 2.0 POST to https://soroban-testnet.stellar.org with method 'getLedgerEntries' and params {keys: [<probe LedgerKey>]} (fixed synthetic all-zero Account key; no account is assumed to exist).

  PASS getLedgers  (64ms)
    description: "Ledger headers starting at the latest ledger (one page)."
    http_status: 200
    start_ledger: 51583040
    ledgers_returned: 1
    latest_ledger: 51583040
    oldest_ledger: 51565760
    has_cursor: true
    evidence: Sent JSON-RPC 2.0 POST to https://soroban-testnet.stellar.org with method 'getLedgers' and params {startLedger: 51583040, pagination: {limit: 2}} (startLedger resolved from getLatestLedger).

  PASS getTransactions  (71ms)
    description: "Transactions in the latest ledger (one page)."
    http_status: 200
    start_ledger: 51583040
    transactions_returned: 1
    latest_ledger: 51583040
    oldest_ledger: 51565760
    has_cursor: true
    evidence: Sent JSON-RPC 2.0 POST to https://soroban-testnet.stellar.org with method 'getTransactions' and params {startLedger: 51583040, pagination: {limit: 2}} (startLedger resolved from getLatestLedger).

Summary: 9 passed, 0 failed, 0 skipped (9 ran)
```

### Human-readable output (repeated runs)

```text
Miyo RPC Labs v0.5.0
Endpoint: https://soroban-testnet.stellar.org

  PASS httpConnectivity  (270ms)
    endpoint_reachable: true
    http_status: 405
    latency_min_ms: 258
    latency_avg_ms: 270.0
    latency_max_ms: 282
    run_count: 3
    evidence: Issued HTTP GET to https://soroban-testnet.stellar.org with timeout 15s and User-Agent 'miyo-rpc/0.5.0'.

  PASS getHealth  (270ms)
    description: "Node health status and ledger window bounds."
    http_status: 200
    latency_min_ms: 261
    latency_avg_ms: 270.0
    latency_max_ms: 286
    run_count: 3
    status: "healthy"
    evidence: Sent JSON-RPC 2.0 POST to https://soroban-testnet.stellar.org with method 'getHealth' and no params.

  ...

Runs: 3 total
Summary: 8 passed, 0 failed, 0 skipped (8 ran)
```

### JSON output

All ten `CheckResult` fields are always present in JSON output. Optional
fields that are not applicable are serialized as `null` (the JSON formatter
does **not** use `skip_serializing_if`), so a `Pass` check still carries an
`evidence` string and `null` `error` / `error_message`. The `error` field is
serialized as the `IssueCategory` enum string: `Transport`, `HttpError`,
`MalformedResponse`, `JsonRpcError`, `UnsupportedMethod`, `MissingField`, or
`Other`.

The first key of every JSON report is `schema_version` (currently `"1.0"`). It is
bumped (major) only when the JSON shape changes non-backwards-compatibly (a
field removed, renamed, or its type changed). **Additive** additions — new
fields or new keys within `details` / `checks` — never bump it. Consumers MUST
read `schema_version` and MUST ignore fields they do not recognize.

```json
{
  "schema_version": "1.0",
  "tool_version": "0.5.0",
  "endpoint": "https://soroban-testnet.stellar.org",
  "runs": 1,
  "checks": [
    {
      "name": "httpConnectivity",
      "status": "pass",
      "timestamp": "2026-10-23T12:00:00+00:00",
      "duration_ms": 142,
      "http_status": 200,
      "endpoint": "https://soroban-testnet.stellar.org",
      "details": {
        "endpoint_reachable": true,
        "http_status": 200
      },
      "error": null,
      "error_message": null,
      "evidence": "Issued HTTP GET to https://soroban-testnet.stellar.org with timeout 15s and User-Agent 'miyo-rpc/0.5.0'."
    },
    {
      "name": "getVersionInfo",
      "status": "fail",
      "timestamp": "2026-10-23T12:00:00+00:00",
      "duration_ms": null,
      "http_status": null,
      "endpoint": "https://soroban-testnet.stellar.org",
      "details": null,
      "error": "UnsupportedMethod",
      "error_message": "JSON-RPC error -32601: method not found",
      "evidence": "Send a JSON-RPC 2.0 POST to https://soroban-testnet.stellar.org with method 'getVersionInfo' and empty params."
    }
  ],
  "summary": {
    "ran": 8,
    "passed": 7,
    "failed": 1,
    "skipped": 0,
    "total_runs": 1
  }
}
```

> **Note:** This example is abbreviated. The real report contains nine
> `checks` entries (`httpConnectivity` plus the eight JSON-RPC methods). Here,
> two are shown inline for illustration; `summary` reflects all nine: `ran`
> counts checks that executed (`passed` + `failed`), `skipped` counts checks
> not performed, and `total_runs` equals the top-level `runs` value.

### URL sanitization

Every URL that appears in output is sanitized before printing. The following
are redacted with the literal `REDACTED`:

- Any **userinfo** component (e.g. `https://user:password@host/...` becomes
  `https://REDACTED:REDACTED@host/...`).
- Any **query parameter** whose key (case-insensitive) matches a sensitive
  pattern, including `key`, `token`, `secret`, `password`, `pass`, `auth`,
  `authorization`, `apikey`, `api_key`, `access_token`, `refresh_token`,
  `private_key`, and `bearer`.

Example:

```text
Input:  https://user:s3cret@rpc.example.com/rpc?api_key=abc123&chain=testnet
Output: https://REDACTED:REDACTED@rpc.example.com/rpc?api_key=REDACTED&chain=testnet
```

**Never** publish credentials, API keys, or secret-bearing URLs. The
sanitization is a safety net, not a security control — treat any endpoint URL
as sensitive.

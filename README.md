# Miyo RPC Labs

Open-source Stellar JSON-RPC diagnostics, compatibility testing, and reproducible
endpoint verification toolkit.

## What It Does

Miyo RPC Labs helps developers diagnose Stellar RPC endpoints by running a
deterministic set of checks against any URL implementing the Stellar
JSON-RPC protocol (Soroban-compatible). It produces a structured report
(human-readable or JSON) that classifies which capabilities a given endpoint
supports, which return errors, and which could not be tested at all.

## Why

When a developer's tooling breaks against a particular Stellar RPC provider,
the root cause is often unclear:

- Is the endpoint down?
- Did the provider drop support for `getHealth`?
- Is the response malformed (HTTP 200 but invalid JSON-RPC)?
- Is this a timeout, or a JSON-RPC `-32601 method-not-found`?

Miyo RPC Labs separates these concerns by running **independent, single-
purpose checks** and classifying each outcome as:

- **Pass** — the check completed and the endpoint behaved as expected.
- **Fail** — the check completed but the endpoint returned an error, a
  non-success HTTP status, or malformed data.
- **Skip** — the check was not performed (e.g., filtered out by `--method`).

This avoids the common pitfall of interpreting HTTP 200 as "RPC is healthy"
or a single failed request as "the provider is down."

## Supported Checks

All checks are run by default. Use `--method <NAME>` to run a single check in
isolation.

| Check              | Method            | Description                                                        |
|--------------------|-------------------|--------------------------------------------------------------------|
| HTTP Connectivity  | (raw HTTP GET)    | Raw HTTP reachability, status code, and response latency.          |
| `getHealth`        | `getHealth`       | Node health status and ledger window bounds.                       |
| `getNetwork`       | `getNetwork`      | Network passphrase and protocol version.                           |
| `getLatestLedger`  | `getLatestLedger` | Latest known ledger sequence, hash, and close time.                |
| `getVersionInfo`   | `getVersionInfo`  | RPC server version and Captive Core version.                       |

### Status semantics

- **Pass** — the endpoint returned a well-formed response for this check. For
  HTTP connectivity, this means an HTTP status below 500. For JSON-RPC checks,
  it means the method was accepted (did not return `-32601 method-not-found`).
- **Fail** — the check ran but the endpoint returned an error: an HTTP 5xx
  status, a JSON-RPC error object, a malformed response, or a connection/
  timeout error.
- **Skip** — the check was not attempted (for example, because `--method`
  selected a different check).

### Limitations

- HTTP connectivity is measured with a plain `GET` request. Stellar JSON-RPC
  endpoints often return `405 Method Not Allowed` for `GET`; this is still
  reported as **Pass** because the HTTP endpoint was reachable.
- Only the four built-in no-parameter JSON-RPC methods are checked, plus the
  HTTP connectivity probe.
- JSON-RPC error codes are classified (`-32601`, `-32602`, `-32603`, etc.) but
  no provider-specific error taxonomy is applied.
- No archive pagination, cross-provider equivalence, or performance
  benchmarking is performed or claimed. Latency statistics describe the
  observed distribution across the configured runs, not a sustained
  performance benchmark.

## Installation

### Prerequisites

- Rust toolchain (stable; the `edition = "2021"` crate).
- macOS, Linux, or Windows (x86_64 or ARM64).

### From source

```bash
git clone https://github.com/Miyo-explore/miyo-rpc-labs.git
cd miyo-rpc-labs
cargo build --release
```

The binary will be at `target/release/miyo-rpc`.

## Usage

```text
miyo-rpc check --url <RPC_URL>                       # human-readable report (default)
miyo-rpc check --url <RPC_URL> --output json         # machine-readable JSON
miyo-rpc check --url <RPC_URL> --method getHealth    # one JSON-RPC check (httpConnectivity still runs)
miyo-rpc check --url <RPC_URL> --timeout-secs 10     # override the 15s default
miyo-rpc check --url <RPC_URL> --runs 5              # repeat 5× with latency stats
miyo-rpc --help
miyo-rpc --version
miyo-rpc check --help
```

### Flags

| Flag               | Short | Default   | Notes                                                                 |
|--------------------|-------|-----------|-----------------------------------------------------------------------|
| `--url <URL>`      | `-u`  | _required_ | The Stellar JSON-RPC endpoint URL to diagnose.                       |
| `--output <FMT>`   | `-o`  | `human`   | `human` (colored terminal) or `json`.                                |
| `--timeout-secs`   | `-t`  | `15`      | Maximum time to wait per HTTP request.                               |
| `--method <NAME>`  | `-m`  | _(none)_  | Run only the named JSON-RPC method check. The HTTP connectivity check (`httpConnectivity`) always runs regardless of this flag. |
| `--runs <N>`       | `-n`  | `1`       | Number of diagnostic runs to perform. Clamped to the range `[1, 10]` by the CLI. |
| `--help`           |       |           | Show help.                                                                                |
| `--version`        |       |           | Print version and exit.                                                                   |

> **`--method` behavior:** The HTTP connectivity check (`httpConnectivity`)
> always runs, regardless of `--method`. The flag only filters the
> **JSON-RPC method checks** (`getHealth`, `getNetwork`, `getLatestLedger`,
> `getVersionInfo`). Method checks that do not match are reported as `SKIP`
> with `evidence` set to `"Skipped: --method filter does not match this
> check."`.

### Exit codes

| Code | Meaning                                                              |
|------|----------------------------------------------------------------------|
| `0`  | All executed checks passed (no `Fail` results).                       |
| `1`  | One or more checks returned `Fail`.                                 |
| `2`  | A non-check error occurred (for example, the report could not be serialized to JSON). |

## Repeated Probes and Latency Statistics

By default the tool runs every check exactly once (`--runs 1`). Use
`--runs N` to repeat all checks and aggregate the results.

- **Default**: 1 run (single pass, no latency aggregation).
- **Maximum**: 10 runs. Values below 1 are raised to 1; values above 10 are
  clamped to 10. This clamping happens in the CLI layer, so the report always
  reflects the *effective* run count actually executed.
- **Aggregation**: across runs, the worst status wins using the precedence
  `Fail > Skip > Pass`. A single `Fail` in any run is never hidden by a `Skip`
  or `Pass` in another run.
- **Latency statistics** are emitted inside the `details` map of each check
  only when more than one run was requested **and** at least one run produced
  a reques...[truncated]

### Run-count semantics

- On a **successful** client initialization, the diagnostic loop executes the
  effective number of `runs`; `runs` and `summary.total_runs` both reflect the
  number of runs performed.
- On a **client-init** failure (e.g., an unsupported URL scheme such as
  `ftp://`), the tool cannot issue RPC calls at all. It runs a single
  connectivity attempt and reports `runs = 1` (and `summary.total_runs = 1`)
  rather than the requested `--runs` value. This ensures the report never
  claims more diagnostic runs than were actually performed.

## Examples

### Human-readable output (single run)

```text
Miyo RPC Labs v0.1.0
Endpoint: https://soroban-testnet.stellar.org

  PASS httpConnectivity  (142ms)
    endpoint_reachable: true
    http_status: 200
    evidence: Issued HTTP GET to https://soroban-testnet.stellar.org with timeout 15s and User-Agent 'miyo-rpc/0.1.0'.

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

Summary: 5 passed, 0 failed, 0 skipped (5 ran)
```

### Human-readable output (repeated runs)

```text
Miyo RPC Labs v0.1.0
Endpoint: https://soroban-testnet.stellar.org

  PASS httpConnectivity  (281ms)
    endpoint_reachable: true
    http_status: 405
    latency_min_ms: 258
    latency_avg_ms: 281.0
    latency_max_ms: 282
    run_count: 3
    evidence: Issued HTTP GET to https://soroban-testnet.stellar.org with timeout 15s and User-Agent 'miyo-rpc/0.1.0'.

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
Summary: 5 passed, 0 failed, 0 skipped (5 ran)
```

### JSON output

All ten `CheckResult` fields are always present in JSON output. Optional
fields that are not applicable are serialized as `null` (the JSON formatter
does **not** use `skip_serializing_if`), so a `Pass` check still carries an
`evidence` string and `null` `error` / `error_message`. The `error` field is
serialized as the `IssueCategory` enum string: `Transport`, `HttpError`,
`MalformedResponse`, `JsonRpcError`, `UnsupportedMethod`, `MissingField`, or
`Other`.

```json
{
  "tool_version": "0.1.0",
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
      "evidence": "Issued HTTP GET to https://soroban-testnet.stellar.org with timeout 15s and User-Agent 'miyo-rpc/0.1.0'."
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
    "ran": 5,
    "passed": 4,
    "failed": 1,
    "skipped": 0,
    "total_runs": 1
  }
}
```

> **Note:** This example is abbreviated. The real report contains five
> `checks` entries (`httpConnectivity` plus the four JSON-RPC methods). Here,
> two are shown inline for illustration; `summary` reflects all five: `ran`
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

## Testing

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

All deterministic unit and integration tests use a local mock RPC server
(`mockito`) and deterministic fixtures. No live Stellar endpoint access is
required to run the test suite.

### Live validation

Supplementary live checks against `https://soroban-testnet.stellar.org` can be
performed manually, but they are **not** part of CI. They are intended only for
manual verification of new releases and may be affected by network conditions.

## Reporting Issues

To report a reproducible compatibility issue:

1. Run `miyo-rpc check --url <YOUR_RPC_URL> --output json > report.json`.
2. Share the sanitized `report.json` and the **exact URL** (without secrets) in
   a GitHub issue.
3. Include the `tool_version` and the `total_runs` field from the report when
   relevant.

## License

Apache-2.0. See [LICENSE](LICENSE).

## Status

This project is early-stage. See [CHANGELOG.md](CHANGELOG.md) for progress.

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

- **Pass** — the method is supported and returned a well-formed response.
- **Fail** — the method is reachable but returns an error or malformed data.
- **Skip** — the check was intentionally not performed (e.g. filtered out by
  `--method`).

This avoids the common pitfall of interpreting HTTP 200 as "RPC is healthy"
or a single failed request as "the provider is down."

## Supported Checks

| Check              | Method          | Description                                                |
|--------------------|-----------------|------------------------------------------------------------|
| HTTP Connectivity  | —               | Raw HTTP reachability, status code, and response latency.  |
| Network Identity   | `getNetwork`    | Returns the network passphrase and protocol version.       |
| Health             | `getHealth`     | Returns health status and ledger retention window.         |
| Latest Ledger      | `getLatestLedger` | Returns the latest ledger sequence, hash, and close time.   |
| Version Info       | `getVersionInfo`  | Returns RPC server version and Captive Core version.     |

### Limitations

- Only the four no-parameter Stellar JSON-RPC methods are checked.
- JSON-RPC error codes are classified (`-32600`, `-32601`, `-32602`,
  `-32603`) but no provider-specific error taxonomy is applied.
- No archive pagination, cross-provider equivalence, or performance
  benchmarking is performed or claimed.

## Installation

### From source

```bash
git clone https://github.com/naninu123/miyo-rpc-labs.git
cd miyo-rpc-labs
cargo build --release
```

The binary will be at `target/release/miyo-rpc`.

## Usage

```
miyo-rpc check --url <RPC_URL>                    # human-readable report
miyo-rpc check --url <RPC_URL> --output json     # machine-readable JSON
miyo-rpc check --url <RPC_URL> --method getHealth      # single check
miyo-rpc check --url <RPC_URL> --timeout-secs 10       # 10s timeout
miyo-rpc check --url <RPC_URL> --runs 5                # repeat 5x with latency stats
miyo-rpc --help
miyo-rpc --version
```

### Examples

#### Human-readable output

```
Endpoint: https://stellar-testnet.stellar.org:8999
Tool: miyo-rpc v0.1.0
Run: 2025-01-01T12:00:00Z (3s total)

  httpConnectivity .... PASS   (200 OK, 142ms)
  getHealth ........... PASS   (200 OK, 156ms)
  getNetwork .......... PASS   (200 OK, 168ms, passphrase: "Public Global Stellarium...")
  getLatestLedger ..... PASS   (200 OK, 178ms, ledger: 51583040)
  getVersionInfo ...... FAIL   (JSON-RPC error: -32601 method not found)

Summary: 4 passed, 1 failed, 0 skipped
```

#### Human-readable output (repeated runs)

```
Endpoint: https://soroban-testnet.stellar.org
Tool: miyo-rpc v0.1.0
Runs: 3 total

  httpConnectivity .... PASS   (281ms avg, 258-286ms over 3 runs)
  getHealth ........... PASS   (262ms avg, 258-266ms over 3 runs)
  ...
```

#### JSON output

```json
{
  "tool_version": "0.1.0",
  "endpoint": "https://stellar-testnet.stellar.org:8999",
  "runs": 3,
  "run_started": "2025-01-01T12:00:00Z",
  "total_duration_ms": 3142,
  "checks": [
    {
      "name": "httpConnectivity",
      "status": "pass",
      "timestamp": "2025-01-01T12:00:00Z",
      "duration_ms": 142,
      "http_status": 200,
      "endpoint": "https://stellar-testnet.stellar.org:8999",
      "details": null,
      "error": null,
      "error_message": null,
      "evidence": null
    }
  ],
  "summary": {
    "passed": 4,
    "failed": 1,
    "skipped": 0
  }
}
```

**Note:** URLs in output are automatically sanitized — query parameters
matching secret-like patterns (e.g. `api_key`, `token`, `secret`) are
redacted before any output or log.

## Testing

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

All tests use a local mock RPC server (`mockito`) and deterministic fixtures.
No live Stellar endpoint access is required.

### Optional live tests

Live integration tests are not yet implemented. When added, they will be
gated behind a feature flag so they never run in ordinary CI.

## Reporting Issues

To report a reproducible compatibility issue:

1. Run `miyo-rpc check --url <YOUR_RPC_URL> --output json > report.json`.
2. Share the sanitized `report.json` and the **exact URL** (without
   secrets) in a GitHub issue.
3. Include the `tool_version` and `run_started` fields from the report.

## License

Apache-2.0. See [LICENSE](LICENSE).

## Status

This project is early-stage. See [CHANGELOG.md](CHANGELOG.md) for progress.

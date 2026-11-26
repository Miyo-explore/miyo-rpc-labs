# Checks

← [Back to README](../README.md)

## Supported Checks

All checks are run by default. Use `--method <NAME>` to run a single JSON-RPC
method check in isolation (the HTTP connectivity check always runs regardless).

| Check              | Method            | Description                                                        |
|--------------------|-------------------|--------------------------------------------------------------------|
| HTTP Connectivity  | (raw HTTP GET)    | Raw HTTP reachability, status code, and response latency.          |
| `getHealth`        | `getHealth`       | Node health status and ledger window bounds.                       |
| `getNetwork`       | `getNetwork`      | Network passphrase and protocol version.                           |
| `getLatestLedger`  | `getLatestLedger` | Latest known ledger sequence, hash, and close time.                |
| `getVersionInfo`   | `getVersionInfo`  | RPC server version and Captive Core version.                       |
| `getEvents`        | `getEvents`       | Contract events in the latest ledger (one page of 10, no filters).|
| `getLedgerEntries` | `getLedgerEntries`| Ledger entries for a fixed probe key (one synthetic Account key).  |
| `getLedgers`       | `getLedgers`      | Ledger headers from the latest ledger (one page of 2).             |
| `getTransactions`  | `getTransactions` | Transactions in the latest ledger (one page of 2).                 |
| `getFeeStats`      | `getFeeStats`     | Fee statistics for recent ledgers (inclusion fee percentiles).     |
| `getTransaction`   | `getTransaction`  | Single transaction by hash (first transaction in the latest ledger).|
| `simulateTransaction` | `simulateTransaction` | Simulates a real InvokeHostFunction transaction (Testnet probe, replayed; Skips on other networks). |

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
- HTTP `429 Too Many Requests` is classified as **`rate_limited`** (distinct
  from other 4xx client errors) and the response's `Retry-After` header is
  surfaced in `details` as `retry_after` (raw value) and, when it parses as an
  integer, `retry_after_secs`. The CLI **does not retry** — no automatic retry,
  backoff, or throttling logic exists. Wait for the indicated interval and run
  the command again yourself.
- `--no-follow-redirects` applies to the JSON-RPC method checks and to the
  `httpConnectivity` probe: with the flag set, a 3xx response is reported as
  a redirect failure (with the `Location` in `details`) instead of being
  followed to a different host.
- Five built-in methods take no parameters. `getEvents` derives its own
  `startLedger` from `getLatestLedger` (the latest ledger, always inside the
  retention window) and fetches a single page of 10 events with no filters;
  the response `cursor` is reported as `has_cursor` but never followed.
  `getLedgers` does the same for ledger headers with a single page of 2.
  `getTransactions` does the same for transactions with a single page of 2
  (server default `includeFailed` applies — failed transactions included).
  `getTransaction` fetches the same one-page result and calls
  `getTransaction` with the first returned hash; an empty page is a Skip
  (there is no transaction to fetch), not a Fail. `getFeeStats` sends no
  params and surfaces the Soroban / classic inclusion-fee mode, p95, and
  transaction count plus `latestLedger` in `details` — the live response
  carries no `feeCharged` percentile and no `ledgerCapacityUsage` field.
  `getLedgerEntries` sends one fixed synthetic probe key — an Account
  `LedgerKey` for the all-zero ed25519 key, which no real wallet owns — and
  needs no `getLatestLedger` round-trip; an empty `entries` array is a Pass
  (the probe account simply does not exist on that network).
  `simulateTransaction` resolves the network via `getNetwork` and replays
  an embedded Testnet probe transaction (a real `InvokeHostFunction`
  transaction observed on Testnet; simulation ignores signatures and
  sequence numbers, so replay is valid). It Passes when the simulation
  returns results, reporting `min_resource_fee`, `results_count`, and
  `latestLedger`; a result-level `error` is a Fail (the probe invocation
  no longer simulates). On any network other than Testnet it Skips with
  evidence — the probe contract only exists on Testnet, so the method
  cannot be proven there without a native invocation. The HTTP
  connectivity probe runs in addition to the eleven method checks.
- JSON-RPC error codes are classified (`-32601`, `-32602`, `-32603`, etc.) but
  no provider-specific error taxonomy is applied.
- No archive pagination, cross-provider equivalence, or performance
  benchmarking is performed or claimed. Latency statistics describe the
  observed distribution across the configured runs, not a sustained
  performance benchmark.

# Checks

← [Back to README](../README.md)

## Supported Checks

All checks are run by default, except `contractTtl`, which only runs
when a contract is given (`--contract <C...>`). Use `--method <NAME>` to run a single JSON-RPC
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
| `contractTtl` | `getLedgerEntries` | Remaining ledgers until a contract's instance and code entries expire. Only runs with `--contract <C...>`; Warns at/below `--ttl-threshold` (default 17280). |

### Status semantics

- **Pass** — the endpoint returned a well-formed response for this check. For
  HTTP connectivity, this means an HTTP status below 500. For JSON-RPC checks,
  it means the method was accepted (did not return `-32601 method-not-found`).
- **Fail** — the check ran but the endpoint returned an error: an HTTP 5xx
  status, a JSON-RPC error object, a malformed response, or a connection/
  timeout error.
- **Warn** — the check ran and the endpoint answered, but the observed
  state is suspicious: with `--runs N` (N ≥ 2), `getLatestLedger` reports
  `Warn` instead of `Pass` when the ledger does not advance (same sequence
  and hash across runs — "possibly stale") or is inconsistent (same
  sequence, changing hash). A single run is inconclusive and never Warns.
  Runs execute back-to-back with no delay, and Stellar ledgers close
  roughly every 5 seconds, so two samples taken inside one ledger
  interval also read as "unchanged" — a `Warn` on a healthy endpoint is
  expected in that case, and is a prompt to look closer, not proof the
  endpoint is frozen. Warn is counted in `ran` but not in `passed`, and
  never changes the exit code — only `Fail` exits `1`. `contractTtl`
  reports `Warn` instead of `Pass` when the smallest remaining value
  (instance or code) is at or below `--ttl-threshold` (default 17280
  ledgers, ≈ 1 day); the boundary is inclusive.
- **Skip** — the check was not attempted (for example, because `--method`
  selected a different check). `contractTtl` Skips when it was explicitly
  requested (`--method contractTtl`) but no `--contract` was given; with
  no `--contract` at all it is absent from the report entirely.

Aggregation precedence across runs is **Fail > Warn > Skip > Pass**: a
`Fail` in any run is never hidden by another run's `Warn`/`Skip`/`Pass`,
and a `Warn` is never hidden by `Skip`/`Pass` — but a `Warn` never
outranks a `Fail`.

### Error shapes

Every failed check carries an `error_shape` in its `details` (human and
JSON output). The shape is finer than the coarse `IssueCategory`: two
failures in the same category can have different shapes, so a reader can
tell *how* the provider failed without opening the raw response. The
mapping is table-driven on the error code (`classify_error_shape` in
`src/error.rs`); it classifies shapes, it does not fingerprint providers.
`IssueCategory` values and exit codes are unchanged.

| `error_shape` | When it appears |
| --- | --- |
| `rate_limited` | HTTP 429 (with `retry_after` / `retry_after_secs` in `details` when the server sends `Retry-After`) |
| `html_error_page` | HTTP error whose body is an HTML page (e.g. a Cloudflare-style error page) |
| `http_server_error` | HTTP 5xx with a non-HTML body |
| `http_client_error` | HTTP 4xx other than 429, with a non-HTML body |
| `http_error` | Any other non-200 HTTP status |
| `method_not_found` | JSON-RPC `-32601` |
| `invalid_params` | JSON-RPC `-32602` |
| `internal_error` | JSON-RPC `-32603` |
| `json_rpc_error` | Any other JSON-RPC error code |
| `non_json_response` | HTTP 200 with a body that is not JSON (the raw body is never shown) |
| `malformed_response` | Well-formed JSON that is not a valid JSON-RPC response (missing `jsonrpc`, `result`, or error `code`) |
| `oversized_response` | Response body exceeds `--max-response-bytes` |
| `unsupported_method` | The method answered but the result carries an error (e.g. `simulateTransaction` result-level `error`) |
| `missing_field` | A required field is absent from an otherwise valid response |
| `transport_error` | Connection / read failure (DNS, refused, reset) |
| `timeout` | The request exceeded `--timeout` |
| `configuration_error` | The client could not even be built (e.g. an unsupported URL scheme) |
| `unknown_error` | Any other failure |

One exception: the `--expected-network` mismatch downgrades a passing
`getNetwork` check to `Fail` without an `error_shape` — that is a
client-side validation failure, not a provider error shape.

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
  connectivity probe runs in addition to the twelve method checks
  (eleven unconditional, plus `contractTtl` when `--contract` is given).
- `contractTtl` reads a contract's instance entry and its Wasm code entry
  via `getLedgerEntries` and reports remaining ledgers until expiry:
  each entry's `liveUntilLedgerSeq` minus the latest ledger sequence
  (resolved via `getLatestLedger`). The instance key is a `ContractData`
  `LedgerKey` for the contract address with the
  `LedgerKeyContractInstance` key and `PERSISTENT` durability; the code
  key is a `ContractCode` `LedgerKey` for the Wasm hash parsed out of the
  instance entry's executable. A missing instance entry is a Fail (the
  contract is archived, expired, or never deployed), and so is a missing
  code entry for a Wasm contract. A Stellar Asset Contract (or an
  external reference) has no Wasm and therefore no code entry by design:
  the check then reports `code_remaining_ledgers` as `null` and judges by
  the instance entry alone. `details` carry `instance_remaining_ledgers`,
  `code_remaining_ledgers`, `latest_ledger`, `instance_live_ledger`,
  `code_live_ledger`, `code_executable`, `code_wasm_hash`, and
  `ttl_threshold`. The contract ID is validated before any request is
  made (a malformed `C...` strkey exits `2`); `--method contractTtl`
  without `--contract` also exits `2`.
- JSON-RPC error codes are classified (`-32601`, `-32602`, `-32603`, etc.) but
  no provider-specific error taxonomy is applied.
- No archive pagination, cross-provider equivalence, or performance
  benchmarking is performed or claimed. Latency statistics describe the
  observed distribution across the configured runs, not a sustained
  performance benchmark.

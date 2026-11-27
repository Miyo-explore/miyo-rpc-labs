# Usage

← [Back to README](../README.md)

## Usage

```text
miyo-rpc check --url <RPC_URL>                       # human-readable report (default)
miyo-rpc check --url <RPC_URL> --output json         # machine-readable JSON
miyo-rpc check --url <RPC_URL> --method getHealth    # one JSON-RPC check (httpConnectivity still runs)
miyo-rpc check --url <RPC_URL> --timeout-secs 10     # override the 15s default
miyo-rpc check --url <RPC_URL> --runs 5              # repeat 5× with latency stats
miyo-rpc check --url <RPC_URL> --expected-network testnet  # validate network identity (testnet/mainnet/futurenet)
miyo-rpc check --url <RPC_URL> --compare <SECOND_RPC_URL>   # compare network identity + ledger against a second endpoint
miyo-rpc check --url <RPC_URL> --method getFeeStats    # one built-in check (httpConnectivity still runs)
miyo-rpc check --url <RPC_URL> --contract <C...>    # check a contract's instance + code TTL (Warn at/below --ttl-threshold, default 17280 ledgers ≈ 1 day)
miyo-rpc check --url <RPC_URL> --method getNewMethod --params '{"key":"..."}'   # probe a not-yet-built-in method with params
miyo-rpc batch --file endpoints.txt                          # diagnose many endpoints (one URL per line; see Batch Mode)
miyo-rpc batch --file endpoints.txt --output json            # aggregated JSON (schema_version "1.0")
miyo-rpc --help
miyo-rpc --version
miyo-rpc check --help
```

### Flags

| Flag               | Short | Default   | Notes                                                                 |
|--------------------|-------|-----------|-----------------------------------------------------------------------|
| `--url <URL>`      | `-u`  | _required_ | The Stellar JSON-RPC endpoint URL to diagnose.                       |
| `--output <FMT>`   | `-o`  | `human`   | `human` (colored terminal) or `json`. Colors are off when `NO_COLOR` is set to a non-empty value or stdout is not a terminal. |
| `--timeout-secs`   | `-t`  | `15`      | Maximum time to wait per HTTP request.                               |
| `--method <NAME>`  | `-m`  | _(none)_  | Run only the named JSON-RPC method. For a built-in check (`getHealth`, `getNetwork`, `getLatestLedger`, `getVersionInfo`, `getEvents`, `getLedgerEntries`, `getLedgers`, `retentionWindow`, `getTransactions`, `getFeeStats`, `getTransaction`, `simulateTransaction`, `contractTtl`) the enriched check runs; for any other name a single arbitrary-method probe runs with `--params`. The HTTP connectivity check (`httpConnectivity`) always runs regardless of this flag. `--method contractTtl` requires `--contract` (exit code `2` otherwise). |
| `--contract <ID>` |       | _(none)_  | Contract ID (strkey `C...`, 56 characters) whose instance and code TTL to check. The `contractTtl` check only runs when this is given; without it, default runs keep 13 checks. The ID is validated (format + checksum) before any request is sent; a malformed ID exits `2`. |
| `--ttl-threshold <N>` | | `17280` | TTL threshold in ledgers for the `contractTtl` check (17280 ≈ 1 day). Remaining ledgers at or below this threshold report `Warn` (inclusive). Only applies when `--contract` is given; ignored otherwise (warning on stderr). |
| `--min-retention <N>` | | `17280` | Minimum-retention threshold in ledgers for the `retentionWindow` check (17280 ≈ 1 day). Retained ledgers at or below this threshold report `Warn` (inclusive). Applies to every run: `retentionWindow` needs no `--contract`. |
| `--params <JSON>`  |       | _(none)_  | JSON-RPC `params` for an arbitrary-method probe: a JSON object, array, or `null`. Must be valid JSON, or the CLI exits `2` before any request is sent. Raw params are never echoed in output. Only applies when `--method` names a non-built-in method; otherwise the params are ignored (warning on stderr). |
| `--params-file <FILE>` |  | _(none)_ | Read JSON-RPC `params` from a file (same JSON rules as `--params`). Only applies when `--method` names a non-built-in method; otherwise the params are ignored (warning on stderr). |
| `--runs <N>`       | `-n`  | `1`       | Number of diagnostic runs to perform. Clamped to the range `[1, 10]` by the CLI. With N ≥ 2, `getLatestLedger` reports `Warn` (not `Pass`) when the ledger is stagnant or inconsistent across runs; see [Checks](checks.md). |
| `--expected-network <NAME>` | _(none)_ | Validate the endpoint's `getNetwork` passphrase against `testnet`, `mainnet`, or `futurenet`. A mismatch is reported as FAIL (exit code 1). |
| `--compare <URL>`      | _(none)_ | Compare the primary endpoint's `getNetwork` passport + `getLatestLedger` sequence/hash against a second endpoint `<URL>`. Divergences are appended as `evidence` (informational; exit code unchanged). |
| `--user-agent <STRING>` | _(none)_ | `miyo-rpc/<VERSION>` | Override the HTTP `User-Agent` header sent with requests. Useful when a gateway, proxy, or rate limiter filters on `User-Agent`. |
| `--max-response-bytes <N>` | _(none)_ | `10485760` (10 MiB) | Maximum response body size in bytes. A larger response is rejected as an error instead of being buffered. Must be greater than zero (exit code `2` otherwise). |
| `--no-follow-redirects` | _(none)_ | _follow_ | Do not follow HTTP redirects. A 3xx response is reported in the check result instead of being followed to a different host. |
| `--help`           |       |           | Show help.                                                                                |
| `--version`        |       |           | Print version and exit.                                                                   |

> **`--method` behavior:** The HTTP connectivity check (`httpConnectivity`)
> always runs, regardless of `--method`. The flag only filters the
> **JSON-RPC method checks** (`getHealth`, `getNetwork`, `getLatestLedger`,
> `getVersionInfo`, `getEvents`, `getLedgerEntries`, `getLedgers`,
> `retentionWindow`, `getTransactions`, `getFeeStats`, `getTransaction`,
> `simulateTransaction`, `contractTtl`). Method checks that do not match are reported as `SKIP`
> with `evidence` set to `"Skipped: --method filter does not match this
> check."`. (`contractTtl` is only in the list when `--contract` is given
> or `--method contractTtl` is requested; without `--contract` it reports
> `SKIP` instead of running.)

> **`--user-agent` / `--max-response-bytes` / `--no-follow-redirects`:** all
> three apply to the `check` subcommand only. The `--compare` endpoint
> reuses the same client settings (same `User-Agent`, same response cap,
> same redirect policy). They do not affect `batch`, which keeps the
> defaults.

> **Human output colors:** PASS/FAIL/SKIP are colored only when stdout is a
> terminal. Set `NO_COLOR` to any non-empty value to disable colors
> (https://no-color.org) — useful when piping output to a file or a CI log.
> An empty `NO_COLOR` is treated as unset. JSON output never contains ANSI
> codes.

### Exit codes

| Code | Meaning                                                              |
|------|----------------------------------------------------------------------|
| `0`  | All executed checks passed (no `Fail` results). A `Warn`-only run exits `0` — warnings are flagged, not failed. |
| `1`  | One or more checks returned `Fail`.                                 |
| `2`  | A non-check error occurred (for example, the report could not be serialized to JSON, or `--contract` is malformed / `--method contractTtl` was given without `--contract`). |

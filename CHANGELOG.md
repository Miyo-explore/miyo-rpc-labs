# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Release cadence is **release-as-ready**: a new tagged release is cut whenever a
milestone is complete and verified locally, rather than on a fixed schedule.

## Versioning — two independent numbers

This project ships **two decoupled version numbers**:

- **Crate / binary version** (`miyo-rpc --version`, `Cargo.toml` `version`):
  Semantic Versioning of the *CLI and Rust API*. Bumped per release
  (`0.x.0` during initial development; `MAJOR.MINOR.PATCH` once stable).
- **`schema_version`** (top-level key in JSON output, constant in `src/lib.rs`):
  versioned **only** with the *JSON report shape*. It is incremented (major)
  solely when the machine-readable structure changes non-backwards-compatibly —
  a field is removed, renamed, or its type changed. **Additive** additions
  (new fields, new keys inside `details` / `checks`, new check results) do **not**
  bump `schema_version`. The 0.1.0 release ships `schema_version: "1.0"`.
  Consumers MUST read `schema_version` and MUST ignore fields they do not
  recognize.

## [Unreleased]

### Added
- Built-in `getFeeStats` check (#33): sends `getFeeStats` with no params
  and surfaces the Soroban / classic inclusion-fee mode, p95, and
  transaction count plus `latestLedger` in `details`. A JSON-RPC error
  (including `-32601`) is a Fail with `UnsupportedMethod`, like the other
  built-in method checks.
- Built-in `getTransaction` check: resolves the latest ledger, fetches one
  page of transactions, and calls `getTransaction` with the first returned
  hash; details carry `tx_hash`, `status`, `ledger`, `fee_charged`, and
  XDR-presence flags. An empty page is a Skip (no transaction to fetch),
  not a Fail.
- Built-in `simulateTransaction` check: resolves the network via
  `getNetwork` and replays an embedded Testnet probe transaction (a real
  `InvokeHostFunction` transaction observed on Testnet; simulation ignores
  signatures and sequence numbers, so replay is valid). Passes when the
  simulation returns results (`min_resource_fee`, `results_count`,
  `latestLedger` in `details`); a result-level `error` is a Fail. On any
  network other than Testnet it Skips with evidence — the probe contract
  only exists on Testnet.
- `Warn` status tier (#34): with `--runs N` (N ≥ 2), `getLatestLedger`
  reports `Warn` instead of `Pass` when the ledger is stagnant (same
  sequence and hash across runs) or inconsistent (same sequence, changing
  hash). Warn is surfaced in human output (magenta `WARN`) and JSON
  (`"warn"` + a `warned` summary count), counted in `ran` but not in
  `passed`, and never changes the exit code. Aggregation precedence is
  Fail > Warn > Skip > Pass. Fixed alongside: the ledger-health analysis
  only read string-form sequences, but live endpoints return the sequence
  as a JSON number — the analysis was a silent no-op on real endpoints
  and now accepts both forms (same fix in `--compare` ledger divergence).

## [0.3.1] - 2026-11-26

### Fixed
- Testnet network passphrase corrected to `Test SDF Network ; September 2015`
  (was `Test SDF Network ; February 2023`): `check --expected-network testnet`
  failed the `getNetwork` validation on the real Testnet even though the
  endpoint was healthy. Verified against a live `getNetwork` response from
  soroban-testnet.stellar.org (#32).
- `httpConnectivity` probe now honours `--no-follow-redirects`: with the flag
  set, a 3xx response is reported as a redirect failure (with the `Location`
  in `details`) instead of being silently followed. Without the flag,
  behaviour is unchanged (#36).

### Added
- `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1); README table of contents,
  status badges, and a Contributing section linking `CONTRIBUTING.md`.
- Repository topics (`stellar`, `soroban`, `rust`, `rpc`, `diagnostics`,
  `cli`) and homepage URL.

### Changed
- `LICENSE` replaced with the canonical Apache-2.0 text from apache.org; the
  previous file was a non-canonical variant GitHub did not recognise.
- Issue template renamed `.github/ISSUE_TEMPLATE/drips_wave_task.md` →
  `contributor_task.md` with neutral wording; all `drips-wave` / `[wave]`
  references removed from repo files and the `drips-wave` label deleted.

## [0.3.0] - 2026-11-25

### Added — Public endpoint report (2026-11-23)
- `docs/ENDPOINT-REPORT-2026-11.md`: live run of all 8 built-in checks
  (3 runs each) against the public Stellar RPC endpoints — Testnet 8/9,
  Futurenet 9/9, Mainnet 7/9 on first run (9/9 on retry). Endpoint URLs
  verified against the official Stellar docs at run time. Linked from the
  README ("Live endpoint report"). Docs only; no code change.

### Added — Built-in `getTransactions` check
- `check` now runs one more built-in check, `getTransactions`, bringing
  the total to eight (plus `httpConnectivity`).
  `--method getTransactions` runs it in isolation.
- `getTransactions` resolves `startLedger` from `getLatestLedger` first
  (the latest ledger, always in range) and then requests a single page
  (`pagination.limit` 2 — each transaction carries base64 XDR envelope +
  result + metadata, so the page stays small). No `includeFailed` is sent,
  so the server default applies (failed transactions included). The
  response `cursor` is reported as `has_cursor` but never followed.
  Details carry `start_ledger`, `transactions_returned`, `latest_ledger`,
  `oldest_ledger`, and `has_cursor`; an empty page is a Pass. If
  `getLatestLedger` fails, `getTransactions` fails instead of probing with
  a guessed ledger. Covered by mockito tests (request shape, empty result,
  dependency failure, malformed result, JSON-RPC error), a `--compare`
  coverage test (the compare run includes the new check on both
  endpoints), and a CLI end-to-end test. Verified live against
  soroban-testnet.stellar.org: the latest ledger returned 2 transactions
  (limit 2, `has_cursor` true); full live run 9/9 checks Pass. No new
  dependencies; read-only.
- `--compare` now runs the full eight-method built-in set on the second
  endpoint, including `getTransactions`.

### Added — Built-in `getLedgerEntries` and `getLedgers` checks
- `check` now runs two more built-in checks, `getLedgerEntries` and
  `getLedgers`, bringing the total to seven (plus `httpConnectivity`).
  `--method getLedgerEntries` / `--method getLedgers` run each in isolation.
- `getLedgerEntries` sends one fixed synthetic probe key — an Account
  `LedgerKey` for the all-zero ed25519 key, which no real wallet owns — so
  the check is network-agnostic and needs no `getLatestLedger` round-trip.
  Details carry `entries_returned` and `latest_ledger`; an empty `entries`
  array is a Pass (the probe account simply does not exist on that
  network). Verified live against soroban-testnet.stellar.org, where the
  probe key resolves to a real entry.
- `getLedgers` resolves `startLedger` from `getLatestLedger` first (the
  latest ledger, always in range) and then requests a single page
  (`pagination.limit` 2 — each ledger carries base64 XDR header +
  metadata, so the page stays small). The response `cursor` is reported
  as `has_cursor` but never followed. Details carry `start_ledger`,
  `ledgers_returned`, `latest_ledger`, `oldest_ledger`, and `has_cursor`;
  an empty page is a Pass. If `getLatestLedger` fails, `getLedgers`
  fails instead of probing with a guessed ledger. Covered by mockito
  tests (request shape, empty result, dependency failure, malformed
  result, JSON-RPC error) and CLI end-to-end tests. No new dependencies;
  read-only.

### Added — Built-in `getEvents` check
- `check` now runs a fifth built-in check, `getEvents`, in addition to
  `getHealth`, `getNetwork`, `getLatestLedger`, and `getVersionInfo`
  (plus `httpConnectivity`). `--method getEvents` runs it in isolation.
- `getEvents` requires a `startLedger` inside the endpoint's retention
  window, so the check resolves it from `getLatestLedger` first (the
  latest ledger, always in range) and then requests a single page
  (`pagination.limit` 10, no filters — all event types, all contracts).
  The response `cursor` is reported as `has_cursor` but never followed.
  Details carry `start_ledger`, `events_returned`, `latest_ledger`,
  `oldest_ledger`, and `has_cursor`; an empty page is a Pass. If
  `getLatestLedger` fails, `getEvents` fails instead of probing with a
  guessed ledger. Covered by mockito tests (request shape, string/number
  sequence, empty page, dependency failure, malformed result, JSON-RPC
  error) and a CLI end-to-end test. No new dependencies; read-only.

## [0.2.0] - 2026-11-19

### Added — `NO_COLOR` support in human output
- Human output (`--output human`, the default) no longer emits ANSI color
  codes when the `NO_COLOR` environment variable is set to a non-empty
  value (https://no-color.org), or when stdout is not a terminal (piped to
  a file, captured by CI). Colors are unchanged on a TTY without
  `NO_COLOR`. JSON output is unaffected and never contains ANSI codes.
  New helpers: `colors_enabled()` and `format_human_with_color(report,
  color)` in `src/output.rs`, covered by unit tests.

### Fixed — Document `--params` / `--params-file` interaction with built-in methods
- README flags table and the "Arbitrary Method Probing" section now state
  that `--params` / `--params-file` only apply when `--method` names a
  non-built-in method. When given without `--method` or with a built-in
  method name, the params are ignored and the CLI prints the existing
  stderr warning verbatim. No behavior changed; documentation only.

### Added — Batch output (`--output human`) and per-endpoint timeout
- `batch --output human` (the default) renders a compact per-endpoint block
  (`<endpoint> [PASS|FAIL] <n> passed, <m> failed`, `[INVALID] <reason>` for
  bad lines) followed by an aggregate `Summary:` line. The wiring
  (`format_batch_human` + output branching in `main.rs`) already existed;
  this closes the remaining gaps: content tests, CLI tests proving human
  output is not JSON, and README documentation. JSON output is unchanged.
- `batch --timeout-secs` (default 15, same as `check`) is now documented and
  tested as a per-request timeout applied to every endpoint in the batch:
  each HTTP request gets its own budget, so a slow endpoint fails its own
  checks with a timeout instead of stalling the batch. The flag itself
  already existed and was already threaded through to the client config.

### Added — RPC client flag (`--no-follow-redirects`) and `futurenet` network
- `check --no-follow-redirects` disables following HTTP redirects (default
  stays follow). A 3xx response is reported in the check result (`HTTP 302: …`)
  instead of silently following the redirect to a different host.
  `check`-only; `batch` keeps the defaults.
- `futurenet` is now a known network for `--expected-network`
  (`Test SDF Future Network ; October 2022`, per the Stellar developer docs).
  Lookup stays case-insensitive. Standalone networks still cannot be
  validated — their passphrases are custom by definition.

### Added — RPC client flags (`--user-agent`, `--max-response-bytes`)
- `check --user-agent <STRING>` overrides the HTTP `User-Agent` header sent
  with requests (default stays `miyo-rpc/<VERSION>`). The override also
  applies to the `httpConnectivity` check and to the `--compare` endpoint.
- `check --max-response-bytes <N>` exposes the 10 MiB response-size cap
  (default unchanged). A larger response is rejected with an error naming
  the cap instead of being buffered into memory.
- `--max-response-bytes 0` and non-numeric values exit `2` **before** any
  request is sent. Both flags are `check`-only; `batch` keeps the defaults.

### Added — HTTP 429 rate-limit classification
- HTTP `429 Too Many Requests` is now classified as `rate_limited`, distinct
  from generic `ClientError`. Other 4xx statuses remain `ClientError`; 5xx
  remains `ServerError`; transport and JSON-RPC classification are unchanged.
- The `Retry-After` response header is surfaced on the failing check under
  `details.retry_after` (raw value, either delta-seconds or an HTTP-date).
  When it parses as an integer, `details.retry_after_secs` is added as well.
  Nothing is synthesised when the header is absent or non-numeric.
- Batch diagnostics inherit the classification and retry fields through the
  shared per-endpoint report serializer; no logic is duplicated.
- The CLI does **not** retry requests automatically. There is no backoff or
  throttle-recovery behaviour.
- Behaviour change: a 429 previously reported as `client_error` now reports as
  `rate_limited`. This is an additive enum variant under the documented
  `schema_version` policy, so no schema bump is required.

### Added — Arbitrary JSON-RPC method probing
- `check --method <NAME>` now probes any JSON-RPC method, not only the four
  built-ins. Naming a built-in (`getHealth`, `getNetwork`, `getLatestLedger`,
  `getVersionInfo`) keeps its enriched check; naming anything else runs a single
  arbitrary-method probe (plus `httpConnectivity`).
- `--params <JSON>` / `--params-file <FILE>` supply JSON-RPC `params` (object,
  array, or null). Malformed JSON or a scalar shape exits `2` **before** any
  request is sent.
- Results are represented by shape (object keys merged into `details`;
  non-objects under `details["result"]`); oversized results are flagged with
  `result_truncated` + `result_size_bytes` instead of being stored silently.
- Raw `--params` values are never echoed in `evidence`, error messages, or output.
- In arbitrary mode, `--expected-network` and `--compare` emit a clear warning
  (no-op) rather than running extra checks implicitly.
- Hardening: `--params`/`--params-file` are rejected (exit 2) when unreadable,
  oversized (> 1 MiB), or a non-regular file; a clear warning is emitted when
  they are ignored because `--method` is absent or names a built-in.
- Hardening: an empty/whitespace-only `--method` is rejected with exit 2 before
  any request.
- Hardening: `ArbitraryMethodCheck`'s `Debug` output redacts params; the library
  API `run_arbitrary_probe` clamps `runs` to at least 1 so a caller cannot get a
  false-success empty report.
- Documented that a remote server may echo input into its response; the tool
  cannot guarantee a server-echoed secret is hidden from the report.

## [0.1.0] - 2026-10-23

Initial public release of the Stellar JSON-RPC diagnostics toolkit.

### Added — M1: Network Identity Validation
- `--expected-network <testnet|mainnet>` validates the endpoint's `getNetwork`
  passphrase against known Stellar network constants.
- Network mismatch is reported as `Fail` (`IssueCategory::Other`); existing
  skipped/already-failed results are preserved.

### Added — M2: Versioned JSON Output
- Top-level `schema_version: "1.0"` on every JSON report.
- Stable, documented field names; additive-only compatibility contract.

### Added — M3: Ledger Health & Convergence
- `analyze_ledger_health` detects stale/suspicious ledger state across runs
  (`--runs N>=2`): distinguishes healthy advancement from confirmed stagnation;
  single-run reported as inconclusive (never mis-flagged).
- `--compare <URL>` reports cross-endpoint divergence (passport mismatch +
  sequence/hash drift) as evidence (informational, exit-code-preserving).

### Added — M4: Batch Endpoint Diagnosis
- `miyo-rpc batch --file endpoints.txt` diagnoses many endpoints in one run with
  bounded concurrency (default 8, clamped to `[1, 32]`), capped at 1000 endpoints.
- Aggregate `BatchReport` (JSON) with per-endpoint breakdown + summary.
- Deterministic exit codes: `0` all healthy, `1` one+ failed, `2` no valid
  endpoints. Invalid lines reported (`valid: false`), never silently dropped.

### Added — M5: Release & Ecosystem Integration
- CI workflow (`ci.yml`): `cargo fmt --check`, `cargo clippy -D warnings`,
  `cargo test --all-targets` on PRs + push to `main` + manual dispatch.
- Release workflow (`release.yml`): tag-triggered (`v*.*.*`), matrix builds for
  Linux x86_64, macOS Apple Silicon + Intel, and Windows x86_64; ships
  per-target `miyo-rpc-v<VERSION>-<target>.tar.gz` + `.sha256`. Global
  `contents: read`, with `contents: write` granted only to the publishing job;
  cannot fire (or publish) from PRs.
- `CHANGELOG.md` with versioning policy (`schema_version` decoupled from crate
  version).

### Core Features (present since 0.1.0)
- `check` subcommand running independent checks: `httpConnectivity`, `getHealth`,
  `getNetwork`, `getLatestLedger`, `getVersionInfo`.
- Human-readable and JSON output; worst-status-wins aggregation across `--runs`.
- URL sanitization redacting sensitive query params / userinfo.

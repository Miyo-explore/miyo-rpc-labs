# Miyo RPC Labs — Product Roadmap

## 1. Product Vision

Miyo RPC Labs will become the standard local CLI for diagnosing, validating,
and reproducing compatibility issues against Stellar JSON-RPC endpoints.

It should be the tool a developer reaches for when an RPC call fails: "Is the
endpoint down, misconfigured, on the wrong network, or does it simply not
support this method?" It answers that with a structured, reproducible report
and a non-zero exit code for every category of failure, so it can also be used
inside CI to assert endpoint health.

**Concrete end state:** Given any Stellar Soroban-compatible RPC URL, `miyo-rpc`
produces a complete per-method capability profile, validates that the endpoint
is on the expected network, detects ledger stalls or divergence across
providers, and emits machine-readable output that CI can gate on — with no
configuration beyond the endpoint URL.

## 2. Problem and Target Users

### The problem

When Stellar development tooling breaks against a specific RPC provider, the
root cause is ambiguous and hard to reproduce:

- Is the endpoint down, or rate-limiting?
- Is it on `testnet` when the user expected `mainnet`?
- Did the provider drop `getVersionInfo`, or return `-32601`?
- Are responses malformed (HTTP 200 with bad JSON-RPC)?
- Is this a timeout, or a slow-but-correct provider?

Developers currently resort to `curl` + manual JSON parsing, which is error-prone,
non-deterministic, and does not sanitize credentials embedded in URLs.

### Intended users

1. **Soroban dApp developers** — verifying their configured RPC provider works
   before deploying or debugging transactions.
2. **DevOps / SRE** — adding an endpoint health gate to CI or monitoring.
3. **Protocol integrators** — comparing providers and filing reproducible
   compatibility bugs.
4. **Contributors** — reproducing and reporting issues to RPC providers or
   Soroban tooling.

### Today's limitations

v0.1.0 resolves the original roadmap gaps: network-identity validation
(`--expected-network`), cross-run ledger progression (`--runs`), `--compare`
for cross-endpoint divergence, machine-readable `schema_version` (`"1.0"`),
URL sanitization, and a CI + release workflow now ship. By-design scope limits
for the current release (plain-`GET` connectivity semantics, only four built-in
JSON-RPC methods, no provider-specific error taxonomy, no sustained
benchmarking) are documented in the README "Limitations" section.

## 3. Scope and Design Principles

### Core responsibilities

- Diagnose endpoint reachability and JSON-RPC method support.
- Classify each failure into a coarse `IssueCategory` and surface a precise
  message + evidence string.
- Aggregate repeated-run results with deterministic precedence (`Fail > Skip > Pass`).
- Emit human-readable and JSON reports suitable for CI consumption (exit codes
  encode pass/fail).

### Explicit non-goals

- This is **not** a transaction sender, wallet, or account inspector.
- It does **not** perform load testing or produce sustained benchmarks.
- It does **not** proxy or cache requests.
- It does **not** persist state between runs.

### Design principles

- **Deterministic by default.** Tests must run without network access.
- **Minimal dependencies.** Only `clap`, `reqwest`, `serde`, `serde_json`,
  `thiserror`, `tokio`, `url`, `urlencoding`, `chrono`. No DB, no cache, no
  HTTP server.
- **Opinionated defaults, escape hatches.** Safe defaults (15s timeout, 1 run)
  but user-overridable via CLI flags.
- **Exit codes mean something.** `0` = all checks pass; `1` = any Fail;
  `2` = output/usage error.
- **No secrets in output.** URL sanitization is mandatory output, not opt-in.

### Constraints

- Rust, stable toolchain, `edition = "2021"`, single binary.
- Must run on Linux / macOS / Windows (x86_64, ARM64).
- No paid APIs or live endpoints required for the test suite.

## 4. Current State and Gap Analysis

### What exists today (verified in `main` @ a4fc479 — tag `v0.1.0`)

| Capability | Status | Evidence |
|------------|--------|----------|
| HTTP connectivity check | Implemented | `src/diagnostics.rs:227` |
| `getHealth`, `getNetwork`, `getLatestLedger`, `getVersionInfo` method checks | Implemented | `src/diagnostics.rs:339` |
| Worst-status aggregation (`Fail > Skip > Pass`) | Implemented | `src/diagnostics.rs:34` |
| Repeated runs (`--runs`, clamped `[1, 10]`) | Implemented | `src/cli.rs:64`, `src/diagnostics.rs:414` |
| Latency statistics (min/avg/max/run_count) | Implemented | `src/diagnostics.rs:516-532` |
| Human + JSON output | Implemented | `src/output.rs:100,155` |
| URL sanitization (userinfo + query params) | Implemented | `src/output.rs:39` |
| Exit codes (0/1/2) | Implemented | `src/main.rs:39,45,47` |
| `--method` single-check filter | Implemented | `src/diagnostics.rs:420` |
| Error classification (`IssueCategory`) | Implemented | `src/error.rs:13` |
| Deterministic test suite (mockito) | Implemented | `tests/diagnostics.rs`, `Cargo.toml:33` |
| CI (fmt, clippy, test) | Implemented | `.github/workflows/ci.yml` |

### Gaps (verified absent)

The gaps that motivated M1–M6 were all **resolved in the `v0.1.0` release**
(see `CHANGELOG.md`); each maps to a shipped milestone. Remaining, unaddressed
limitations are tracked in the README "Limitations" section:

| Gap | Resolution |
|-----|------------|
| No network identity validation | resolved (M1: `--expected-network`) |
| No `--expected-network` flag | resolved (M1) |
| No cross-method coherence checks | resolved (M4: `--compare` passport + ledger drift) |
| No batch / multi-endpoint mode | resolved (M4) |
| No JSON schema version | resolved (M2: `schema_version = "1.0"`) |
| No release workflow | resolved (M5: `release.yml`, live for `v0.1.0`) |
| No changelog discipline | resolved (M5: `CHANGELOG.md`) |

## 5. Phased Development Plan

### Milestone 1 — Verified Network Identity (implemented)

- **Objective:** Add a `--expected-network` flag that validates the endpoint's
  `getNetwork` passphrase against a known Stellar network.
- **User value:** CI can assert an endpoint is on `mainnet` or `testnet`; misconfigured
  or spoofed endpoints fail immediately with a deterministic report.
- **Scope:**
  - `--expected-network <testnet|mainnet>` flag (case-insensitive).
  - Known passphrase constants for `testnet` and `mainnet`.
  - On mismatch: downgrade the `getNetwork` check to `Fail` with `IssueCategory::Other`
    and a descriptive `error_message`; recompute the summary so `summary.passed` / `failed` reflect the downgrade.
  - On unknown network name: emit a clear usage error (exit code `2`).
  - If `getNetwork` is skipped (`--method` filter) or already `Fail`, validation is skipped silently.
- **Dependencies:** none.
- **Definition of done:**
  - `cargo test --all-targets` passes with new tests for match/mismatch/skip/unknown.
  - `cargo clippy -- -D warnings` and `cargo fmt --check` pass.
  - README documents the flag and its behavior.
- **Validation:** unit tests on network lookup + integration tests on the enforcement function.
- **Priority:** **P0** — this is the single highest-risk gap (silent wrong-network diagnosis)
  and is independently deliverable without touching repeated-run or output code.

### Milestone 2 — JSON Output Schema Versioning (implemented)

- **Objective:** Add a top-level `schema_version` field to JSON output.
- **User value:** Machine consumers can detect report-shape changes across versions.
- **Scope:** Add `"schema_version": "1.0"` to `JsonReport` (first key). Compatibility policy: **additive** changes (new fields) do NOT bump the version; only removals / renames / type changes do. Consumers MUST ignore unknown fields. Documented in README.
- **Dependencies:** Milestone 1 (touch output.rs anyway; combine).
- **Priority:** P1 — needed before any CI integration.
- **Validation:** `test_json_schema_version_and_field_types` asserts presence +
  value + field types; `test_json_includes_compare_divergence` and
  `test_json_includes_ledger_evidence` confirm M3/M4 evidence survives into JSON.

### Milestone 3 — Ledger Progression Sanity Check (implemented)

- **Objective:** When `--runs N` (N >= 2), verify `getLatestLedger` sequence
  advances monotonically across runs (detect stalls).
- **User value:** Detect endpoints that freeze or serve a stale ledger.
- **Scope:** `analyze_ledger_health` collects `sequence` + `id` (ledger hash)
  per run and appends a freshness verdict (`healthy` / `possibly-stale` /
  `inconsistent`) to the check's `evidence`. Non-monotonic or stagnant state
  is surfaced as evidence, not a `Fail` — `Warn` tier is deferred (see Risks),
  and a single run is reported as inconclusive rather than mis-flagged.
- **Dependencies:** Milestone 1.
- **Priority:** P2 — valuable but expands the status model.
- **Validation:** 5 unit tests in `src/diagnostics.rs` (advancing / stagnant /
  inconsistent / single-run / no-check), all green under `cargo test`.

### Milestone 4 — Batch Endpoint Diagnosis (implemented)

- **Objective:** Diagnose many endpoints from a file, and compare two endpoints
  in one invocation.
- **User value:** Bulk provider health + cross-endpoint divergence detection.
- **Scope (MVP):** `miyo-rpc batch --file endpoints.txt` parses one URL per line
  (blanks and `#` comments skipped, duplicates removed, capped at 1000), validates
  URLs, runs `DiagnosticReport::run_all` per endpoint with bounded concurrency
  (default 8, clamped to [1, 32]), and emits an aggregate `BatchReport` (JSON,
  versioned). `--compare <URL>` on `check` adds pairwise divergence evidence.
- **Exit codes:** 0 = all valid endpoints healthy; 1 = one+ failed; 2 = no valid
  endpoints (file error / all-invalid / empty). Invalid lines are reported, not
  silently dropped; invalid input is distinguishable from diagnostic failures.
- **Validation:** 5 unit tests in `src/batch.rs` + 3 integration tests in
  `tests/diagnostics.rs` (`test_batch_*`), green under `cargo test --all-targets`.
- **Dependencies:** Milestones 1–3.
- **Priority:** P3 — moderate scope.

### Milestone 5 — Release & Ecosystem Integration (implemented)

- **Objective:** Automated release workflow + CHANGELOG discipline.
- **User value:** Users install prebuilt, checksummed binaries; contributors see
  a changelog and a versioning policy.
- **Scope:** CI (`ci.yml`: fmt + clippy + test, least-privilege `contents: read`),
  tag-triggered release (`release.yml`: builds `x86_64-unknown-linux-gnu`, ships
  `.tar.gz` + `.sha256`, global `contents: read` with `contents: write` only on
  the publishing job, cannot fire from PRs),
  `CHANGELOG.md` with `schema_version`-vs-crate-version policy.
- **Dependencies:** Milestones 1–4.
- **Priority:** P4 — process hygiene, not a feature.
- **Validation:** YAML lint clean; local `cargo fmt --check` / `cargo clippy -D
  warnings` / `cargo test --all-targets` green. Hosted CI green on every `main`
  push; tag release executed for `v0.1.0` (release workflow run `40001191938`,
  success).

### Milestone 6 — Cross-Platform Releases (implemented — shipped in `v0.1.0`)

- **Objective:** Automated builds + checksummed artifacts for Linux, macOS
  (Apple Silicon + Intel), and Windows x86_64.
- **User value:** Native binaries for all supported platforms via GitHub Releases.
- **Scope:** matrix `release.yml` (`ubuntu-latest`, `macos-15`, `macos-15-intel`,
  `windows-2022`); uniform `miyo-rpc-v<VERSION>-<target>.tar.gz` + `.sha256`;
  per-platform install/verify instructions in README.
- **Dependencies:** Milestone 5.
- **Priority:** P3.
- **Validation:** YAML lint clean; all four matrix targets compiled and packaged
  on hosted runners for `v0.1.0` (release workflow run `40001191938` — 4 builds
  + publish success).

## 6. Success Criteria

| Metric | Target | Measured by |
|--------|--------|-------------|
| Test suite runs with zero network access | 100% | `cargo test --all-targets` offline |
| CI is green on every PR | Yes | GitHub Actions status |
| A new contributor can run `cargo test` and see all tests pass | Yes | documented in README |
| `--expected-network` mismatch exits non-zero | Yes | integration test |
| README example outputs match `format_human` / `format_json` output | Yes | cross-checked against source |
| Time from "endpoint broken" to "reproduced report file" | < 5 s | manual timing on testnet |

## 7. Risks and Open Decisions

| Risk | Impact | Decision |
|------|--------|----------|
| Hardcoding passphrases couples us to Stellar network constants | Low | Accept — passphrases are public, stable, and rarely rotate. Re-evaluate if a network upgrade changes them. |
| Unknown network names could silently no-op | Medium | Reject unknown names with exit code `2` rather than ignoring. |
| Adding new fields mid-trajectory may break consumers | Medium | Gate behind `schema_version` (Milestone 2) after this milestone. |
| Ledger progression needs a `Warn` tier beyond Pass/Fail/Skip | Medium | Defer to Milestone 3; Milestone 1 stays within existing status model. |

## 8. Immediate Next Steps

M1–M6 are complete and shipped in `v0.1.0`.

- **Completed (M5):** CI + tag-triggered releases with checksummed binaries;
  `CHANGELOG.md` + versioning policy (`schema_version` decoupled from crate version).
- **Completed (M6):** multi-platform release matrix (Linux/macOS-Windows),
  per-platform install instructions.
- **Next (not a milestone):** gather user feedback after the first tagged release.

1. Run `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-targets`.

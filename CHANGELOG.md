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
  per-target `miyo-rpc-v<VERSION>-<target>.tar.gz` + `.sha256`. `contents: release`
  least-privilege permission; cannot fire (or publish) from PRs.
- `CHANGELOG.md` with versioning policy (`schema_version` decoupled from crate
  version).

### Core Features (present since 0.1.0)
- `check` subcommand running independent checks: `httpConnectivity`, `getHealth`,
  `getNetwork`, `getLatestLedger`, `getVersionInfo`.
- Human-readable and JSON output; worst-status-wins aggregation across `--runs`.
- URL sanitization redacting sensitive query params / userinfo.

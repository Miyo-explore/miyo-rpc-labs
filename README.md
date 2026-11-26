# Miyo RPC Labs

[![CI](https://github.com/Miyo-explore/miyo-rpc-labs/actions/workflows/ci.yml/badge.svg)](https://github.com/Miyo-explore/miyo-rpc-labs/actions/workflows/ci.yml)
[![Release](https://github.com/Miyo-explore/miyo-rpc-labs/actions/workflows/release.yml/badge.svg)](https://github.com/Miyo-explore/miyo-rpc-labs/actions/workflows/release.yml)
[![GitHub release](https://img.shields.io/github/v/release/Miyo-explore/miyo-rpc-labs)](https://github.com/Miyo-explore/miyo-rpc-labs/releases)
[![License](https://img.shields.io/github/license/Miyo-explore/miyo-rpc-labs)](LICENSE)

Open-source Stellar JSON-RPC diagnostics, compatibility testing, and reproducible
endpoint verification toolkit.

## Contents

- [What It Does](#what-it-does)
- [Why](#why)
- [Supported Checks](#supported-checks)
- [Installation](#installation)
- [Usage](#usage)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [License](#license)
- [Status](#status)

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

Status semantics and limitations: [docs/checks.md](docs/checks.md).

## Installation

- Rust toolchain (stable; the `edition = "2021"` crate).
- macOS, Linux, or Windows (x86_64 or ARM64).

```bash
git clone https://github.com/Miyo-explore/miyo-rpc-labs.git
cd miyo-rpc-labs
cargo build --release
```

The binary will be at `target/release/miyo-rpc`. Prebuilt checksummed
binaries are on [GitHub Releases](https://github.com/Miyo-explore/miyo-rpc-labs/releases)
— see [docs/installation.md](docs/installation.md) for per-platform download
and checksum verification.

## Usage

```text
miyo-rpc check --url <RPC_URL>                       # human-readable report (default)
miyo-rpc check --url <RPC_URL> --output json         # machine-readable JSON
miyo-rpc check --url <RPC_URL> --method getHealth    # one JSON-RPC check (httpConnectivity still runs)
miyo-rpc check --url <RPC_URL> --timeout-secs 10     # override the 15s default
miyo-rpc check --url <RPC_URL> --runs 5              # repeat 5× with latency stats
miyo-rpc check --url <RPC_URL> --expected-network testnet  # validate network identity (testnet/mainnet/futurenet)
miyo-rpc check --url <RPC_URL> --compare <SECOND_RPC_URL>   # compare network identity + ledger against a second endpoint
miyo-rpc check --url <RPC_URL> --method getTransaction --params '{"hash":"abc..."}'   # probe a non-built-in method with params
miyo-rpc check --url <RPC_URL> --method simulateTransaction --params-file tx.json        # params from a file
miyo-rpc batch --file endpoints.txt                          # diagnose many endpoints (one URL per line; see Batch Mode)
miyo-rpc batch --file endpoints.txt --output json            # aggregated JSON (schema_version "1.0")
miyo-rpc --help
miyo-rpc --version
miyo-rpc check --help
```

Full flag reference, exit codes, and output notes:
[docs/usage.md](docs/usage.md).

## Documentation

- [Installation](docs/installation.md) — prerequisites, building from source, prebuilt binaries and checksum verification.
- [Usage](docs/usage.md) — commands, all flags, exit codes.
- [Checks](docs/checks.md) — the supported checks catalogue, status semantics, limitations.
- [Network Identity and Repeated Probes](docs/network.md) — `--expected-network` validation, `--compare`, `--runs` and latency statistics.
- [Arbitrary Method Probing](docs/methods.md) — probing non-built-in methods with `--params` / `--params-file`.
- [Examples](docs/examples.md) — human-readable and JSON output examples, URL sanitization.
- [Batch Mode](docs/batch.md) — diagnosing many endpoints in one invocation.
- [Development](docs/development.md) — running tests, live validation, reporting issues.
- [Endpoint Report (2026-11)](docs/ENDPOINT-REPORT-2026-11.md) — live run against public Stellar RPC endpoints.

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for the
development setup, code-quality gates (`cargo fmt`, `cargo clippy -D warnings`,
`cargo test`), and the pull-request process. Good first issues are labelled
`good first issue` and carry a `complexity:*` label so you can judge the size
before picking one up. All behaviour is covered by the
[Code of Conduct](CODE_OF_CONDUCT.md).

## License

Apache-2.0. See [LICENSE](LICENSE).

## Status

This project is early-stage. See [CHANGELOG.md](CHANGELOG.md) for progress.

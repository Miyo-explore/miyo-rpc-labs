# Development

← [Back to README](../README.md)

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

### Live endpoint report

[`ENDPOINT-REPORT-2026-11.md`](ENDPOINT-REPORT-2026-11.md) records
a live run of `miyo-rpc` (8 built-in method checks, 3 runs each) against the
public Stellar RPC endpoints for Testnet, Futurenet, and Mainnet on
2026-11-23: per-endpoint, per-method pass/fail, latency, and the two
anomalies found (a stale expected Testnet passphrase in the tool, and
transient Mainnet timeouts that passed on retry). Endpoint URLs were verified
against the official Stellar docs at run time; SDF runs no public Mainnet RPC
endpoint, so the Mainnet run used a publicly listed third-party endpoint.

## Reporting Issues

To report a reproducible compatibility issue:

1. Run `miyo-rpc check --url <YOUR_RPC_URL> --output json > report.json`.
2. Share the sanitized `report.json` and the **exact URL** (without secrets) in
   a GitHub issue.
3. Include the `tool_version` and the `total_runs` field from the report when
   relevant.

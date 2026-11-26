# Network Identity and Repeated Probes

← [Back to README](../README.md)

## Network Identity Validation

`--expected-network <NAME>` validates the endpoint's `getNetwork` passphrase
against a known Stellar network passphrase (`testnet`, `mainnet`, or
`futurenet`). Standalone (private/local) networks use custom passphrases and
cannot be validated with this flag. If the
endpoint reports a different network, the `getNetwork` check is downgraded to
`Failed` with `IssueCategory::Other`, and `summary.passed`/`failed`/`ran` are
recomputed so they stay consistent:

```text
miyo-rpc check --url <RPC_URL> --expected-network testnet
```

- The HTTP connectivity check is unaffected.
- If `getNetwork` is skipped (via `--method`) or already failed, no validation
  is applied — the existing result is preserved.
- An unknown network name is a usage error (exit code `2`).

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
  a measured duration.

### Run-count semantics

- On a **successful** client initialization, the diagnostic loop executes the
  effective number of `runs`; `runs` and `summary.total_runs` both reflect the
  number of runs performed.
- On a **client-init** failure (e.g., an unsupported URL scheme such as
  `ftp://`), the tool cannot issue RPC calls at all. It runs a single
  connectivity attempt and reports `runs = 1` (and `summary.total_runs = 1`)
  rather than the requested `--runs` value. This ensures the report never
  claims more diagnostic runs than were actually performed.

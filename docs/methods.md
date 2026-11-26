# Arbitrary Method Probing

← [Back to README](../README.md)

## Arbitrary Method Probing

By default `check` runs the eleven built-in checks (`getHealth`,
`getNetwork`, `getLatestLedger`, `getVersionInfo`, `getEvents`,
`getLedgerEntries`, `getLedgers`, `getTransactions`, `getFeeStats`,
`getTransaction`, `simulateTransaction`) plus `httpConnectivity`. The
only standard RPC method without a built-in check is `sendTransaction`,
which this tool deliberately does not support (read-only by design, see
`ROADMAP.md`). Arbitrary probing remains for methods added by a future
protocol version or a provider extension — pass the method name to
`--method` together with JSON-RPC `params`:

```text
miyo-rpc check --url https://soroban-testnet.stellar.org \
  --method getNewMethod --params '{"key":"AAAA..."}'
```

- `--params` accepts a JSON **object**, **array**, or `null`. Malformed JSON or
  any other scalar shape exits with code `2` **before** any request is sent.
- `--params-file <FILE>` reads the same JSON from a file.
- The built-in checks are the default; naming a built-in (`getHealth`, etc.)
  runs that enriched check exactly as before. Naming anything else runs a
  single arbitrary-method probe with the given params (plus `httpConnectivity`).
- The response `result` is represented in the report by its shape: an **object**
  merges its keys into `details`; an **array / scalar / null** is stored under
  `details["result"]`. Oversized results are **not** silently stored in full —
  the report flags `result_truncated: true` with `result_size_bytes`.
- A well-formed JSON-RPC **error** object stays `Fail` (exit code `1`); only a
  valid `result` is `Pass`.
- Raw `--params` values are **never** echoed in `evidence`, error messages, or
  output — do not pass secrets in params.
- A remote server may echo your input (params or the URL) back in its response.
  The tool truncates `error.data` and `result`, but it cannot promise that a
  server-echoed secret is hidden from the report — treat the report as sensitive.
- In arbitrary mode `--expected-network` and `--compare` have nothing to act on;
  the CLI prints a warning and does not run extra checks implicitly.
- `--params` / `--params-file` are ignored when `--method` is omitted or
  names a built-in method, because built-in checks take no params. The CLI
  prints a warning to stderr in that case:
  `warning: --params/--params-file is ignored: --method does not name an
  arbitrary method (built-in checks take no params).` The params are not
  applied to any check; to use them, name a non-built-in method with
  `--method`.
- `--runs` aggregates repeated probes with worst-status-wins (a single `Fail`
  across runs is never hidden by a `Pass`).

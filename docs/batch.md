# Batch Mode

← [Back to README](../README.md)

## Batch Mode

`miyo-rpc batch --file endpoints.txt` diagnoses many Stellar/Soroban RPC
endpoints in a single invocation and emits one aggregate report suitable for
automation.

- **File format** — one URL per line; blank lines and `#` comments are skipped;
  duplicate URLs are de-duplicated (first occurrence wins). At most 1000
  endpoints are read (constant `MAX_ENDPOINTS`).
- **Concurrency** — up to `--concurrency` endpoints (default 8, clamped to
  `[1, 32]`) are diagnosed in parallel via a bounded `tokio::task::JoinSet`
  with a counting semaphore.
- **Exit codes**
  | Code | Meaning |
  |------|---------|
  | 0    | all valid endpoints are healthy (every check passed). |
  | 1    | one or more valid endpoints had a failing check. |
  | 2    | no valid endpoints were diagnosed (file unreadable, empty, or all lines invalid). |
- **Reporting invalid input** — invalid lines are kept in the output with
  `valid: false` and an `error` reason; they are never silently dropped. Exit
  code 2 distinguishes input failure from diagnostic failures (exit code 1).
- **Output formats** — `--output human` (default) prints a compact
  per-endpoint block (`<endpoint> [PASS|FAIL] <n> passed, <m> failed`,
  `[INVALID] <reason>` for bad lines) followed by an aggregate `Summary:`
  line. `--output json` emits the aggregate JSON document below. Both
  formats sanitize URLs identically (credentials are `REDACTED`).
- **Timeout** — `--timeout-secs` (default 15, same as `check`) is a
  per-request timeout applied to every endpoint in the batch: each HTTP
  request (the connectivity probe and every method check) gets its own
  budget, so one slow endpoint fails its own checks with a timeout instead
  of stalling the whole batch.

**Human output** (`--output human`):

```
Miyo RPC Labs v0.6.0
Batch diagnosis: 2 endpoint(s)

  https://soroban-testnet.stellar.org [PASS] 5 passed, 0 failed
  ftp://bad [INVALID] 'ftp://bad': scheme must be http or https

Summary: 2 total, 1 valid, 1 invalid, 1 healthy, 0 failed (exit code 0)
```

**JSON structure** (`--output json`, versioned with `schema_version`):

```json
{
  "schema_version": "1.0",
  "tool_version": "0.6.0",
  "runs": 1,
  "endpoints": [
    {
      "endpoint": "https://soroban-testnet.stellar.org",
      "valid": true,
      "report": { "schema_version": "1.0", "checks": [ ... ], "summary": { ... } }
    },
    {
      "endpoint": "ftp://bad",
      "valid": false,
      "error": "'ftp://bad': scheme must be http or https"
    }
  ],
  "summary": { "total": 2, "valid": 1, "invalid": 1, "healthy": 1, "failed": 0, "exit_code": 1 }
}
```

> Per-endpoint `report` mirrors the single-check `format_json` shape (it also
> carries its own `schema_version`). The top-level aggregate report is versioned
> by the same `schema_version: "1.0"` constant in `src/lib.rs`.

# Public Stellar RPC Endpoint Report — November 2026

**Run date:** 2026-11-23, ~09:50–09:52 WIB (02:50–02:52 UTC)
**Tool:** `miyo-rpc 0.2.0` (built from source, `main` @ `ac4724b`,
`docs/endpoint-report-2026-11` branch), 8 built-in method checks plus
`httpConnectivity`.
**Runs per endpoint:** 3 (`--runs 3`), 15 s timeout per request unless noted.
**Command (per endpoint):**

```bash
miyo-rpc check --url <URL> --output json --runs 3 --expected-network <testnet|futurenet|mainnet>
```

## Endpoint URLs — verified from official Stellar docs at run time

URLs were taken from the official Stellar Developer Docs on 2026-11-23, not
from memory:

- Testnet and Futurenet RPC URLs, and the statement that SDF runs **no public
  Mainnet RPC endpoint** ("Third-party providers only"): [Networks —
  developers.stellar.org/docs/networks](https://developers.stellar.org/docs/networks)
  (page updated 2026-11-11).
- Mainnet endpoint: the [RPC Providers —
  developers.stellar.org/docs/data/apis/rpc/providers](https://developers.stellar.org/docs/data/apis/rpc/providers)
  page (updated 2026-11-11) lists publicly accessible APIs; this run used
  `https://mainnet.sorobanrpc.com` (sorobanrpc.com, Mainnet row, no API key
  required).

| Network | Endpoint | Source |
|---|---|---|
| Testnet | `https://soroban-testnet.stellar.org` | SDF public RPC, Networks page |
| Futurenet | `https://rpc-futurenet.stellar.org` | SDF public RPC, Networks page |
| Mainnet | `https://mainnet.sorobanrpc.com` | Public API, RPC Providers page (SDF runs no public Mainnet RPC) |

## Results

Status per check: **Pass** / **Fail**. Latency is the average across the 3
runs (min–max in parentheses), in milliseconds, as reported by the tool
(`latency_avg_ms` / `latency_min_ms` / `latency_max_ms`). Latency figures are
observed values from this run only, not a benchmark.

### Testnet — `https://soroban-testnet.stellar.org` — 8/9 pass

Latest ledger `5875712`, status `healthy`, retention window 120960 ledgers
(oldest `5754753`), protocol version 30.

| Check | Status | Latency avg (min–max) ms |
|---|---|---|
| httpConnectivity | Pass | 987 (909–1140) |
| getHealth | Pass | 321 (70–824) |
| getNetwork | **Fail** — see note 1 | 79 (75–88) |
| getLatestLedger | Pass | 72 (71–74) |
| getVersionInfo | Pass | 72 (70–77) |
| getEvents | Pass | 100 (71–155) |
| getLedgerEntries | Pass | 72 (71–76) |
| getLedgers | Pass | 86 (72–115) |
| getTransactions | Pass | 71 (70–72) |

### Futurenet — `https://rpc-futurenet.stellar.org` — 9/9 pass

Latest ledger `1755982`, status `healthy`, retention window 120960 ledgers
(oldest `1635023`), protocol version 30.

| Check | Status | Latency avg (min–max) ms |
|---|---|---|
| httpConnectivity | Pass | 914 (856–997) |
| getHealth | Pass | 333 (53–893) |
| getNetwork | Pass | 60 (54–64) |
| getLatestLedger | Pass | 101 (94–110) |
| getVersionInfo | Pass | 56 (54–61) |
| getEvents | Pass | 57 (54–61) |
| getLedgerEntries | Pass | 57 (54–62) |
| getLedgers | Pass | 97 (95–101) |
| getTransactions | Pass | 60 (54–66) |

### Mainnet — `https://mainnet.sorobanrpc.com` — 7/9 pass on first run, 9/9 on retry

Latest ledger `65623241` (first run) / `65623257` (retry), protocol version 29.

| Check | Status (first run) | Latency avg (min–max) ms | Status (retry, 30 s timeout) |
|---|---|---|---|
| httpConnectivity | Pass | 1242 (1000–1608) | Pass |
| getHealth | **Fail** — see note 2 | 547 (541–553) | **Pass** (711 avg, 479–943) |
| getNetwork | **Fail** — see note 2 | 532 (531–533) | **Pass** (700 avg, 484–916; passphrase `Public Global Stellar Network ; September 2015`, protocol 29) |
| getLatestLedger | Pass | 2426 (528–6204) | — |
| getVersionInfo | Pass | 553 (523–598) | — |
| getEvents | Pass | 554 (547–559) | — |
| getLedgerEntries | Pass | 546 (535–556) | — |
| getLedgers | Pass | 566 (546–579) | — |
| getTransactions | Pass | 537 (527–546) | — |

Retry command: `miyo-rpc check --url https://mainnet.sorobanrpc.com
--output json --runs 2 --timeout-secs 30 --method getHealth` (exit 0) and the
same for `--method getNetwork` (exit 0). Only the two failing methods were
re-run; the other six were not re-run because they already passed.

## Notes

1. **Testnet `getNetwork` "Fail" is a bug in this tool, not in the endpoint.**
   The endpoint returned the correct passphrase `Test SDF Network ;
   September 2015` (protocol version 30), matching the official Networks
   page. The tool's `--expected-network testnet` validation failed because
   `src/network.rs` hardcodes the stale passphrase `Test SDF Network ;
   February 2023`. The endpoint is fine; the tool's expected value is wrong.
   Tracked as a follow-up fix (not fixed in this docs-only change).
2. **Mainnet `getHealth` / `getNetwork` first-run failures were transient.**
   Both failed with `error: "transport"`, `error_message: "request timed out
   after 15s"`, while the other six checks against the same endpoint passed
   in the same run. Both passed on retry with a 30 s timeout. Note an
   inconsistency in the tool's own reporting: the failing checks report
   `error_message: "request timed out after 15s"` yet their latency fields
   show ~0.5 s — the timeout is recorded at a different layer than the
   latency statistics, so the two numbers should not be compared directly.
   The provider is also noticeably slower than the SDF endpoints
   (~0.5–0.7 s per method vs ~0.06–0.1 s; `getLatestLedger` averaged
   2426 ms with a 6204 ms max on the first run).
3. **HTTP connectivity probe:** Stellar JSON-RPC endpoints return `405
   Method Not Allowed` for the plain `GET` probe; per the tool's semantics
   this is still a **Pass** (the endpoint is reachable). Testnet/Futurenet
   probes took ~0.9–1.0 s, Mainnet ~1.2 s.
4. **Scope:** read-only diagnostic checks only. No transaction was submitted
   (`sendTransaction` is out of scope for this tool) and no account data was
   written anywhere. `getLedgerEntries` used the tool's fixed synthetic
   all-zero Account probe key, which no real wallet owns.

## How to reproduce

```bash
git clone https://github.com/Miyo-explore/miyo-rpc-labs.git
cd miyo-rpc-labs
cargo build --release
./target/release/miyo-rpc check --url https://soroban-testnet.stellar.org --output json --runs 3 --expected-network testnet
./target/release/miyo-rpc check --url https://rpc-futurenet.stellar.org --output json --runs 3 --expected-network futurenet
./target/release/miyo-rpc check --url https://mainnet.sorobanrpc.com --output json --runs 3 --expected-network mainnet
```

Endpoint URLs may change; re-verify them against
https://developers.stellar.org/docs/networks and
https://developers.stellar.org/docs/data/apis/rpc/providers before re-running.

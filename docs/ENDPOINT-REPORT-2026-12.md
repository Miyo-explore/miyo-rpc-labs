# Public Stellar RPC Endpoint Report — December 2026

**Run date:** 2026-11-26, ~11:19–11:21 UTC (18:19–18:21 WIB)
**Tool:** `miyo-rpc 0.3.1` (built from source, `main` @ `9a1577b`,
`docs/endpoint-report-2026-12` branch), 11 built-in method checks plus
`httpConnectivity` — the three new checks from v0.4.0 (`getFeeStats`,
`getTransaction`, `simulateTransaction`) are covered for the first time.
**Runs per endpoint:** 3 (`--runs 3`), 15 s timeout per request.
**Command (per endpoint):**

```bash
miyo-rpc check --url <URL> --output json --runs 3 --expected-network <testnet|futurenet|mainnet>
```

## Endpoint URLs — verified from official Stellar docs at run time

URLs were taken from the official Stellar Developer Docs on 2026-11-26, not
from memory:

- Testnet and Futurenet RPC URLs, and the statement that SDF runs **no public
  Mainnet RPC endpoint** ("Third-party providers only"): [Networks —
  developers.stellar.org/docs/networks](https://developers.stellar.org/docs/networks)
  (page updated 2026-11-24).
- Mainnet endpoint: the [RPC Providers —
  developers.stellar.org/docs/data/rpc/rpc-providers](https://developers.stellar.org/docs/data/rpc/rpc-providers)
  page lists publicly accessible APIs; this run used
  `https://mainnet.sorobanrpc.com` (sorobanrpc.com, Mainnet row, no API key
  required).

| Network | Endpoint | Source |
|---|---|---|
| Testnet | `https://soroban-testnet.stellar.org` | SDF public RPC, Networks page |
| Futurenet | `https://rpc-futurenet.stellar.org` | SDF public RPC, Networks page |
| Mainnet | `https://mainnet.sorobanrpc.com` | Public API, RPC Providers page (SDF runs no public Mainnet RPC) |

## Results

Status per check: **Pass** / **Warn** / **Skip** / **Fail**. Latency is the
average across the 3 runs (min–max in parentheses), in milliseconds, as
reported by the tool (`latency_avg_ms` / `latency_min_ms` /
`latency_max_ms` inside each check's `details`). Latency figures are
observed values from this run only, not a benchmark.

### Testnet — `https://soroban-testnet.stellar.org` — 12/12 pass

Latest ledger `5933670` (advanced to `5933672` across the 3 runs —
healthy), status `healthy`, retention window 120960 ledgers (oldest
`5812711`), protocol version 30.

| Check | Status | Latency avg (min–max) ms |
|---|---|---|
| httpConnectivity | Pass | 887 (788–999) |
| getHealth | Pass | 304 (92–718) |
| getNetwork | Pass | 95 (87–103) |
| getLatestLedger | Pass | 94 (86–100) |
| getVersionInfo | Pass | 96 (88–102) |
| getEvents | Pass | 108 (92–137) |
| getLedgerEntries | Pass | 89 (74–98) |
| getLedgers | Pass | 95 (93–97) |
| getTransactions | Pass | 108 (75–151) |
| getFeeStats | Pass | 111 (87–146) |
| getTransaction | Pass | 102 (92–112) |
| simulateTransaction | Pass | 189 (174–198) |

New checks on Testnet: `getFeeStats` — Soroban inclusion fee mode 100,
p95 200, 523 transactions; classic inclusion fee mode 100, p95 200;
latest ledger 5933671. `getTransaction` — first transaction in the latest
ledger (`678cce3989098bde…`), on-chain status `SUCCESS`, ledger 5933671,
envelope/result/result-meta XDR all present. `simulateTransaction` —
replayed the embedded Testnet probe (hash
`bbcc753be258d79ad3caa1c66ae150116ab1bc91103b98f7d52a2a4481c38d95`),
`min_resource_fee` 30962, 1 result, latest ledger 5933671.

### Futurenet — `https://rpc-futurenet.stellar.org` — 10/12 pass, 2 skip

Latest ledger `13123` (advanced to `13124` across the 3 runs — healthy),
status `healthy`, protocol version 30. The network was reset recently:
`getHealth` reports oldest ledger `63`, so only ~13k ledgers of history
exist. Version string `31.0.0~vnext-1b9b66e23f434c6baf13eb1ea43b1ea12d1b11cf`
— a bleeding-edge preview build, as expected on Futurenet.

| Check | Status | Latency avg (min–max) ms |
|---|---|---|
| httpConnectivity | Pass | 1115 (682–1762) |
| getHealth | Pass | 361 (92–896) |
| getNetwork | Pass | 162 (89–304) |
| getLatestLedger | Pass | 180 (96–347) |
| getVersionInfo | Pass | 87 (69–97) |
| getEvents | Pass | 98 (95–101) |
| getLedgerEntries | Pass | 99 (96–105) |
| getLedgers | Pass | 90 (71–103) |
| getTransactions | Pass | 133 (96–208) |
| getFeeStats | Pass | 150 (92–262) |
| getTransaction | **Skip** — see note 2 | — |
| simulateTransaction | **Skip** — see note 3 | — |

New checks on Futurenet: `getFeeStats` — Soroban inclusion fee mode 0,
p95 0, 0 transactions (no Soroban traffic on the reset network); classic
inclusion fee mode 100, p95 100; latest ledger 13123.

### Mainnet — `https://mainnet.sorobanrpc.com` — 11/12 pass, 1 skip

Latest ledger `65681194` (advanced to `65681198` across the 3 runs —
healthy), status `healthy`, retention window ~120959 ledgers (oldest
`65560235`), protocol version **30** (was 29 in the November report).

| Check | Status | Latency avg (min–max) ms |
|---|---|---|
| httpConnectivity | Pass | 823 (777–915) |
| getHealth | Pass | 481 (202–950) |
| getNetwork | Pass | 207 (194–232) |
| getLatestLedger | Pass | 260 (194–324) |
| getVersionInfo | Pass | 211 (188–246) |
| getEvents | Pass | 216 (172–289) |
| getLedgerEntries | Pass | 196 (189–202) |
| getLedgers | Pass | 191 (182–198) |
| getTransactions | Pass | 190 (177–199) |
| getFeeStats | Pass | 201 (183–233) |
| getTransaction | Pass | 217 (194–261) |
| simulateTransaction | **Skip** — see note 3 | — |

New checks on Mainnet: `getFeeStats` — Soroban inclusion fee mode 200,
p95 200, 6332 transactions; classic inclusion fee mode 100, p95 100;
latest ledger 65681196. `getTransaction` — first transaction in the latest
ledger (`ee873e8930c85430…`), on-chain status `SUCCESS`, ledger 65681196.
The provider is noticeably faster than in November (~0.2 s per method vs
~0.5 s; `getLatestLedger` averaged 260 ms vs 2426 ms then).

## Notes

1. **Testnet `getNetwork` now passes.** The November report's Fail was a
   bug in this tool (stale expected passphrase in `src/network.rs`);
   it was fixed in v0.3.1 (#32) and the endpoint now validates correctly
   against `Test SDF Network ; September 2015`, protocol 30.
2. **Futurenet `getTransaction` Skip is an empty page, not a failure.**
   The latest ledger (13123) holds no transactions on the freshly reset
   network, so there is no hash to call `getTransaction` with. The check
   Skips by design rather than failing on an empty page.
3. **`simulateTransaction` Skip on Futurenet and Mainnet is by design.**
   The built-in check replays a real `InvokeHostFunction` transaction
   observed on Testnet (hash
   `bbcc753be258d79ad3caa1c66ae150116ab1bc91103b98f7d52a2a4481c38d95`),
   which references a contract that only exists there; on any other
   network the check Skips with that evidence. Arbitrary probing
   (`--method simulateTransaction --params`) remains the path for other
   networks.
4. **No `Warn` appeared in this run.** `getLatestLedger` passed on all
   three endpoints because the 3 back-to-back samples straddled a ledger
   close on each. The new `Warn` tier (v0.4.0) would report "possibly
   stale" if all samples fell inside one ~5 s ledger interval — that is
   a sampling artifact of back-to-back runs, not proof the endpoint is
   frozen; a second invocation a few seconds later is the cross-check.
5. **HTTP connectivity probe:** Stellar JSON-RPC endpoints return `405
   Method Not Allowed` for the plain `GET` probe; per the tool's semantics
   this is still a **Pass** (the endpoint is reachable).
6. **Scope:** read-only diagnostic checks only. No transaction was submitted
   (`sendTransaction` is out of scope for this tool) and no account data was
   written anywhere. `getLedgerEntries` used the tool's fixed synthetic
   all-zero Account probe key, which no real wallet owns. The
   `simulateTransaction` probe replays an already-applied Testnet
   transaction; simulation ignores signatures and sequence numbers, so
   replay is valid and submits nothing.

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
https://developers.stellar.org/docs/data/rpc/rpc-providers before re-running.

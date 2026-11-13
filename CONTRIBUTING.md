# Contributing to Miyo RPC Labs

Thank you for your interest in contributing. This document describes how to
set up a development environment and the standards for contributions.

## Development Setup

```bash
git clone https://github.com/Miyo-explore/miyo-rpc-labs.git
cd miyo-rpc-labs
cargo build
```

Rust 1.75+ is required (edition 2021).

## Code Quality

All contributions must pass CI:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

Do not suppress warnings or skip tests to obtain a green CI result.

## Testing

- All integration tests use `mockito` for local, deterministic mock servers.
- Do **not** add tests that depend on a live Stellar RPC endpoint in the
  default test suite. Optional live tests must be behind a feature flag.
- When fixing a bug, add a regression test that reproduces the failure.

## Pull Requests

1. Create a branch from `main`.
2. Make your change with tests.
3. Ensure `cargo test --all-targets` passes.
4. Open a PR with a clear description of the problem and the fix.
5. Wait for CI to pass. Do not merge until CI is green.

## Drips Wave tasks

Some issues are funded through [Drips Wave](https://www.drips.network/)
and carry the `drips-wave` label plus one `complexity:*` label, which sets
the reward for the task:

| Label | Reward |
| --- | --- |
| `complexity: trivial` | 100 pts |
| `complexity: medium` | 150 pts |
| `complexity: high` | 200 pts |

How to take a Wave task:

1. Pick an open issue labelled `drips-wave` and comment that you are
   taking it, so two people do not work on the same task.
2. Fork the repo and create a branch from `main` (`feat/...`, `fix/...`,
   `test/...`, or `docs/...`).
3. Implement the change with tests. Use the issue template
   (`.github/ISSUE_TEMPLATE/drips_wave_task.md`) acceptance criteria as
   your checklist.
4. Open a PR that links the issue (`Closes #N`) and wait for CI to go
   green and for a maintainer review. Do not merge until CI is green.
5. Rewards for completed Wave tasks are paid in Stellar USDC via Drips.

## License

By contributing, you agree that your contributions are made available under
the Apache-2.0 license (see LICENSE).

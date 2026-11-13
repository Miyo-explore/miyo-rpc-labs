---
name: Drips Wave task
about: A task funded through Drips Wave — pick it up, complete the acceptance criteria, open a PR
title: "[wave] "
labels: ["drips-wave"]
assignees: []
---

## Description

<!-- What needs to be done, and why it matters for the project. Keep it concrete: one task, one outcome. -->

## Acceptance criteria

<!-- Check each box before opening the PR. All of them must hold. -->

- [ ] Change implemented as described above
- [ ] Tests added or updated (see `CONTRIBUTING.md` — no live-endpoint tests in the default suite)
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `cargo test --all-targets` passes
- [ ] README and/or CHANGELOG updated if behavior changed

## Complexity

<!-- Maintainers set one of these labels; it also sets the Drips reward for the task. -->

- `complexity: trivial` — small, well-scoped change (100 pts)
- `complexity: medium` — moderate change, some design decisions (150 pts)
- `complexity: high` — large change, new feature or cross-cutting work (200 pts)

## How to claim

1. Comment on this issue that you are taking it.
2. Fork the repo, create a branch from `main` (`feat/...`, `fix/...`, `test/...`, or `docs/...`).
3. Open a PR that links this issue (`Closes #N`).
4. Wait for CI to go green and for a maintainer review — do not merge until CI is green.
5. Rewards for completed Wave tasks are paid in Stellar USDC via Drips.

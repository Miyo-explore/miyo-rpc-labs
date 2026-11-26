---
name: Contributor task
about: A small, well-scoped task — pick it up, complete the acceptance criteria, open a PR
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

<!-- Maintainers set one of these labels; it helps contributors judge the size of the task. -->

- `complexity: trivial` — small, well-scoped change
- `complexity: medium` — moderate change, some design decisions
- `complexity: high` — large change, new feature or cross-cutting work

## How to claim

1. Comment on this issue that you are taking it.
2. Fork the repo, create a branch from `main` (`feat/...`, `fix/...`, `test/...`, or `docs/...`).
3. Open a PR that links this issue (`Closes #N`).
4. Wait for CI to go green and for a maintainer review — do not merge until CI is green.

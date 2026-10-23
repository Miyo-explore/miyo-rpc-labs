# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial CLI with `check` subcommand and `--url`, `--json`, `--method`,
  `--timeout` options.
- Endpoint connectivity check: HTTP reachability, status code, latency.
- JSON-RPC method checks for `getHealth`, `getNetwork`, `getLatestLedger`,
  and `getVersionInfo`.
- Structured `CheckResult` type with status classification:
  `Pass`, `Fail`, `Skip`, `Unknown`.
- Human-readable and machine-readable (JSON) output formats.
- Automatic URL sanitization that redacts query parameters matching
  secret-like patterns (`api_key`, `token`, `secret`, `key`).
- `mockito`-based integration tests covering success, HTTP errors,
  JSON-RPC errors, malformed JSON, type mismatches, connection failures,
  and URL redaction.

### Security

- `.gitignore` excludes `.env`, `*.pem`, `*.key`, and other credential
  files.
- No credentials appear in fixtures or test data.

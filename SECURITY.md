# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |

## Reporting a Vulnerability

**Please do not report security vulnerabilities through the public GitHub
issue tracker.**

To report a security issue:

1. Email the maintainer at `al7795927@gmail.com` with:
   - A description of the vulnerability and potential impact.
   - Steps to reproduce.
   - Your name/handle for attribution (optional).
2. You will receive a response within 72 hours. If the issue is confirmed,
   a patch will be prepared and you will be notified of the release schedule.

## Scope

In scope:
- Credential leakage in tool output or logs.
- URL sanitization bypass that exposes secrets.

Out of scope:
- Theoretical vulnerabilities in upstream dependencies (report those
  directly to the crate maintainers).
- Issues requiring access to a paid or private RPC endpoint.

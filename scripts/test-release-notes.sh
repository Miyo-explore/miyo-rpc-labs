#!/usr/bin/env bash
# Regression tests for scripts/extract-changelog-section.sh.
#
# Focuses on exact version matching: a tag must never match a heading that
# merely shares a version prefix (e.g. `0.1` vs `0.1.0` / `0.10.0`), and a
# prerelease heading (`0.1.0-alpha`) must not be returned for `0.1.0`.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
extract="$here/extract-changelog-section.sh"

fail=0

# Strip leading and trailing blank lines so assertions do not depend on the
# changelog's blank-line padding around headings.
trim() {
  awk '
    { lines[NR] = $0 }
    END {
      start = 1; while (start <= NR && lines[start] == "") start++
      end = NR;   while (end >= start && lines[end] == "") end--
      for (i = start; i <= end; i++) print lines[i]
    }
  '
}

assert_eq() {
  local label="$1" got="$2" want="$3"
  if [ "$got" != "$want" ]; then
    echo "FAIL: $label"
    echo "  expected: [$want]"
    echo "  got     : [$got]"
    fail=1
  else
    echo "PASS: $label"
  fi
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Fixture with versions that share prefixes, newest-first (Keep a Changelog).
cat > "$tmp/CHANGELOG.md" <<'EOF'
## [Unreleased]

## [0.10.0] - 2026-11-01

0.10.0 content line.

## [0.1.0] - 2026-10-23

0.1.0 content line.

## [0.1.0-alpha] - 2026-09-01

0.1.0-alpha content line.
EOF

# 0.1 must NOT match 0.1.0 or 0.10.0 (the old prefix match failed here).
assert_eq "tag 0.1 extracts nothing (no exact heading)" \
  "$(bash "$extract" 0.1 "$tmp/CHANGELOG.md" | trim)" ""

# 0.1.0 must extract exactly its own section, not 0.10.0 or 0.1.0-alpha.
assert_eq "tag 0.1.0 extracts its own section only" \
  "$(bash "$extract" 0.1.0 "$tmp/CHANGELOG.md" | trim)" "0.1.0 content line."

# 0.10.0 must extract exactly its own section.
assert_eq "tag 0.10.0 extracts its own section only" \
  "$(bash "$extract" 0.10.0 "$tmp/CHANGELOG.md" | trim)" "0.10.0 content line."

# 0.1.0-alpha must extract exactly its own prerelease section.
assert_eq "tag 0.1.0-alpha extracts the prerelease section" \
  "$(bash "$extract" 0.1.0-alpha "$tmp/CHANGELOG.md" | trim)" "0.1.0-alpha content line."

# A tag absent from the changelog yields empty output (caller applies fallback).
assert_eq "unknown tag extracts nothing" \
  "$(bash "$extract" 9.9.9 "$tmp/CHANGELOG.md" | trim)" ""

# Real CHANGELOG sanity: 0.1.0 must be non-empty and must not leak Unreleased.
real="$(bash "$extract" 0.1.0)"
case "$real" in
  *"Unreleased"*) echo "FAIL: real 0.1.0 leaked the Unreleased section"; fail=1 ;;
  "")            echo "FAIL: real 0.1.0 section is empty"; fail=1 ;;
  *)             echo "PASS: real 0.1.0 section extracted ($(printf '%s\n' "$real" | wc -l) lines, no Unreleased leak)" ;;
esac

if [ "$fail" -ne 0 ]; then
  echo "extract-changelog-section tests FAILED"
  exit 1
fi
echo "all extract-changelog-section tests passed"

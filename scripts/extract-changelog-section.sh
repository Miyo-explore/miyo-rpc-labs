#!/usr/bin/env bash
# Print the CHANGELOG.md section for an exact version.
#
# Matches the version heading precisely, so a tag like `0.1` never pulls the
# `0.1.0` (or `0.10.0`) section, and a prerelease heading such as
# `## [0.1.0-alpha]` is only returned for tag `0.1.0-alpha`.
#
# Usage: extract-changelog-section.sh <version> [changelog-file]
set -euo pipefail

VERSION="${1:?usage: extract-changelog-section.sh <version> [changelog-file]}"
FILE="${2:-CHANGELOG.md}"

awk -v tag="$VERSION" '
  /^## \[/ {
    v = $0
    sub(/^## \[/, "", v)   # strip leading "## ["
    sub(/\].*$/, "", v)     # strip from the first "]" to end of line
    in_sec = (v == tag) ? 1 : 0
    next
  }
  in_sec { print }
' "$FILE"

#!/usr/bin/env bash
# Print one CHANGELOG.md section body: `scripts/release-notes.sh 0.1.0` or
# `scripts/release-notes.sh Unreleased`. Fails when the section is missing or empty.
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 <version|Unreleased> [changelog]" >&2
  exit 2
fi
section="$1"
changelog="${2:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/CHANGELOG.md}"

body="$(awk -v heading="## [${section}]" '
  index($0, heading) == 1 { found = 1; next }
  found && /^## \[/ { exit }
  found { print }
' "$changelog")"

# Trim leading and trailing blank lines.
body="$(printf '%s\n' "$body" | sed -e '/./,$!d' | tac | sed -e '/./,$!d' | tac)"
if [[ -z "$body" ]]; then
  echo "CHANGELOG section [$section] is missing or empty in $changelog" >&2
  exit 1
fi
printf '%s\n' "$body"

#!/bin/bash
# Strips #[cfg(test)] module bodies from Rust source files.
# Outputs only production code lines (no #[cfg(test)] module bodies).
#
# Usage:
#   scripts/strip-cfg-test.sh <file.rs>           # strip test modules, print production code
#   scripts/strip-cfg-test.sh <file.rs> | wc -l    # count production lines
#
# The script tracks brace depth from the #[cfg(test)] attribute to the
# matching closing brace, correctly handling nested braces inside test modules.
# It resets state per file (FNR == 1) so it works with multiple files via xargs.

awk '
  FNR == 1 { skip = 0; depth = 0; seen_brace = 0 }
  /#\[cfg\(test\)\]/ {
    skip = 1; depth = 0; seen_brace = 0
    line = $0
    for (i = 1; i <= length(line); i++) {
      c = substr(line, i, 1)
      if (c == "{") { depth++; seen_brace = 1 }
      if (c == "}") depth--
    }
    if (seen_brace && depth <= 0) skip = 0
    next
  }
  skip {
    line = $0
    for (i = 1; i <= length(line); i++) {
      c = substr(line, i, 1)
      if (c == "{") { depth++; seen_brace = 1 }
      if (c == "}") depth--
    }
    # Handle #[cfg(test)] mod tests; (declaration without braces)
    if (!seen_brace && line ~ /;[[:space:]]*$/) { skip = 0; next }
    if (seen_brace && depth <= 0) skip = 0
    next
  }
  { print }
' "$1"

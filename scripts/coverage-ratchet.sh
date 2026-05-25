#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

coverage_dir="${COVERAGE_ARTIFACT_DIR:-target/coverage}"
lcov_path="${COVERAGE_LCOV_PATH:-$coverage_dir/lcov.info}"
summary_path="${COVERAGE_SUMMARY_PATH:-$coverage_dir/summary.txt}"
baseline_path="${COVERAGE_BASELINE_PATH:-docs/test-suite-coverage-baseline.txt}"
nextest_profile="${NEXTEST_PROFILE:-ci}"

mkdir -p "$coverage_dir" "$(dirname "$baseline_path")"

if ! cargo llvm-cov --version >/dev/null 2>&1; then
  cat >&2 <<'MSG'
cargo-llvm-cov is required for the coverage ratchet lane.
Install it with `cargo install cargo-llvm-cov --locked` and ensure the rustup
`llvm-tools-preview` component is installed for the active toolchain.
MSG
  exit 2
fi

# cargo-llvm-cov runs the builtin nextest integration; NEXTEST_PROFILE selects
# the repository profile from .config/nextest.toml.
NEXTEST_PROFILE="$nextest_profile" \
  cargo llvm-cov nextest --workspace --all-features --lcov --output-path "$lcov_path"

python3 - "$lcov_path" "$baseline_path" "$summary_path" <<'PY'
from __future__ import annotations

import sys
from pathlib import Path

lcov_path = Path(sys.argv[1])
baseline_path = Path(sys.argv[2])
summary_path = Path(sys.argv[3])

lines_found = 0
lines_hit = 0
for line in lcov_path.read_text(errors="ignore").splitlines():
    if line.startswith("LF:"):
        lines_found += int(line.split(":", 1)[1])
    elif line.startswith("LH:"):
        lines_hit += int(line.split(":", 1)[1])

if lines_found == 0:
    raise SystemExit("coverage ratchet could not find LF/LH totals in lcov output")

# LLVM coverage can vary by a handful of lines when independently scheduled
# integration tests take different diagnostic branches. Compare the ratchet at
# two decimal places so sub-basis-point noise does not fail an otherwise
# unchanged suite, while still catching meaningful coverage regressions.
percent = (lines_hit / lines_found) * 100.0
rounded_percent = round(percent, 4)
comparison_percent = round(percent, 2)
if baseline_path.exists():
    baseline = float(baseline_path.read_text().strip())
    status = "PASS" if comparison_percent + 1e-9 >= baseline else "FAIL"
else:
    baseline = comparison_percent
    baseline_path.write_text(f"{comparison_percent:.2f}\n")
    status = "BASELINE-RECORDED"

summary = (
    f"coverage_status={status}\n"
    f"line_coverage_percent={rounded_percent:.4f}\n"
    f"baseline_percent={baseline:.4f}\n"
    f"lines_hit={lines_hit}\n"
    f"lines_found={lines_found}\n"
    f"lcov_path={lcov_path}\n"
    f"baseline_path={baseline_path}\n"
)
summary_path.write_text(summary)
print(summary, end="")

if status == "FAIL":
    raise SystemExit(1)
PY

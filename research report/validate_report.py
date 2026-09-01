# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///
# ─── How to run ───
# uv run .omo/ulw-research/20260831-044447/validate_report.py \
#   .omo/ulw-research/20260831-044447/grok-build-harness-parity-audit.md \
#   .omo/ulw-research/20260831-044447/claim-graph.md \
#   .omo/ulw-research/20260831-044447/debate-log.md

from pathlib import Path
import re
import sys
from typing import Final


REQUIRED_HEADINGS: Final = (
    "## Executive summary",
    "## What 1:1 parity means",
    "## Prioritized findings",
    "## Detailed comparison by surface",
    "## Implementation roadmap",
    "## Verification matrix",
    "## Unresolved and refuted claims",
    "## Methodology",
)
REQUIRED_FIELDS: Final = (
    "**What**:",
    "**Why**:",
    "**Where**:",
    "**How**:",
    "**Verify**:",
    "**Dependencies**:",
    "**Risks**:",
)


def validation_errors(report: str, claims: str, debates: str) -> tuple[str, ...]:
    """Return every report-contract violation."""
    errors: list[str] = []
    for heading in REQUIRED_HEADINGS:
        if heading not in report:
            errors.append(f"missing heading: {heading}")

    if "STATUS: draft" in report or "Pending." in report:
        errors.append("report is still a draft")

    findings = re.findall(
        r"(?ms)^### (P[01]-\d+).*?(?=^### P[01]-\d+|^## |\Z)",
        report,
    )
    if len(findings) < 8:
        errors.append("fewer than eight P0/P1 findings")

    finding_blocks = re.findall(
        r"(?ms)^### P[01]-\d+.*?(?=^### P[01]-\d+|^## |\Z)",
        report,
    )
    for index, block in enumerate(finding_blocks, start=1):
        for field in REQUIRED_FIELDS:
            if field not in block:
                errors.append(f"finding {index} missing {field}")
        if re.search(r"\bC\d{2,}\b", block) is None:
            errors.append(f"finding {index} missing claim id")

    harness_refs = re.findall(r"`crates/harness-tui/[^`]+:\d+`", report)
    grok_refs = re.findall(r"`inspirations/grok-build/[^`]+:\d+`", report)
    if len(harness_refs) < 12:
        errors.append("fewer than twelve Harness file:line references")
    if len(grok_refs) < 12:
        errors.append("fewer than twelve Grok Build file:line references")

    branding = report.lower()
    if "preserve harness" not in branding or "logo" not in branding:
        errors.append("branding-preservation constraint is missing")

    supported_claims = len(re.findall(r"\|\s*C\d{2,}\s*\|.*\|\s*supported\s*\|", claims))
    if supported_claims < len(findings):
        errors.append("supported claim count is below priority finding count")

    debate_rows = [
        line
        for line in debates.splitlines()
        if line.startswith("| D") and line.count("|") >= 6
    ]
    if len(debate_rows) < len(findings):
        errors.append("debate coverage is below priority finding count")

    return tuple(errors)


def main() -> int:
    """Validate a final parity report and its evidence ledgers."""
    if len(sys.argv) != 4:
        print("usage: validate_report.py REPORT CLAIM_GRAPH DEBATE_LOG")
        return 2

    report_path, claims_path, debates_path = map(Path, sys.argv[1:])
    errors = validation_errors(
        report_path.read_text(encoding="utf-8"),
        claims_path.read_text(encoding="utf-8"),
        debates_path.read_text(encoding="utf-8"),
    )
    if errors:
        print("FAIL")
        for error in errors:
            print(f"- {error}")
        return 1

    print("PASS: report contract and evidence ledgers are consistent")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Fail when forbidden source-brand terms appear in checked files."""
from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterator
    from typing import Final

sys.dont_write_bytecode = True

ALLOWED_DIRS: Final[set[str]] = {".git", ".sisyphus", "inspirations", "target"}
# Files and PRDs that intentionally reference the upstream terminal product
# while implementing command-palette/provider parity.
ALLOWED_PARITY_PATHS: Final[set[Path]] = {
    Path("crates/harness-tui/src/keybindings/parity_matrix.rs"),
    Path("crates/harness-core/src/coord/formatter/real_discovery.rs"),
    Path("crates/harness-core/src/coord/formatter/real_discovery/support.rs"),
    Path("crates/harness-core/src/coord/formatter/real_discovery/rule_tests.rs"),
    Path("crates/harness-core/src/coord/formatter/real_discovery/tests.rs"),
    Path("crates/harness-core/src/coord/formatter/registry.rs"),
    Path("crates/harness-core/src/coord/formatter/resolver.rs"),
    Path("crates/harness-core/src/coord/task_lifecycle.rs"),
    Path("crates/harness-tui/src/app/auth_dialog/provider_menu.rs"),
    Path("crates/harness-tui/src/app/auth_dialog/tests.rs"),
    Path("crates/harness-tui/src/app/palette_controller.rs"),
    Path("crates/harness-tui/src/app/tests.rs"),
    Path("crates/harness-tui/src/app/tests/palette_parity_tests.rs"),
    Path("crates/harness-tui/src/app/tests/opencode_subagent_parity_apps_test.rs"),
    Path("crates/harness-tui/src/app/tests/opencode_subagent_parity_event_helpers_test.rs"),
    Path("crates/harness-tui/src/app/tests/opencode_subagent_parity_events_test.rs"),
    Path("crates/harness-tui/src/app/tests/opencode_subagent_parity_evidence_test.rs"),
    Path("crates/harness-tui/src/app/tests/subagent_footer_navigation_tests.rs"),
    Path("crates/harness-tui/src/keybindings/palette_model.rs"),
    Path("crates/harness-tui/src/keybindings/paritymatrix.rs"),
    Path("crates/harness-tui/src/layout.rs"),
    Path("crates/harness-tui/src/ui_chrome_exact_tests.rs"),
    Path("crates/harness-tui/src/ui_subagent_footer_entry_body_tests.rs"),
    Path("crates/harness-tui/src/ui_subagent_footer_exact_tests.rs"),
    Path("crates/harness-tui/src/ui_transcript_exact_tests/task_rows.rs"),
    Path("docs/chat-rendering-parity.md"),
    Path("docs/ctrl-p-command-palette-parity-plan.md"),
    Path("docs/onboarding-terminal-migration-prd.md"),
    Path("docs/parity-gaps/opencode-parity-gaps.md"),
}
ALLOWED_MATCH_LINES: Final[dict[Path, set[int]]] = {
    Path("scripts/check-forbidden-branding.py"): set(),
    Path("configs/config.json"): {710, 835},
    Path("configs/provider-catalog.generated.json"): {1},
    Path("crates/harness-core/src/config/public.rs"): {
        60, 193, 315, 477, 483, 489, 497, 660,
    },
    Path("crates/harness-core/src/config/public/normalization.rs"): {262},
    Path("crates/harness-core/src/coord/formatter/real_discovery.rs"): {1, 21},
    Path("crates/harness-core/src/coord/formatter/registry.rs"): {1, 3, 60},
    Path("crates/harness-core/src/coord/formatter/resolver.rs"): {17},
    Path("crates/harness-tui/src/keybindings.rs"): {595},
    Path("crates/harness/src/models.rs"): {17},
    Path("crates/harness/tests/config_docs_reference_test.rs"): {95},
    Path(
        "crates/harness/tests/config_schema_cli/"
        "03_config_validate_cli_loads_separate_tui_test.rs"
    ): {202, 203, 212, 244, 277, 299},
    Path("docs/config.md"): {
        73, 89, 100, 118, 134, 135, 136, 137, 139, 140,
        141, 142, 143, 145, 146, 147, 149, 153, 156, 157,
        158, 160, 162, 163, 164, 165, 262, 282, 413, 414,
    },
    Path("docs/test-suite-prd.md"): {
        7, 139, 154, 157, 427, 433, 456, 487,
        648, 871, 873, 874,
    },
}
ALLOWED_MATCH_TEXT: Final[dict[Path, set[str]]] = {
    Path("docs/test-suite-prd.md"): {
        "- `inspirations/"
        "open"
        "code"
        "/packages/http-recorder/README.md` + `src/*` + `test/record-replay.test.ts`,",
        "`packages/"
        "open"
        "code"
        "/test/cli/cmd/tui/attention.test.ts`,",
    },
}
SOURCE_PREFIX: Final[str] = "p" + "i"
FORBIDDEN_PATTERNS: Final[list[re.Pattern[str]]] = [
    re.compile(pattern, re.IGNORECASE)
    for pattern in (
        "open" + "code",
        r"open\s+code",
        SOURCE_PREFIX + "-mono",
        SOURCE_PREFIX + r"\s+mono",
        r"\b" + SOURCE_PREFIX + r"\b",
    )
]


class ParsedArgs(argparse.Namespace):
    """Typed namespace for parsed CLI arguments."""

    root: Path | None
    root_opt: Path | None

    def __init__(self) -> None:
        """Initialize with default values."""
        super().__init__()
        self.root = None
        self.root_opt = None


def parse_root() -> Path:
    """Parse the repository root from CLI arguments."""
    parser = argparse.ArgumentParser(
        description="Scan repository files for forbidden source-brand terms."
    )
    _ = parser.add_argument(
        "root",
        type=Path,
        nargs="?",
        default=None,
        help="Repository root to scan.",
    )
    _ = parser.add_argument(
        "--root",
        dest="root_opt",
        type=Path,
        default=None,
        help="Repository root to scan. Defaults to this script's workspace root.",
    )
    args = parser.parse_args(namespace=ParsedArgs())
    return args.root_opt or args.root or Path(__file__).resolve().parents[1]


def is_allowed(path: Path) -> bool:
    """Return True if the path is exempt from branding checks."""
    if path.name == "check-forbidden-branding.py":
        return True
    if path in ALLOWED_PARITY_PATHS:
        return True
    if any(part in ALLOWED_DIRS for part in path.parts):
        return True
    name = path.name
    oc = "open" + "code"
    if path.parent.name == "docs":
        if name.startswith(oc + "-tools-parity"):
            return True
        vendor_ui_backend_suffix = "_ui_" + "p" + "i" + "_backend_prd.md"
        if (
            name.startswith("agent_harness_")
            and name.endswith(vendor_ui_backend_suffix)
        ):
            return True
        if name in (
            "roadmap-v1.md",
            oc + "-tui-parity.md",
            oc + "-visual-tool-parity-prd.md",
            "hyperplan-desktop-app-" + oc + "-feel.md",
            "agent_harness_" + oc + "_ui_" + "p" + "i"
            + "_backend_prd_missing_specs.md",
            "config-restructure-prompt.md",
            "config-restructure-spec.md",
        ):
            return True
    return name.startswith(oc + "_tools_parity_inventory") and name.endswith(
        ".v1.json"
    )


def is_allowed_match(relative: Path, line_number: int, line: str) -> bool:
    """Return True if a forbidden match is explicitly allowlisted."""
    return (
        line_number in ALLOWED_MATCH_LINES.get(relative, set())
        or line.strip() in ALLOWED_MATCH_TEXT.get(relative, set())
    )


def is_binary(path: Path) -> bool:
    """Return True if the file appears to be binary."""
    try:
        with path.open("rb") as handle:
            return b"\0" in handle.read(4096)
    except OSError:
        return True


def iter_files(root: Path) -> Iterator[tuple[Path, Path]]:
    """Yield (absolute, relative) paths for files to scan."""
    git_paths = git_repository_files(root)
    if git_paths is not None:
        for relative in git_paths:
            if not is_allowed(relative):
                yield root / relative, relative
        return

    for current_root, dir_names, file_names in os.walk(root):
        current = Path(current_root)
        dir_names[:] = [name for name in dir_names if name not in ALLOWED_DIRS]
        if is_allowed(current.relative_to(root)):
            continue
        for file_name in file_names:
            path = current / file_name
            relative = path.relative_to(root)
            if not is_allowed(relative):
                yield path, relative


def git_repository_files(root: Path) -> list[Path] | None:
    """Return tracked files via git ls-files, or None if git is unavailable."""
    try:
        # git is a trusted system command; PATH lookup is intentional
        result = subprocess.run(  # noqa: S603
            [  # noqa: S607
                "git",
                "-C",
                str(root),
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
    except (OSError, subprocess.CalledProcessError):
        return None

    if not result.stdout:
        return []

    return [
        Path(item.decode("utf-8"))
        for item in result.stdout.split(b"\0")
        if item
    ]


def matches_for_file(path: Path, relative: Path) -> list[str]:
    """Return a list of forbidden-brand findings for the given file."""
    if is_binary(path):
        return []

    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return []
    except OSError as error:
        return [f"{relative}: failed to read: {error}"]

    matches: list[str] = []
    for line_number, line in enumerate(text.splitlines(), start=1):
        for pattern in FORBIDDEN_PATTERNS:
            if pattern.search(line):
                if is_allowed_match(relative, line_number, line):
                    break
                matches.append(f"{relative}:{line_number}: forbidden brand term")
                break
    return matches


def main() -> int:
    """Scan repository files for forbidden source-brand terms."""
    try:
        root = parse_root().resolve()
        if not root.is_dir():
            print(f"scan root does not exist: {root}", file=sys.stderr)
            return 2

        findings: list[str] = []
        for path, relative in iter_files(root):
            findings.extend(matches_for_file(path, relative))

        if findings:
            print(
                "Forbidden source-brand terms found outside allowed paths:",
                file=sys.stderr,
            )
            for finding in findings:
                print(f"  {finding}", file=sys.stderr)
            return 1

        print("No forbidden source-brand terms found outside allowed paths.")
        return 0
    finally:
        shutil.rmtree(
            Path(__file__).with_name("__pycache__"), ignore_errors=True
        )


if __name__ == "__main__":
    raise SystemExit(main())

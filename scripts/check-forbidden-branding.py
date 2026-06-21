#!/usr/bin/env python3
"""Fail when forbidden source-brand terms appear in checked files."""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from collections.abc import Iterator
from pathlib import Path
from typing import cast

sys.dont_write_bytecode = True


ALLOWED_DIRS = {".git", ".sisyphus", "inspirations", "target"}
ALLOWED_MATCH_LINES = {
    Path("scripts/check-forbidden-branding.py"): {118, 119, 120, 121, 122, 123, 124},
    Path("configs/config.json"): {710, 835},
    Path("configs/provider-catalog.generated.json"): {1},
    Path("crates/harness-core/src/config/public.rs"): {60, 193, 315, 477, 483, 489, 497},
    Path("crates/harness-tui/src/keybindings.rs"): {595},
    Path("crates/harness/src/models.rs"): {17},
    Path("crates/harness/tests/config_docs_reference_test.rs"): {95},
    Path("crates/harness/tests/config_schema_cli/03_config_validate_cli_loads_separate_tui_test.rs"): {
        202,
        203,
        212,
        244,
        277,
        299,
    },
    Path("docs/config.md"): {
        73,
        89,
        100,
        118,
        134,
        135,
        136,
        137,
        139,
        140,
        141,
        142,
        143,
        145,
        146,
        147,
        149,
        153,
        156,
        157,
        158,
        160,
        162,
        163,
        164,
        165,
        262,
        282,
        413,
        414,
    },
    Path("docs/test-suite-prd.md"): {7, 139, 154, 157, 427, 433, 456, 487, 648, 871, 873, 874},
}
ALLOWED_MATCH_TEXT = {
    Path("docs/test-suite-prd.md"): {
        "- `inspirations/"
        + "open"
        + "code"
        + "/packages/http-recorder/README.md` + `src/*` + `test/record-replay.test.ts`,",
        "`packages/"
        + "open"
        + "code"
        + "/test/cli/cmd/tui/attention.test.ts`,",
    },
}
SOURCE_PREFIX = "p" + "i"
FORBIDDEN_PATTERNS = [
    re.compile(pattern, re.IGNORECASE)
    for pattern in (
        "open" + "code",
        r"open\s+code",
        SOURCE_PREFIX + "-mono",
        SOURCE_PREFIX + r"\s+mono",
        r"\b" + SOURCE_PREFIX + r"\b",
    )
]


def parse_root() -> Path:
    parser = argparse.ArgumentParser(
        description="Scan repository files for forbidden source-brand terms."
    )
    _ = parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="Repository root to scan. Defaults to this script's workspace root.",
    )
    args = parser.parse_args()
    root = cast(object, getattr(args, "root"))
    if isinstance(root, Path):
        return root
    return Path(str(root))


def is_allowed(path: Path) -> bool:
    if any(part in ALLOWED_DIRS for part in path.parts):
        return True
    if path.parent.name == "docs":
        name = path.name
        if name.startswith("agent_harness_") and name.endswith("_ui_pi_backend_prd.md"):
            return True
        if name in (
            "roadmap-v1.md",
            "opencode-tui-parity.md",
            "opencode-visual-tool-parity-prd.md",
            "hyperplan-desktop-app-opencode-feel.md",
            "agent_harness_opencode_ui_pi_backend_prd_missing_specs.md",
            "config-restructure-prompt.md",
            "config-restructure-spec.md",
        ):
            return True
    return False


def is_allowed_match(relative: Path, line_number: int, line: str) -> bool:
    return line_number in ALLOWED_MATCH_LINES.get(relative, set()) or line.strip() in ALLOWED_MATCH_TEXT.get(relative, set())


def is_binary(path: Path) -> bool:
    try:
        with path.open("rb") as handle:
            return b"\0" in handle.read(4096)
    except OSError:
        return True


def iter_files(root: Path) -> Iterator[tuple[Path, Path]]:
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
    try:
        result = subprocess.run(
            [
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

    paths: list[Path] = []
    for item in result.stdout.split(b"\0"):
        if item:
            paths.append(Path(item.decode("utf-8")))
    return paths


def matches_for_file(path: Path, relative: Path) -> list[str]:
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
    try:
        root = parse_root().resolve()
        if not root.is_dir():
            print(f"scan root does not exist: {root}", file=sys.stderr)
            return 2

        findings: list[str] = []
        for path, relative in iter_files(root):
            findings.extend(matches_for_file(path, relative))

        if findings:
            print("Forbidden source-brand terms found outside allowed paths:", file=sys.stderr)
            for finding in findings:
                print(f"  {finding}", file=sys.stderr)
            return 1

        print("No forbidden source-brand terms found outside allowed paths.")
        return 0
    finally:
        shutil.rmtree(Path(__file__).with_name("__pycache__"), ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())

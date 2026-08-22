#!/usr/bin/env python3
"""Static gates for the deterministic test-suite overhaul.

The default command is intentionally strict and returns non-zero when
violations are present. Use --report-only during migration to capture the
current baseline without claiming acceptance.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tempfile
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterable
    from typing import Final

GATES: Final[tuple[str, ...]] = (
    "no-sleeps",
    "no-global-state",
    "no-real-world-deps",
    "live-provider-env",
    "file-focus",
    "cassette-secrets",
    "orphan-snapshots",
    "taxonomy",
    "test-names",
    "path-isolation",
    "t5-line-budget",
    "conventions",
)

DEFAULT_T5_LINE_BUDGET: Final[int] = 4_000
DEFAULT_MAX_LINES: Final[int] = 800
MIN_TEST_NAME_WORDS: Final[int] = 4
MAX_DISPLAYED_VIOLATIONS: Final[int] = 300
CONVENTIONS_BASELINE_PATH: Final[Path] = Path(
    "docs/testing/test-suite-conventions-baseline.json"
)

PROCESS_GLOBAL_STATE_EXEMPTIONS: Final[set[str]] = {
    "crates/harness-core/src/provider_catalog.rs",
    "crates/harness-core/src/coord/tests/workspace_snapshot_tests.rs",
    "crates/harness-core/tests/poc_candidate3_catalog_poisoning_test.rs",
    "crates/harness-core/tests/browser_oidc_test.rs",
}

PATTERNS: Final[dict[str, list[re.Pattern[str]]]] = {
    "no-sleeps": [
        re.compile(r"\bstd::thread::sleep\b"),
        re.compile(r"\bthread::sleep\b"),
        re.compile(r"\btokio::time::sleep\b"),
        re.compile(r"\bsleep\s*\("),
    ],
    "no-global-state": [
        re.compile(r"\bstd::env::set_var\s*\("),
        re.compile(r"\benv::set_var\s*\("),
        re.compile(r"\bset_process_env_var\s*\("),
        re.compile(r"\bstd::env::remove_var\s*\("),
        re.compile(r"\benv::remove_var\s*\("),
        re.compile(r"\bremove_process_env_var\s*\("),
        re.compile(r"\bstd::env::set_current_dir\s*\("),
        re.compile(r"\benv::set_current_dir\s*\("),
        re.compile(r"\bstd::env::current_dir\s*\("),
        re.compile(r"\benv::current_dir\s*\("),
        re.compile(r"\bEnvGuard::set\s*\("),
    ],
    "no-real-world-deps": [
        re.compile(r"\bstd::process::Command::new\s*\("),
        re.compile(r"\btokio::process::Command::new\s*\("),
        re.compile(r"\bCommand::new\s*\("),
        re.compile(r"\bProcessCommand::new\s*\("),
        re.compile(r"CARGO_BIN_EXE_"),
        re.compile(r"\bTcpListener::bind\s*\("),
        re.compile(r"\b[A-Za-z_][A-Za-z0-9_]*TcpListener::bind\s*\("),
        re.compile(r"\bTcpStream::connect"),
        re.compile(r"\bMockServer::start\s*\("),
        re.compile(r"\bwiremock::"),
        re.compile(r"\bportable_pty\b"),
    ],
}

SECRET_PATTERNS: Final[dict[str, re.Pattern[str]]] = {
    "openai_api_key": re.compile(r"\bsk-[A-Za-z0-9_-]{10,}\b"),
    "anthropic_api_key": re.compile(r"\bsk-ant-[A-Za-z0-9_-]{10,}\b"),
    "google_api_key": re.compile(r"\bAIza[0-9A-Za-z_-]{20,}\b"),
    "aws_access_key_id": re.compile(r"\bAKIA[0-9A-Z]{16}\b"),
    "github_pat": re.compile(r"\bgithub_pat_[A-Za-z0-9_]{20,}\b"),
    "github_token": re.compile(r"\bghp_[A-Za-z0-9]{20,}\b"),
    "authorization_bearer": re.compile(
        r'\bauthorization"?\s*:\s*"?bearer\s+[A-Za-z0-9._~+/=-]{8,}',
        re.IGNORECASE,
    ),
    "pem_private_key": re.compile(
        r"-----BEGIN [A-Z ]*PRIVATE KEY-----"
    ),
}

TEST_ATTRIBUTE: Final[re.Pattern[str]] = re.compile(
    r"\s*#\[(?:tokio::)?test(?:\([^]]*\))?\]"
)
TEST_FUNCTION: Final[re.Pattern[str]] = re.compile(
    r"(?:async\s+)?fn\s+([A-Za-z0-9_]+)\s*\("
)
INSTA_ASSERTION: Final[re.Pattern[str]] = re.compile(
    r"\binsta::assert_[A-Za-z0-9_]*snapshot!\s*\("
)
REQUIRED_SECTION_MARKERS: Final[tuple[str, ...]] = (
    "// arrange", "// act", "// assert"
)
LIVE_PROVIDER_ENV: Final[re.Pattern[str]] = re.compile(
    r"HARNESS_LIVE_PROXY(?:_CONFIG|_PROVIDER|_MODEL)?"
)

T5_PATH_PARTS: Final[tuple[str, ...]] = (
    "crates/harness-testkit/src/bin/native_visual_helper.rs",
    "crates/harness-testkit/tests/pty_e2e.rs",
    "crates/harness-testkit/tests/live_proxy_e2e.rs",
    "crates/harness-testkit/tests/native_visual_e2e.rs",
    "crates/harness-testkit/tests/support/harness_bin.rs",
    "crates/harness-testkit/tests/support/live_",
    "crates/harness-testkit/tests/support/native_visual.rs",
    "crates/harness-testkit/tests/support/native_visual/",
    "crates/harness-testkit/tests/support/native_visual_",
    "crates/harness-testkit/tests/support/native_visual_e2e_impl.rs",
    "crates/harness-testkit/tests/support/pty_",
    "crates/harness/tests/binary_smoke.rs",
    "crates/harness/tests/pty_happy_path_recorded.rs",
    "crates/harness-tui/tests/pty_e2e.rs",
    "crates/harness-tui/tests/support/pty_e2e_impl.rs",
)

HOST_PATH_LITERAL: Final[re.Pattern[str]] = re.compile(
    r'"/(?:tmp|var|home|srv|Users|private/tmp)[^"\\n]*"'
)
FS_LITERAL_ACCESS: Final[re.Pattern[str]] = re.compile(
    r"\b(?:std::fs::|fs::|File::|OpenOptions::)"
    r"(?:read|read_to_string|write|create_dir|create_dir_all"
    r"|read_dir|copy|remove_file|remove_dir_all"
    r"|open|create|new)\s*\("
)

TAXONOMY_SUFFIXES: Final[tuple[str, ...]] = (
    "_test.rs",
    "_tests.rs",
    "_regression.rs",
    "_repro.rs",
    "_recorded.rs",
    "_perf.rs",
    "_smoke.rs",
    "_support.rs",
)


@dataclass(frozen=True, slots=True)
class Violation:
    """A single gate violation finding."""

    gate: str
    path: str
    line: int
    detail: str


class ParsedArgs(argparse.Namespace):
    """Typed namespace for parsed CLI arguments."""

    gate: list[str] | None
    max_lines: int
    t5_max_lines: int
    json: bool
    report_only: bool
    self_test: bool

    def __init__(self) -> None:
        """Initialize with default values."""
        super().__init__()
        self.gate = None
        self.max_lines = DEFAULT_MAX_LINES
        self.t5_max_lines = DEFAULT_T5_LINE_BUDGET
        self.json = False
        self.report_only = False
        self.self_test = False


def repo_root() -> Path:
    """Return the repository root path."""
    return Path(__file__).resolve().parents[1]


def rel(path: Path, root: Path) -> str:
    """Return the POSIX-relative path of *path* from *root*."""
    return path.resolve().relative_to(root.resolve()).as_posix()


def rust_files(root: Path) -> list[Path]:
    """Return sorted Rust source files under the workspace."""
    return sorted((root / "crates").glob("**/*.rs"))


def is_t5_path(relative: str) -> bool:
    """Return True if the path belongs to the T5 signoff lane."""
    return any(part in relative for part in T5_PATH_PARTS)


def is_source_test_module_path(path: Path, root: Path) -> bool:
    """Return True if the path is a src/tests.rs module."""
    relative = rel(path, root)
    parts = relative.split("/")
    if "src" not in parts:
        return False
    src_index = parts.index("src")
    src_parts = parts[src_index + 1 :]
    return path.name == "tests.rs" or "tests" in src_parts[:-1]


def is_test_code(path: Path, root: Path) -> bool:
    """Return True if the file contains deterministic test code."""
    relative = rel(path, root)
    if is_t5_path(relative):
        return False
    if is_source_test_module_path(path, root):
        return True
    if "/tests/" in f"/{relative}":
        return True
    if "/src/" in f"/{relative}":
        try:
            text = path.read_text(errors="ignore")
        except OSError:
            return False
        return "#[cfg(test)]" in text or "mod tests" in text
    return False


def test_code_lines(  # noqa: C901 - cfg(test) brace tracking requires branches
    path: Path, root: Path
) -> list[tuple[int, str]]:
    """Return lines that belong to deterministic test code.

    Integration-test files under ``tests/`` are entirely test code. Files
    under ``src/`` can contain both product code and ``#[cfg(test)]``
    modules; only the latter are part of the static test-suite gates so
    production waits, subprocess launchers, or TCP listeners do not create
    false positives.
    """
    relative = rel(path, root)
    lines = path.read_text(errors="ignore").splitlines()
    if "/tests/" in f"/{relative}":
        return list(enumerate(lines, start=1))
    if is_source_test_module_path(path, root):
        return list(enumerate(lines, start=1))
    if "/src/" not in f"/{relative}":
        return []

    selected: list[tuple[int, str]] = []
    pending_cfg_test = False
    in_cfg_test_item = False
    brace_depth = 0

    for line_number, line in enumerate(lines, start=1):
        stripped = line.strip()
        if "#[cfg(test)]" in stripped:
            pending_cfg_test = True
            continue

        if pending_cfg_test and (
            not stripped
            or stripped.startswith(("#[", "//"))
        ):
            continue

        if pending_cfg_test:
            in_cfg_test_item = True
            pending_cfg_test = False
            brace_depth = line.count("{") - line.count("}")
            selected.append((line_number, line))
            if brace_depth <= 0 and ";" in line:
                in_cfg_test_item = False
            continue

        if in_cfg_test_item:
            selected.append((line_number, line))
            brace_depth += line.count("{") - line.count("}")
            if brace_depth <= 0:
                in_cfg_test_item = False

    return selected


def scan_pattern_gate(root: Path, gate: str) -> list[Violation]:
    """Scan test code for pattern-based gate violations."""
    violations: list[Violation] = []
    for path in rust_files(root):
        if not is_test_code(path, root):
            continue
        relative = rel(path, root)
        if relative in PROCESS_GLOBAL_STATE_EXEMPTIONS:
            continue
        for line_number, line in test_code_lines(path, root):
            for pattern in PATTERNS[gate]:
                if pattern.search(line):
                    violations.append(
                        Violation(
                            gate, relative, line_number,
                            pattern.pattern,
                        )
                    )
                    break
    return violations


def contains_test_function(path: Path) -> bool:
    """Return True if the file contains a test function."""
    text = path.read_text(errors="ignore")
    return "#[test]" in text or "#[tokio::test" in text


def scan_file_focus(root: Path, max_lines: int) -> list[Violation]:
    """Scan for test files exceeding the line budget."""
    violations: list[Violation] = []
    for path in sorted((root / "crates").glob("**/tests/**/*.rs")):
        relative = rel(path, root)
        if relative in PROCESS_GLOBAL_STATE_EXEMPTIONS:
            continue
        if is_t5_path(relative):
            continue
        helper_path = (
            "/support/" in f"/{relative}"
            or "/common/" in f"/{relative}"
        )
        covered_helper = (
            "/support/" in f"/{relative}"
            or path.name.endswith("_impl.rs")
        )
        if helper_path and not covered_helper and not contains_test_function(path):
            continue
        text = path.read_text(errors="ignore")
        line_count = len(text.splitlines())
        if line_count > max_lines:
            violations.append(
                Violation(
                    "file-focus", relative, 0,
                    f"{line_count} lines > {max_lines}",
                )
            )
    return violations


def iter_test_function_bodies(  # noqa: C901 - body extraction requires branches
    root: Path,
) -> Iterable[tuple[str, int, str, str]]:
    """Yield (relative, line, name, body) for each test function."""
    for path in rust_files(root):
        relative = rel(path, root)
        if is_t5_path(relative) or "_perf" in path.stem:
            continue
        if not is_test_code(path, root):
            continue

        lines = path.read_text(errors="ignore").splitlines()
        for index, line in enumerate(lines):
            if not TEST_ATTRIBUTE.match(line):
                continue
            cursor = index + 1
            while cursor < len(lines):
                candidate = lines[cursor].strip()
                if candidate and not candidate.startswith("#"):
                    break
                cursor += 1
            if cursor >= len(lines):
                continue
            match = TEST_FUNCTION.search(lines[cursor])
            if not match:
                continue
            brace_depth = 0
            body: list[str] = []
            for body_index in range(cursor, len(lines)):
                body.append(lines[body_index])
                brace_depth += (
                    lines[body_index].count("{")
                    - lines[body_index].count("}")
                )
                if brace_depth <= 0 and "{" in "\n".join(body):
                    break
            yield relative, cursor + 1, match.group(1), "\n".join(body)


def iter_test_function_blocks(  # noqa: C901 - block extraction requires branches
    root: Path,
) -> Iterable[tuple[str, int, str, str]]:
    """Yield (relative, line, name, block) for each test function."""
    for path in rust_files(root):
        relative = rel(path, root)
        if is_t5_path(relative) or "_perf" in path.stem:
            continue
        if not is_test_code(path, root):
            continue

        lines = path.read_text(errors="ignore").splitlines()
        for index, line in enumerate(lines):
            if not TEST_ATTRIBUTE.match(line):
                continue
            cursor = index + 1
            while cursor < len(lines):
                candidate = lines[cursor].strip()
                if candidate and not candidate.startswith("#"):
                    break
                cursor += 1
            if cursor >= len(lines):
                continue
            match = TEST_FUNCTION.search(lines[cursor])
            if not match:
                continue
            brace_depth = 0
            block: list[str] = lines[index:cursor]
            for body_index in range(cursor, len(lines)):
                block.append(lines[body_index])
                brace_depth += (
                    lines[body_index].count("{")
                    - lines[body_index].count("}")
                )
                if brace_depth <= 0 and "{" in "\n".join(block):
                    break
            yield relative, index + 1, match.group(1), "\n".join(block)


def conventions_key(relative: str, name: str) -> str:
    """Return the conventions baseline key for a test function."""
    return f"{relative}::{name}"


def conventions_baseline_entry(relative: str, name: str) -> str:
    """Return the SHA-256 hash of the conventions key."""
    return hashlib.sha256(
        conventions_key(relative, name).encode("utf-8")
    ).hexdigest()


def load_conventions_baseline(root: Path) -> set[str]:
    """Load the conventions baseline from the baseline JSON file."""
    path = root / CONVENTIONS_BASELINE_PATH
    if not path.exists():
        return set()
    try:
        data: object = json.loads(path.read_text(errors="ignore"))
    except json.JSONDecodeError as exc:
        return {
            f"<invalid-baseline-json>:{exc.lineno}:{exc.colno}"
        }
    if not isinstance(data, list):
        return {"<invalid-baseline-shape>"}
    entries: set[str] = set()
    for entry in data:
        if isinstance(entry, str):
            entries.add(entry)
        else:
            entries.add("<invalid-baseline-entry>")
    return entries


def scan_conventions(root: Path) -> list[Violation]:
    """Scan for convention debt (missing arrange/act/assert sections)."""
    violations: list[Violation] = []
    baseline = load_conventions_baseline(root)
    current_debt: set[str] = set()
    invalid_baseline = sorted(
        entry for entry in baseline
        if entry.startswith("<invalid-baseline")
    )
    violations.extend(
        Violation(
            "conventions",
            rel(root / CONVENTIONS_BASELINE_PATH, root),
            0,
            f"invalid conventions baseline entry: {entry}",
        )
        for entry in invalid_baseline
    )
    for relative, line_number, _name, body in iter_test_function_bodies(root):
        missing = [
            marker for marker in REQUIRED_SECTION_MARKERS
            if marker not in body
        ]
        if missing:
            key = conventions_baseline_entry(relative, _name)
            current_debt.add(key)
            if key in baseline:
                continue
            violations.append(
                Violation(
                    "conventions",
                    relative,
                    line_number,
                    "new convention debt: test body must include "
                    "// arrange, // act, and // assert sections "
                    "or be recorded in "
                    "docs/testing/test-suite-conventions-baseline.json",
                )
            )
    for stale in sorted(baseline - current_debt):
        if stale.startswith("<invalid-baseline"):
            continue
        violations.append(
            Violation(
                "conventions",
                rel(root / CONVENTIONS_BASELINE_PATH, root),
                0,
                f"stale conventions baseline entry no longer "
                f"matches current debt: {stale}",
            )
        )
    return violations


def scan_live_provider_env(root: Path) -> list[Violation]:
    """Scan for tests requiring live provider env vars."""
    violations: list[Violation] = []
    for relative, line_number, _name, block in iter_test_function_blocks(root):
        if "#[ignore" in block:
            continue
        if LIVE_PROVIDER_ENV.search(block):
            violations.append(
                Violation(
                    "live-provider-env",
                    relative,
                    line_number,
                    "deterministic tests must not require "
                    "HARNESS_LIVE_PROXY live provider env",
                )
            )
    return violations


def scan_t5_line_budget(root: Path, max_lines: int) -> list[Violation]:
    """Scan for T5 test directory exceeding the line budget."""
    t5_root = root / "crates" / "harness-testkit" / "tests"
    if not t5_root.exists():
        return []
    total = 0
    for path in sorted(t5_root.glob("**/*.rs")):
        if is_t5_path(rel(path, root)):
            total += len(path.read_text(errors="ignore").splitlines())
    if total <= max_lines:
        return []
    return [
        Violation(
            "t5-line-budget",
            rel(t5_root, root),
            0,
            f"{total} T5 signoff Rust lines > {max_lines}",
        )
    ]


def cassette_roots(root: Path) -> list[Path]:
    """Return cassette fixture directories."""
    return [
        path for path in (root / "crates").glob("**/fixtures/cassettes")
        if path.is_dir()
    ]


def scan_cassette_secrets(root: Path) -> list[Violation]:
    """Scan cassette fixtures for hardcoded secrets."""
    violations: list[Violation] = []
    for cassette_root in cassette_roots(root):
        for path in sorted(cassette_root.glob("**/*")):
            if not path.is_file():
                continue
            relative = rel(path, root)
            text = path.read_text(errors="ignore")
            for line_number, line in enumerate(
                text.splitlines(), start=1
            ):
                for name, pattern in SECRET_PATTERNS.items():
                    if pattern.search(line):
                        violations.append(
                            Violation(
                                "cassette-secrets",
                                relative,
                                line_number,
                                name,
                            )
                        )
    return violations


def snapshot_source(path: Path) -> str | None:
    """Extract the source path from an insta snapshot file."""
    lines = path.read_text(errors="ignore").splitlines()
    if not lines or lines[0].strip() != "---":
        return None
    for line in lines[1:]:
        stripped = line.strip()
        if stripped == "---":
            return None
        if stripped.startswith("source:"):
            value = stripped.split(":", 1)[1].strip()
            return value.strip("\"'") or None
    return None


def snapshot_crate_root(path: Path, root: Path) -> Path | None:
    """Return the crate root for a snapshot file."""
    try:
        relative = path.resolve().relative_to(
            (root / "crates").resolve()
        )
    except ValueError:
        return None
    parts = relative.parts
    if not parts:
        return None
    return root / "crates" / parts[0]


def crate_rust_text(path: Path, root: Path) -> str:
    """Return concatenated Rust source text for the snapshot's crate."""
    crate_root = snapshot_crate_root(path, root)
    if crate_root is None:
        return ""
    return "\n".join(
        rust_path.read_text(errors="ignore")
        for rust_path in sorted(crate_root.glob("**/*.rs"))
    )


def scan_orphan_snapshots(root: Path) -> list[Violation]:
    """Scan for orphaned insta snapshots."""
    violations: list[Violation] = []
    rust_text_cache: dict[Path, str] = {}
    for path in sorted((root / "crates").glob("**/snapshots/*.snap")):
        relative = rel(path, root)
        source = snapshot_source(path)
        if source is None:
            continue
        source_path = Path(source)
        if not source_path.is_absolute():
            source_path = root / source_path
        crate_root = snapshot_crate_root(path, root)
        crate_text = rust_text_cache.setdefault(
            crate_root or root,
            crate_rust_text(path, root),
        )
        if path.stem in crate_text:
            continue
        if not source_path.exists():
            violations.append(
                Violation(
                    "orphan-snapshots",
                    relative,
                    0,
                    f"snapshot source does not exist: {source}",
                )
            )
            continue
        source_text = source_path.read_text(errors="ignore")
        if not INSTA_ASSERTION.search(source_text):
            violations.append(
                Violation(
                    "orphan-snapshots",
                    relative,
                    0,
                    f"snapshot source has no insta snapshot "
                    f"assertion and snapshot name is unreferenced "
                    f"in crate Rust code: {source}",
                )
            )
    return violations


def scan_taxonomy(root: Path) -> list[Violation]:
    """Scan for test files not following taxonomy naming conventions."""
    violations: list[Violation] = []
    for path in sorted((root / "crates").glob("**/tests/**/*.rs")):
        relative = rel(path, root)
        helper_path = (
            "/support/" in f"/{relative}"
            or "/common/" in f"/{relative}"
        )
        if is_t5_path(relative) or (
            helper_path and not contains_test_function(path)
        ):
            continue
        name = path.name
        if not name.endswith(TAXONOMY_SUFFIXES):
            violations.append(
                Violation(
                    "taxonomy",
                    relative,
                    0,
                    "test file name must use a tier/intent suffix "
                    "such as _test, _regression, _recorded, "
                    "_perf, or _smoke",
                )
            )
        stem = path.stem
        if "regression" in stem and not stem.endswith("_regression"):
            violations.append(
                Violation(
                    "taxonomy",
                    relative,
                    0,
                    "regression test files must end with _regression",
                )
            )
        if "repro" in stem and not stem.endswith("_repro"):
            violations.append(
                Violation(
                    "taxonomy",
                    relative,
                    0,
                    "repro test files must end with _repro",
                )
            )
    return violations


def iter_test_functions(
    root: Path,
) -> Iterable[tuple[str, int, str]]:
    """Yield (relative, line, name) for each test function."""
    attribute = re.compile(r"\s*#\[(?:tokio::)?test(?:\([^]]*\))?\]")
    function = re.compile(r"(?:async\s+)?fn\s+([A-Za-z0-9_]+)\s*\(")
    for path in rust_files(root):
        relative = rel(path, root)
        lines = path.read_text(errors="ignore").splitlines()
        for index, line in enumerate(lines):
            if not attribute.match(line):
                continue
            cursor = index + 1
            while cursor < len(lines):
                candidate = lines[cursor].strip()
                if candidate and not candidate.startswith("#"):
                    break
                cursor += 1
            if cursor >= len(lines):
                continue
            match = function.search(lines[cursor])
            if match:
                yield relative, cursor + 1, match.group(1)


def scan_test_names(root: Path) -> list[Violation]:
    """Scan for test function names that are too short."""
    violations: list[Violation] = []
    for relative, line_number, name in iter_test_functions(root):
        if is_t5_path(relative):
            continue
        word_count = len([part for part in name.split("_") if part])
        if word_count < MIN_TEST_NAME_WORDS:
            violations.append(
                Violation(
                    "test-names",
                    relative,
                    line_number,
                    "test function name must be a descriptive "
                    "snake_case sentence with at least four words",
                )
            )
        if "regression" in name and not name.endswith("_regression"):
            violations.append(
                Violation(
                    "test-names",
                    relative,
                    line_number,
                    "regression test functions must end with "
                    "_regression",
                )
            )
        if "repro" in name and not name.endswith("_repro"):
            violations.append(
                Violation(
                    "test-names",
                    relative,
                    line_number,
                    "repro test functions must end with _repro",
                )
            )
    return violations


def scan_path_isolation(root: Path) -> list[Violation]:
    """Scan for tests using literal host paths instead of temp dirs."""
    violations: list[Violation] = []
    for path in rust_files(root):
        if not is_test_code(path, root):
            continue
        relative = rel(path, root)
        for line_number, line in test_code_lines(path, root):
            if (
                FS_LITERAL_ACCESS.search(line)
                and HOST_PATH_LITERAL.search(line)
            ):
                violations.append(
                    Violation(
                        "path-isolation",
                        relative,
                        line_number,
                        "test filesystem access must use "
                        "TestWorkspace/temp dirs or committed "
                        "fixtures, not literal host paths",
                    )
                )
    return violations


def run_gates(  # noqa: C901 - gate dispatch requires one branch per gate
    root: Path,
    gates: Iterable[str],
    max_lines: int,
    t5_max_lines: int,
) -> list[Violation]:
    """Run the requested gates and return all violations."""
    violations: list[Violation] = []
    for gate in gates:
        match gate:
            case gate_name if gate_name in PATTERNS:
                violations.extend(scan_pattern_gate(root, gate_name))
            case "live-provider-env":
                violations.extend(scan_live_provider_env(root))
            case "file-focus":
                violations.extend(scan_file_focus(root, max_lines))
            case "cassette-secrets":
                violations.extend(scan_cassette_secrets(root))
            case "orphan-snapshots":
                violations.extend(scan_orphan_snapshots(root))
            case "taxonomy":
                violations.extend(scan_taxonomy(root))
            case "test-names":
                violations.extend(scan_test_names(root))
            case "path-isolation":
                violations.extend(scan_path_isolation(root))
            case "t5-line-budget":
                violations.extend(
                    scan_t5_line_budget(root, t5_max_lines)
                )
            case "conventions":
                violations.extend(scan_conventions(root))
            case _:
                msg = f"unknown gate: {gate}"
                raise ValueError(msg)
    return violations


def print_text(
    violations: list[Violation], report_only: bool
) -> None:
    """Print violations in text format."""
    if not violations:
        print("test-suite gates: PASS")
        return
    status = "REPORT" if report_only else "FAIL"
    print(f"test-suite gates: {status} ({len(violations)} violation(s))")
    for violation in violations[:MAX_DISPLAYED_VIOLATIONS]:
        location = (
            violation.path
            if violation.line == 0
            else f"{violation.path}:{violation.line}"
        )
        print(f"- [{violation.gate}] {location} {violation.detail}")
    if len(violations) > MAX_DISPLAYED_VIOLATIONS:
        omitted = len(violations) - MAX_DISPLAYED_VIOLATIONS
        print(f"... {omitted} more violation(s) omitted")


def self_test() -> int:
    """Run the script's built-in unit self-test."""
    with tempfile.TemporaryDirectory(
        prefix="harness-gate-self-test-"
    ) as tmp:
        root = Path(tmp)
        test_dir = root / "crates" / "demo" / "tests"
        cassette_dir = (
            root / "crates" / "demo" / "tests" / "fixtures" / "cassettes"
        )
        test_dir.mkdir(parents=True)
        cassette_dir.mkdir(parents=True)
        _ = (test_dir / "slow.rs").write_text(
            "#[test]\nfn slow() { "
            "std::thread::sleep(std::time::Duration::from_millis(1)); "
            "}\n"
        )
        _ = (test_dir / "global_state.rs").write_text(
            '#[test]\nfn global() { std::env::set_var("A", "B"); }\n'
        )
        _ = (test_dir / "subprocess.rs").write_text(
            'use std::process::Command;\n'
            '#[test]\nfn p() { let _ = Command::new("echo"); }\n'
        )
        _ = (test_dir / "live_env.rs").write_text(
            '#[test]\nfn live_env() { '
            'let _ = std::env::var("HARNESS_LIVE_PROXY"); }\n'
        )
        _ = (test_dir / "aliases_test.rs").write_text(
            "use std::process::Command as ProcessCommand;\n"
            "use std::net::TcpListener as LocalTcpListener;\n"
            "#[test]\n"
            "fn aliases() {\n"
            '    let _ = EnvGuard::set(&[("A", Some("B"))]);\n'
            '    let _ = ProcessCommand::new("echo");\n'
            '    let _ = LocalTcpListener::bind("127.0.0.1:0");\n'
            "    let _ = MockServer::start();\n"
            "}\n"
        )
        _ = (test_dir / "path_leak_test.rs").write_text(
            'use std::fs;\n'
            "#[test]\n"
            'fn writes_literal_tmp_path() { '
            'fs::write("/tmp/leak", "bad").unwrap(); }\n'
        )
        _ = (test_dir / "fixed_bug_test.rs").write_text(
            "#[test]\nfn fixed_bug_regression_name_is_wrong() {}\n"
        )
        t5_dir = root / "crates" / "harness-testkit" / "tests"
        t5_dir.mkdir(parents=True)
        _ = (t5_dir / "pty_e2e.rs").write_text(
            "#[test]\nfn t5_smoke_runs() {}\n"
        )
        _ = (t5_dir / "deterministic_test.rs").write_text(
            "#[test]\nfn deterministic_test_runs() {}\n" + "// filler\n" * 3
        )

        source_test_dir = root / "crates" / "demo" / "src"
        source_test_dir.mkdir(parents=True)
        _ = (source_test_dir / "tests.rs").write_text(
            "#[tokio::test]\n"
            "async fn source_test_sleep() { "
            "tokio::time::sleep("
            "std::time::Duration::from_millis(1)).await; }\n"
        )
        _ = (test_dir / "oversized_test.rs").write_text(
            "#[test]\nfn t() {}\n" + "// filler\n" * 605
        )
        _ = (cassette_dir / "bad.json").write_text(
            '{"Authorization":"Bearer sk-secret000000000"}\n'
        )
        snapshot_dir = test_dir / "snapshots"
        snapshot_dir.mkdir()
        _ = (snapshot_dir / "slow__old.snap").write_text(
            "---\nsource: crates/demo/tests/slow.rs\n"
            "expression: old\n---\nold\n"
        )
        if scan_t5_line_budget(root, 2):
            print(
                "self-test counted non-T5 harness-testkit tests in T5 budget",
                file=sys.stderr,
            )
            return 1
        violations = run_gates(root, GATES, 600, 1)
        gates = {violation.gate for violation in violations}
        expected = {
            "no-sleeps", "no-global-state", "no-real-world-deps",
            "live-provider-env", "file-focus", "cassette-secrets",
            "orphan-snapshots", "taxonomy", "test-names",
            "path-isolation", "t5-line-budget", "conventions",
        }
        missing = expected - gates
        if missing:
            print(
                f"self-test missing gates: {sorted(missing)}",
                file=sys.stderr,
            )
            return 1
    print("self-test: PASS")
    return 0


def main(argv: list[str]) -> int:
    """Entry point: run test-suite gates."""
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument(
        "--gate", action="append", choices=GATES,
        help="Gate to run; repeatable. Defaults to all gates.",
    )
    _ = parser.add_argument(
        "--max-lines", type=int, default=DEFAULT_MAX_LINES,
        help="Maximum lines allowed for focused test files.",
    )
    _ = parser.add_argument(
        "--t5-max-lines", type=int,
        default=DEFAULT_T5_LINE_BUDGET,
        help="Maximum total Rust lines under harness-testkit/tests.",
    )
    _ = parser.add_argument(
        "--json", action="store_true",
        help="Emit JSON instead of text.",
    )
    _ = parser.add_argument(
        "--report-only", action="store_true",
        help="Print violations but exit zero during migration.",
    )
    _ = parser.add_argument(
        "--self-test", action="store_true",
        help="Run the script's built-in unit self-test.",
    )
    args = parser.parse_args(argv, namespace=ParsedArgs())

    if args.self_test:
        return self_test()

    root = repo_root()
    gates = args.gate or list(GATES)
    violations = run_gates(
        root, gates, args.max_lines, args.t5_max_lines
    )
    if args.json:
        print(
            json.dumps(
                {
                    "ok": not violations,
                    "violations": [asdict(v) for v in violations],
                },
                indent=2,
            )
        )
    else:
        print_text(violations, args.report_only)
    return 0 if args.report_only or not violations else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

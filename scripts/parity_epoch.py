#!/usr/bin/env python3
"""Compute a deterministic product epoch from canonical source inputs.

The product epoch is sha256(canonical JSON manifest) where the manifest
lists every product-affecting file with its content hash.  Two identical
input sets always yield one epoch; one source-byte mutation changes it;
excluded files never affect it.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from typing import Final

sys.dont_write_bytecode = True

EXCLUDED_PREFIXES: Final[tuple[str, ...]] = (
    ".git/",
    "target/",
    "sessions/",
    ".harness/",
    ".gnhf/",
    ".sisyphus/",
    ".omx/",
    ".omo/",
    ".codex/",
    "artifacts/",
    "inspirations/",
    "__pycache__/",
    ".mypy_cache/",
    ".ruff_cache/",
    ".codegraph/",
    # Scope document records the product epoch; including it in the source
    # manifest would create a self-referential hash that never converges.
    "docs/grok-cleanroom-scope.v1.json",
)

INCLUDED_EXTENSIONS: Final[frozenset[str]] = frozenset({
    ".rs", ".toml", ".lock", ".json", ".jsonc", ".md", ".py", ".sh",
    ".yaml", ".yml", ".txt", ".cfg", ".ini",
})

EXTRA_INCLUDE_PATHS: Final[tuple[str, ...]] = (
    "rust-toolchain.toml",
    "rust-toolchain",
    "Cargo.toml",
    "Cargo.lock",
)


def repo_root() -> Path:
    result = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        capture_output=True,
        text=True,
        check=True,
    )
    return Path(result.stdout.strip())


def is_excluded(rel_path: str) -> bool:
    return any(rel_path.startswith(prefix) for prefix in EXCLUDED_PREFIXES)


def is_product_affecting(rel_path: str) -> bool:
    if is_excluded(rel_path):
        return False
    basename = os.path.basename(rel_path)
    if basename in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rust-toolchain"):
        return True
    _, ext = os.path.splitext(rel_path)
    return ext in INCLUDED_EXTENSIONS


def tracked_files(root: Path) -> list[str]:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard"],
        capture_output=True,
        text=True,
        check=True,
        cwd=root,
    )
    return [line for line in result.stdout.splitlines() if line]


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as fh:
        while chunk := fh.read(1 << 16):
            digest.update(chunk)
    return digest.hexdigest()


def build_manifest(root: Path) -> dict[str, list[dict[str, str]]]:
    entries: list[dict[str, str]] = []
    for rel_path in sorted(tracked_files(root)):
        if not is_product_affecting(rel_path):
            continue
        abs_path = root / rel_path
        if not abs_path.is_file():
            continue
        entries.append({
            "path": rel_path,
            "sha256": file_sha256(abs_path),
        })
    return {"files": entries}


def canonical_json(manifest: dict[str, list[dict[str, str]]]) -> str:
    return json.dumps(manifest, indent=2, sort_keys=True, ensure_ascii=True) + "\n"


def compute_epoch(manifest_json: str) -> str:
    return hashlib.sha256(manifest_json.encode()).hexdigest()


def self_test() -> None:
    with tempfile.TemporaryDirectory() as tmp_dir:
        root = Path(tmp_dir)
        src = root / "src"
        src.mkdir()
        (root / "Cargo.toml").write_text('[package]\nname = "test"\n')
        (src / "main.rs").write_text("fn main() {}\n")
        (root / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "stable"\n')

        session_dir = root / "sessions" / "run-1"
        session_dir.mkdir(parents=True)
        (session_dir / "events.jsonl").write_text('{"seq":1}\n')

        artifacts_dir = root / "artifacts" / "qa"
        artifacts_dir.mkdir(parents=True)
        (artifacts_dir / "log.txt").write_text("evidence log\n")

        subprocess.run(["git", "init"], cwd=root, capture_output=True, check=True)
        subprocess.run(["git", "add", "-A"], cwd=root, capture_output=True, check=True)

        m1_json = canonical_json(build_manifest(root))
        epoch_a = compute_epoch(m1_json)

        m2_json = canonical_json(build_manifest(root))
        epoch_b = compute_epoch(m2_json)

        assert epoch_a == epoch_b, (
            f"FAIL: identical inputs produced different epochs: {epoch_a} != {epoch_b}"
        )
        print(f"PASS: identical inputs yield one epoch ({epoch_a[:16]}...)")

        manifest = build_manifest(root)
        paths_in_manifest = {entry["path"] for entry in manifest["files"]}
        assert "sessions/run-1/events.jsonl" not in paths_in_manifest, (
            "FAIL: session file included in manifest"
        )
        assert "artifacts/qa/log.txt" not in paths_in_manifest, (
            "FAIL: artifact file included in manifest"
        )
        print("PASS: excluded files (sessions, artifacts) absent from manifest")

        (src / "main.rs").write_text("fn main() { println!(\"mutated\"); }\n")
        subprocess.run(["git", "add", "-A"], cwd=root, capture_output=True, check=True)

        m3_json = canonical_json(build_manifest(root))
        epoch_c = compute_epoch(m3_json)

        assert epoch_c != epoch_a, (
            f"FAIL: source mutation did not change epoch: {epoch_c} == {epoch_a}"
        )
        print(f"PASS: source-byte mutation changes epoch ({epoch_c[:16]}...)")

        (session_dir / "events.jsonl").write_text('{"seq":999,"mutated":true}\n')
        subprocess.run(["git", "add", "-A"], cwd=root, capture_output=True, check=True)

        m4_json = canonical_json(build_manifest(root))
        epoch_d = compute_epoch(m4_json)

        assert epoch_d == epoch_c, (
            f"FAIL: excluded file mutation changed epoch: {epoch_d} != {epoch_c}"
        )
        print("PASS: excluded file mutation does not affect epoch")

    print(f"\nself-test: 4/4 passed")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run deterministic self-test in a temporary git repo.",
    )
    parser.add_argument(
        "--manifest-only",
        action="store_true",
        help="Print the canonical JSON manifest without the epoch.",
    )
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return

    root = repo_root()
    manifest = build_manifest(root)
    manifest_json = canonical_json(manifest)

    if args.manifest_only:
        sys.stdout.write(manifest_json)
        return

    epoch = compute_epoch(manifest_json)
    result = {
        "product_epoch": epoch,
        "file_count": len(manifest["files"]),
        "schema": "parity-epoch-v1",
        "root": str(root),
    }
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()

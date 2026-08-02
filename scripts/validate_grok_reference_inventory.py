#!/usr/bin/env python3
"""Canonical frozen-reference interaction inventory validator.

Todo 8 integrates the Todo 2 frozen-reference behavior inventory into
``docs/grok-reference-interaction-inventory.v1.json``.  This validator is the
canonical gate.  It recomputes ``reference_epoch`` from the frozen reference
inputs with the exact Todo 1 formula, proves the frozen binary is unchanged,
re-hashes every cited reference source file, and fails closed on inventory
drift, duplicate or omitted rows, grouped catch-all rows, absent source
symbols, forbidden status claims, and exclusion leakage.

Usage:
    python3 scripts/validate_grok_reference_inventory.py \
        --inventory docs/grok-reference-interaction-inventory.v1.json \
        --source-root "$REFERENCE_ROOT" \
        --reference-bin "$REFERENCE_BIN" \
        --reference-inputs <attemptDir>/task-1-grok-build-clean-room-parity/reference-inputs.json
    python3 scripts/validate_grok_reference_inventory.py --self-test
"""
# allow: SIZE_OK - single fail-closed validator owning inventory counts, row
# schema, catch-all/exclusion/status rules, frozen-source drift checks, and a
# hermetic self-test; splitting would shard one provenance contract.

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tempfile
from collections.abc import Callable
from pathlib import Path
from typing import Any, Final, NoReturn

VALID_CATEGORIES: Final = frozenset({
    "action", "action_def", "slash_module", "builtin_command", "setting",
    "active_modal", "terminal_brand", "focus_transition", "action_category",
    "view", "terminal_conditional", "scrollback_component",
    "runtime_feature_owner", "documented_journey", "pty_scenario_family",
    "observable_fallback",
})
COUNT_TARGET_ALIASES: Final = {
    "actions": "action",
    "action_defs": "action_def",
    "slash_modules": "slash_module",
    "builtin_commands": "builtin_command",
    "settings": "setting",
    "active_modals": "active_modal",
    "terminal_brands": "terminal_brand",
}
ALLOWED_DISPOSITIONS: Final = frozenset({
    "pending", "blocked", "incomplete", "retained", "approved_exclusion",
})
FORBIDDEN_DISPOSITIONS: Final = frozenset({"pass", "diverged"})
# Rows that cite an approved-exclusion source root must carry an exclusion
# disposition. ``approved_exclusion`` is the assigned scope verdict; legacy
# ``pending``/``blocked`` remain tolerated for pre-classification inventories.
EXCLUSION_DISPOSITIONS: Final = frozenset({"pending", "blocked", "approved_exclusion"})
EXCLUSION_ROOTS: Final = (
    "crates/codegen/xai-grok-voice/",
    "crates/codegen/xai-grok-telemetry/",
    "crates/codegen/xai-grok-workspace-client/",
    "crates/codegen/xai-grok-plugin-marketplace/",
)
CATCH_ALL_TOKENS: Final = frozenset({"*", "**", "all", "various", "misc", "etc", "any"})
WILDCARD_CHARS: Final = ("*", "?", "[")
VALID_PROOF_DIMENSIONS: Final = frozenset({f"P{n}" for n in range(10)})
REQUIRED_ROW_FIELDS: Final = (
    "category", "source_path", "source_symbol", "line", "trigger", "focus_owner",
    "state_transition", "rendered_effect", "side_effect", "persistence",
    "viewport_capability_conditions", "approved_disposition", "p0_p9_applicability",
)
# Registry categories are scraped from frozen source: their symbols must appear
# literally (or normalized, or as the module stem) in the cited file.  Doc-derived
# categories carry synthesized anchors that cite the owning module by path only.
SOURCE_ANCHORED_CATEGORIES: Final = frozenset({
    "action", "action_def", "slash_module", "builtin_command", "setting",
    "active_modal", "terminal_brand", "focus_transition", "action_category",
})
CHUNK_SIZE: Final = 1 << 16
DESCRIPTION: Final = "Canonical frozen-reference interaction inventory validator (clean-room parity)"


def _fail(message: str) -> NoReturn:
    raise ValueError(message)


def canonical_json_bytes(value: Any) -> bytes:
    """Canonical JSON exactly matching the Todo 1 reference_epoch input."""
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def recompute_reference_epoch(reference_inputs: dict[str, Any]) -> str:
    """Recompute reference_epoch with the exact Todo 1 formula.

    ``sha256(canonical_json(reference HEAD, tree id, clean status, binary
    sha/version, and every cited source file entry))``.
    """
    root = reference_inputs["reference_root"]
    binary = reference_inputs["reference_bin"]
    payload = {
        "reference_head": root["head"],
        "reference_tree_id": root["tree_id"],
        "reference_clean_status": root["clean_status"],
        "reference_binary": {"sha256": binary["sha256"], "version": binary["version"]},
        "source_files": reference_inputs["source_files"],
    }
    return hashlib.sha256(canonical_json_bytes(payload)).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(CHUNK_SIZE):
            digest.update(chunk)
    return digest.hexdigest()


def load_json_object(path: Path, label: str) -> dict[str, Any]:
    if not path.is_file():
        _fail(f"{label} must be an existing JSON file: {path}")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        _fail(f"{label} root must be a JSON object: {path}")
    return value


def _has_wildcard(value: str) -> bool:
    return any(character in value for character in WILDCARD_CHARS)


def _content_reader(source_root: Path) -> Callable[[str], tuple[str, str, str]]:
    """Return (content, normalized content, file stem) per cited path, cached."""
    cache: dict[str, tuple[str, str, str]] = {}

    def read(rel_path: str) -> tuple[str, str, str]:
        if rel_path not in cache:
            content = (source_root / rel_path).read_text(encoding="utf-8", errors="replace")
            normalized = "".join(ch.lower() for ch in content if ch.isalnum())
            cache[rel_path] = (content, normalized, Path(rel_path).stem)
        return cache[rel_path]

    return read


def _symbol_present(symbol: str, content: str, normalized: str, stem: str) -> bool:
    last = symbol.rsplit("::", 1)[-1].strip()
    return last in content or last == stem or "".join(ch.lower() for ch in last if ch.isalnum()) in normalized


def validate_counts(inventory: dict[str, Any], rows: list[dict[str, Any]], defects: list[str]) -> None:
    """Fail closed when declared counts disagree with the rows or targets."""
    metadata = inventory.get("metadata")
    if not isinstance(metadata, dict):
        defects.append("inventory metadata must be an object")
        return
    actual = metadata.get("actual_counts")
    targets = metadata.get("count_targets")
    if not isinstance(actual, dict) or not isinstance(targets, dict):
        defects.append("inventory metadata.actual_counts and count_targets must be objects")
        return
    computed: dict[str, int] = {}
    for row in rows:
        category = row.get("category")
        if isinstance(category, str):
            computed[category] = computed.get(category, 0) + 1
    for category, count in actual.items():
        if category not in VALID_CATEGORIES:
            defects.append(f"actual_counts category is not a known inventory category: {category}")
        if computed.get(category, 0) != count:
            defects.append(f"inventory count drift: actual_counts.{category}={count} rows={computed.get(category, 0)}")
    for category in computed:
        if category not in actual:
            defects.append(f"inventory rows use a category absent from actual_counts: {category}")
    for plural, target in targets.items():
        singular = COUNT_TARGET_ALIASES.get(plural)
        if singular is None:
            defects.append(f"count_targets key is not a known registry target: {plural}")
            continue
        if actual.get(singular) != target:
            defects.append(f"count target drift: count_targets.{plural}={target} actual_counts.{singular}={actual.get(singular)}")


def validate_rows(
    rows: list[dict[str, Any]],
    manifest_paths: frozenset[str],
    read_content: Callable[[str], tuple[str, str, str]],
    defects: list[str],
) -> None:
    """Validate every row schema, uniqueness, catch-all, status, and symbol rule."""
    seen: set[str] = set()
    for index, row in enumerate(rows):
        if not isinstance(row, dict):
            defects.append(f"row {index}: not a JSON object")
            continue
        missing = [field for field in REQUIRED_ROW_FIELDS if field not in row]
        if missing:
            defects.append(f"row {index}: missing required fields {missing}")
            continue
        category = str(row["category"])
        source_path = str(row["source_path"])
        symbol = str(row["source_symbol"])
        disposition = str(row["approved_disposition"])
        if category not in VALID_CATEGORIES:
            defects.append(f"row {index}: invalid category '{category}'")
        if _has_wildcard(source_path) or _has_wildcard(symbol) or " " in symbol.strip():
            defects.append(f"row {index}: grouped catch-all row (path={source_path} symbol={symbol})")
        last_segment = symbol.rsplit("::", 1)[-1].strip().lower()
        if last_segment in CATCH_ALL_TOKENS:
            defects.append(f"row {index}: grouped catch-all symbol '{symbol}'")
        key = f"{category}::{source_path}::{symbol}::{row['line']}"
        if key in seen:
            defects.append(f"row {index}: duplicate behavior id {key}")
        seen.add(key)
        if disposition in FORBIDDEN_DISPOSITIONS:
            defects.append(f"row {index}: forbidden status claim '{disposition}' (wave 0 rows stay incomplete/pending)")
        elif disposition not in ALLOWED_DISPOSITIONS:
            defects.append(f"row {index}: invalid approved_disposition '{disposition}'")
        if any(source_path.startswith(root) for root in EXCLUSION_ROOTS):
            if disposition not in EXCLUSION_DISPOSITIONS:
                defects.append(f"row {index}: exclusion leakage under approved-exclusion root with disposition '{disposition}'")
        dimensions = [part.strip() for part in str(row["p0_p9_applicability"]).split(",") if part.strip()]
        unknown = sorted(set(dimensions) - VALID_PROOF_DIMENSIONS)
        if not dimensions or unknown:
            defects.append(f"row {index}: invalid p0_p9_applicability '{row['p0_p9_applicability']}'")
        if source_path not in manifest_paths:
            defects.append(f"row {index}: source path not in frozen reference manifest: {source_path}")
            continue
        try:
            content, normalized, stem = read_content(source_path)
        except OSError:
            defects.append(f"row {index}: source file unreadable: {source_path}")
            continue
        if category in SOURCE_ANCHORED_CATEGORIES and not _symbol_present(symbol, content, normalized, stem):
            defects.append(f"row {index}: absent source symbol '{symbol}' in {source_path}")


def validate_reference_inputs(
    reference_inputs: dict[str, Any],
    inventory: dict[str, Any],
    source_root: Path,
    reference_bin: Path,
    defects: list[str],
) -> str:
    """Prove epoch, binary, root, and source-file freshness; return the epoch."""
    metadata = inventory.get("metadata")
    recorded = reference_inputs.get("reference_epoch")
    inventory_epoch = metadata.get("reference_epoch") if isinstance(metadata, dict) else None
    recomputed = recompute_reference_epoch(reference_inputs)
    if recomputed != recorded or recorded != inventory_epoch:
        defects.append(
            "reference_epoch drift: recomputed="
            f"{recomputed} reference_inputs={recorded} inventory={inventory_epoch}"
        )
    binary = reference_inputs["reference_bin"]
    if not reference_bin.is_file():
        defects.append(f"reference binary missing: {reference_bin}")
    elif sha256_file(reference_bin) != binary.get("sha256"):
        defects.append("reference binary drift: sha256 differs from frozen reference inputs")
    recorded_root = Path(str(reference_inputs["reference_root"]["path"]))
    if source_root.resolve() != recorded_root.resolve():
        defects.append(f"source root mismatch: {source_root} != recorded {recorded_root}")
    for entry in reference_inputs["source_files"]:
        rel = str(entry["path"])
        live = source_root / rel
        if not live.is_file():
            defects.append(f"reference source file missing: {rel}")
            continue
        if sha256_file(live) != entry.get("sha256"):
            defects.append(f"reference source drift: {rel}")
        elif live.stat().st_size != entry.get("size"):
            defects.append(f"reference source size drift: {rel}")
    return recomputed


def run_validation(args: argparse.Namespace) -> str:
    """Run every check fail-closed and return the recomputed reference epoch."""
    inventory = load_json_object(Path(args.inventory), "inventory")
    reference_inputs = load_json_object(Path(args.reference_inputs), "reference-inputs")
    rows = inventory.get("rows")
    defects: list[str] = []
    if not isinstance(rows, list) or not rows:
        _fail("inventory rows must be a non-empty list")
    manifest_paths = frozenset(str(entry["path"]) for entry in reference_inputs["source_files"])
    source_root = Path(args.source_root)
    validate_counts(inventory, rows, defects)
    validate_rows(rows, manifest_paths, _content_reader(source_root), defects)
    epoch = validate_reference_inputs(reference_inputs, inventory, source_root, Path(args.reference_bin), defects)
    if defects:
        _fail("inventory invalid: " + "; ".join(defects))
    return epoch


# ---------------------------------------------------------------------------
# Hermetic self-test.
# ---------------------------------------------------------------------------

def _write_json(path: Path, value: dict[str, Any]) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _row(category: str, source_path: str, symbol: str, line: int, proof: str = "P1", disposition: str = "pending") -> dict[str, Any]:
    return {
        "category": category, "source_path": source_path, "source_symbol": symbol, "line": line,
        "trigger": "key_event_or_command_palette", "focus_owner": "varies_by_when_context",
        "state_transition": f"dispatches_{symbol.rsplit('::', 1)[-1]}", "rendered_effect": "varies_by_handler",
        "side_effect": "none", "persistence": "none", "viewport_capability_conditions": "none",
        "approved_disposition": disposition, "p0_p9_applicability": proof,
        "notes": "self-test fixture row",
    }


def _source_entry(path: Path, rel: str) -> dict[str, Any]:
    stat = path.stat()
    return {"mode": f"{stat.st_mode & 0o777:03o}", "path": rel, "sha256": sha256_file(path), "size": stat.st_size, "type": "file"}


def _build_fixture(root: Path) -> tuple[Path, Path, Path, Path]:
    """Create a hermetic frozen reference tree, inputs manifest, and inventory."""
    source_root = root / "reference"
    actions_dir = source_root / "crates/codegen/xai-grok-pager/src/actions"
    slash_dir = source_root / "crates/codegen/xai-grok-pager/src/slash/commands"
    voice_dir = source_root / "crates/codegen/xai-grok-voice/src"
    for directory in (actions_dir, slash_dir, voice_dir):
        directory.mkdir(parents=True)
    actions = actions_dir / "mod.rs"
    slash = slash_dir / "mod.rs"
    voice = voice_dir / "mod.rs"
    actions.write_text("pub enum ActionId { SendPrompt }\n", encoding="utf-8")
    slash.write_text("pub fn slash_modules() { register(Help); }\n", encoding="utf-8")
    voice.write_text("pub enum VoiceId { EnableVoiceMode }\n", encoding="utf-8")
    bin_path = source_root / "target/debug/xai-grok-pager"
    bin_path.parent.mkdir(parents=True)
    bin_path.write_bytes(b"self-test frozen reference binary")
    actions_rel = "crates/codegen/xai-grok-pager/src/actions/mod.rs"
    slash_rel = "crates/codegen/xai-grok-pager/src/slash/commands/mod.rs"
    voice_rel = "crates/codegen/xai-grok-voice/src/mod.rs"
    reference_inputs: dict[str, Any] = {
        "reference_bin": {"path": str(bin_path), "sha256": sha256_file(bin_path), "version": "grok 0.0.0-self-test (t) [stable]"},
        "reference_root": {"clean_status": "clean", "head": "f" * 40, "tree_id": "a" * 40, "path": str(source_root)},
        "source_files": [
            _source_entry(actions, actions_rel),
            _source_entry(slash, slash_rel),
            _source_entry(voice, voice_rel),
        ],
    }
    reference_inputs["reference_epoch"] = recompute_reference_epoch(reference_inputs)
    inputs_path = root / "reference-inputs.json"
    _write_json(inputs_path, reference_inputs)
    rows = [
        _row("action", actions_rel, "ActionId::SendPrompt", 1, "P0"),
        _row("action", voice_rel, "VoiceId::EnableVoiceMode", 1),
        _row("slash_module", slash_rel, "SlashModule::Help", 1),
    ]
    inventory = {
        "metadata": {
            "reference_revision": "f" * 40,
            "reference_source_root": str(source_root),
            "reference_binary": str(bin_path),
            "generated_by": "validate_grok_reference_inventory.py self-test",
            "count_targets": {"actions": 2, "slash_modules": 1},
            "actual_counts": {"action": 2, "slash_module": 1},
            "approved_disposition_policy": "all_rows_pending;_no_pass_status_assigned",
            "reference_epoch": reference_inputs["reference_epoch"],
            "reference_inputs_path": str(inputs_path),
        },
        "rows": rows,
    }
    inventory_path = root / "inventory.json"
    _write_json(inventory_path, inventory)
    return inventory_path, inputs_path, source_root, bin_path


def _argv(inventory: Path, inputs: Path, source_root: Path, bin_path: Path) -> list[str]:
    return [
        "--inventory", str(inventory), "--source-root", str(source_root),
        "--reference-bin", str(bin_path), "--reference-inputs", str(inputs),
    ]


def _expect_pass(parser: argparse.ArgumentParser, argv: list[str], label: str) -> str:
    epoch = run_validation(parser.parse_args(argv))
    print(f"PASS: {label} (reference_epoch={epoch[:16]}...)")
    return epoch


def _expect_fail(parser: argparse.ArgumentParser, argv: list[str], label: str, needle: str) -> None:
    try:
        run_validation(parser.parse_args(argv))
    except ValueError as error:
        if needle not in str(error):
            _fail(f"self-test '{label}' rejected for the wrong reason: {error}")
        print(f"PASS: {label} rejected ({needle})")
        return
    _fail(f"self-test '{label}' unexpectedly accepted")


def self_test() -> int:
    """Exercise the happy path and every failure mutation hermetically."""
    parser = argparse.ArgumentParser(description="self-test argument shape")
    _ = parser.add_argument("--inventory")
    _ = parser.add_argument("--source-root")
    _ = parser.add_argument("--reference-bin")
    _ = parser.add_argument("--reference-inputs")
    with tempfile.TemporaryDirectory() as tmp_dir:
        root = Path(tmp_dir)
        inventory_path, inputs_path, source_root, bin_path = _build_fixture(root)
        base = _argv(inventory_path, inputs_path, source_root, bin_path)
        epoch = _expect_pass(parser, base, "fresh inventory validates")
        if epoch != json.loads(inputs_path.read_text(encoding="utf-8"))["reference_epoch"]:
            _fail("self-test epoch did not round-trip")

        def mutated(label: str, needle: str, change: Callable[[dict[str, Any]], None]) -> None:
            inventory = json.loads(inventory_path.read_text(encoding="utf-8"))
            change(inventory)
            mutated_path = root / f"inventory-{label}.json"
            _write_json(mutated_path, inventory)
            argv = base.copy()
            argv[argv.index("--inventory") + 1] = str(mutated_path)
            _expect_fail(parser, argv, label, needle)

        def duplicate_row(inventory: dict[str, Any]) -> None:
            inventory["rows"].append(dict(inventory["rows"][0]))

        def omit_row(inventory: dict[str, Any]) -> None:
            inventory["rows"] = [row for row in inventory["rows"] if "SendPrompt" not in str(row["source_symbol"])]

        def catch_all_symbol(inventory: dict[str, Any]) -> None:
            inventory["rows"][0]["source_symbol"] = "ActionId::*"

        def pass_claim(inventory: dict[str, Any]) -> None:
            inventory["rows"][0]["approved_disposition"] = "pass"

        def absent_symbol(inventory: dict[str, Any]) -> None:
            inventory["rows"][0]["source_symbol"] = "ActionId::Missing"

        def exclusion_leak(inventory: dict[str, Any]) -> None:
            voice_row = next(row for row in inventory["rows"] if "xai-grok-voice" in str(row["source_path"]))
            voice_row["approved_disposition"] = "incomplete"

        def target_drift(inventory: dict[str, Any]) -> None:
            inventory["metadata"]["count_targets"]["actions"] = 3

        def manifest_path(inventory: dict[str, Any]) -> None:
            inventory["rows"][0]["source_path"] = "crates/codegen/unknown.rs"

        mutated("duplicate-id", "duplicate behavior id", duplicate_row)
        mutated("omitted-row", "inventory count drift", omit_row)
        mutated("catch-all-symbol", "catch-all", catch_all_symbol)
        mutated("forbidden-pass-claim", "forbidden status claim", pass_claim)
        mutated("absent-source-symbol", "absent source symbol", absent_symbol)
        mutated("exclusion-leakage", "exclusion leakage", exclusion_leak)
        mutated("count-target-drift", "count target drift", target_drift)
        mutated("manifest-path-unknown", "not in frozen reference manifest", manifest_path)

        tampered_inputs = json.loads(inputs_path.read_text(encoding="utf-8"))
        original_sha = tampered_inputs["source_files"][0]["sha256"]
        tampered_inputs["source_files"][0]["sha256"] = "0" * 64
        tampered_inputs["reference_epoch"] = recompute_reference_epoch(tampered_inputs)
        tampered_path = root / "reference-inputs-tampered.json"
        _write_json(tampered_path, tampered_inputs)
        argv = base.copy()
        argv[argv.index("--reference-inputs") + 1] = str(tampered_path)
        _expect_fail(parser, argv, "epoch-drift", "reference_epoch drift")

        drifted_source = source_root / "crates/codegen/xai-grok-pager/src/actions/mod.rs"
        original_bytes = drifted_source.read_bytes()
        drifted_source.write_bytes(original_bytes + b"// drifted\n")
        _expect_fail(parser, base, "source-file-drift", "source drift")
        drifted_source.write_bytes(original_bytes)
        _expect_pass(parser, base, "restored source validates again")

        bin_path.write_bytes(bin_path.read_bytes() + b"+drift")
        _expect_fail(parser, base, "reference-binary-drift", "binary drift")
        bin_path.write_bytes(b"self-test frozen reference binary")

        empty_root = root / "empty-root"
        empty_root.mkdir()
        argv = base.copy()
        argv[argv.index("--source-root") + 1] = str(empty_root)
        _expect_fail(parser, argv, "source-root-mismatch", "source root mismatch")

    print("self-test: 14/14 passed")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=DESCRIPTION)
    _ = parser.add_argument("--inventory", type=Path)
    _ = parser.add_argument("--source-root", type=Path)
    _ = parser.add_argument("--reference-bin", type=Path)
    _ = parser.add_argument("--reference-inputs", type=Path)
    _ = parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    for flag in ("inventory", "source_root", "reference_bin", "reference_inputs"):
        value = getattr(args, flag)
        if not isinstance(value, Path) or not value.is_file() and not value.is_dir():
            _fail(f"--{flag.replace('_', '-')} must name an existing path (or use --self-test)")
    epoch = run_validation(args)
    print(f"inventory_valid=true reference_epoch={epoch}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"inventory_valid=false error={error}", file=sys.stderr)
        raise SystemExit(1) from error

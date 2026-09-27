from pathlib import Path
import hashlib, json, shutil, subprocess
out = Path(__file__).resolve().parent
base = "7918123b2e2946a8b87a94a9c03fe9b594a13b58"
paths = [Path(p) for p in ["crates/harness-tui/src/composer_atoms/buffer.rs", "crates/harness-tui/src/composer_atoms/grapheme.rs", "crates/harness-tui/src/composer_integration/presentation.rs", "crates/harness-tui/src/ui_markdown_table_tests.rs", "crates/harness-tui/src/ui_composer/presentation.rs", "crates/harness-tui/src/ui_composer/bordered.rs", "crates/harness-tui/src/ui_composer/collapsed.rs", "crates/harness-tui/src/ui_composer/document.rs"]]
current = {p: p.read_bytes() for p in paths}
(out / "source.patch").write_bytes(subprocess.check_output(["git", "diff", "--binary", base, "--", "crates/harness-tui/src"]))
(out / "cargo.json").write_bytes(subprocess.check_output(["cargo", "metadata", "--format-version", "1"]))
def build(side):
    with (out / f"build-{side}.log").open("w") as log:
        subprocess.run(["cargo", "build", "--release", "-p", "harness-tui", "--all-features", "--test", "rewrite_performance_test", "--example", "rewrite_probe"], stdout=log, stderr=log, check=True)
        metadata = json.loads(subprocess.check_output(["cargo", "nextest", "list", "--release", "-p", "harness-tui", "--all-features", "--test", "rewrite_performance_test", "--list-type", "binaries-only", "--message-format", "json"], stderr=log))
    binary = next(iter(metadata["rust-binaries"].values()))
    kept = out / f"{side}.bin"
    shutil.copy2(binary["binary-path"], kept)
    binary["binary-path"] = str(kept)
    shutil.copy2("target/release/examples/rewrite_probe", out / f"{side}-probe")
    folder = out / side
    folder.mkdir(exist_ok=True)
    (folder / "binaries.json").write_text(json.dumps(metadata, indent=2) + "\n")
    receipt = {"source": base + (" plus source.patch" if side == "candidate" else ""),
               "fixture_sha256": hashlib.sha256(Path("crates/harness-tui/tests/rewrite_performance_test.rs").read_bytes()).hexdigest(),
               "sha256": hashlib.sha256(kept.read_bytes()).hexdigest(),
               "production_sources": [{"path": str(p), "sha256": hashlib.sha256(p.read_bytes()).hexdigest()} for p in paths]}
    (folder / "binary.json").write_text(json.dumps(receipt, indent=2) + "\n")
try:
    for p in paths:
        p.write_bytes(subprocess.check_output(["git", "show", f"{base}:{p}"]))
    build("before")
finally:
    for p, data in current.items():
        p.write_bytes(data)
build("candidate")

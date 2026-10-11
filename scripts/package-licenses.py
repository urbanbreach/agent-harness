#!/usr/bin/env python3
"""Collect dependency and embedded-asset notices from the release build inputs."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

import ziglang

ROOT = Path(__file__).resolve().parent.parent


def main():
    destination = Path(sys.argv[1])
    version = subprocess.check_output(["cargo-about", "--version"], text=True).strip()
    if version != "cargo-about 0.9.2":
        raise SystemExit("release notices require cargo-about 0.9.2")
    with tempfile.TemporaryDirectory() as temporary:
        report = Path(temporary) / "licenses.json"
        subprocess.run([
            "cargo-about", "generate", "--locked", "--fail",
            "--manifest-path", str(ROOT / "crates/harness/Cargo.toml"),
            "--config", str(ROOT / "about.toml"), "--format", "json",
            "--output-file", str(report),
        ], cwd=ROOT, check=True)
        data = json.loads(report.read_text())
    if not data["licenses"] or not data["crates"]:
        raise SystemExit("dependency license inventory is empty")
    destination.mkdir(parents=True)
    with (destination / "dependencies.txt").open("w") as output:
        output.write("Dependency licenses and upstream distribution notices\n\n")
        for license in data["licenses"]:
            output.write(f"\n=== {license['name']} ===\n")
            for entry in license["used_by"]:
                package = entry["crate"]
                output.write(f"{package['name']} {package['version']} — {', '.join(package['authors'])}\n")
                if package.get("repository"):
                    output.write(package["repository"] + "\n")
            output.write("\n" + license["text"] + "\n")
        # Preserve original attribution, including bundled C libraries and NOTICE files.
        # Upstream distributions may include notices for components we do not link.
        for entry in data["crates"]:
            package = entry["package"]
            root = Path(package["manifest_path"]).parent
            for path in sorted(root.rglob("*")):
                if not path.is_file() or not path.name.upper().startswith(("LICENSE", "COPYING", "COPYRIGHT", "NOTICE")):
                    continue
                output.write(f"\n=== {package['name']} {package['version']}: {path.relative_to(root)} ===\n")
                output.write(path.read_text() + "\n")
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
    zig = Path(ziglang.__file__).parent
    notices = {
        "prompts.txt": ROOT / "crates/harness-core/prompts/LICENSE.upstream",
        "syntax.txt": ROOT / "crates/harness-tui/assets/syntax/LICENSE.upstream",
        "acorn.txt": ROOT / "crates/harness-eval/vendor/ACORN-LICENSE",
        "rust-stdlib.html": sysroot / "share/doc/rust/COPYRIGHT-library.html",
        "musl.txt": zig / "lib/libc/musl/COPYRIGHT",
        "zig.txt": zig / "LICENSE",
    }
    for name, source in notices.items():
        if not source.is_file() or source.stat().st_size == 0:
            raise SystemExit(f"missing release notice: {source}")
        shutil.copyfile(source, destination / name)


if __name__ == "__main__":
    main()

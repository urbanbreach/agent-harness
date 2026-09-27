from pathlib import Path
import json, subprocess
out = Path(__file__).resolve().parent
p = Path("crates/harness-tui/src/composer_atoms/buffer.rs")
original = p.read_bytes()
reference = subprocess.check_output(["git", "show", f"7918123b2e2946a8b87a94a9c03fe9b594a13b58:{p}"])
changed = reference.replace(b"> 1\n", b"> 2\n").replace(b"if character == '\\n'", b"if character == '\\0'")
assert changed != reference
command = ["cargo", "nextest", "run", "--profile", "ci", "-p", "harness-tui", "--all-features", "--test", "composer_atoms_test", "--no-fail-fast"]
try:
    p.write_bytes(changed)
    (out / "red.patch").write_bytes(subprocess.check_output(["git", "diff", "--", str(p)]))
    with (out / "red.log").open("w") as log:
        result = subprocess.run(command, stdout=log, stderr=log)
    (out / "red.json").write_text(json.dumps({"command": command, "exit": result.returncode}, indent=2) + "\n")
    assert result.returncode != 0, "behavioral checks must fail"
finally:
    p.write_bytes(original)

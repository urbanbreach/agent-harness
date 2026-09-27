from pathlib import Path
import json, subprocess
out = Path(__file__).resolve().parent
checks = [
    ("all", ["cargo", "nextest", "run", "--profile", "ci", "-p", "harness-tui", "--all-features"]),
    ("workspace-check", ["cargo", "check", "--workspace"]),
    ("clippy", ["cargo", "clippy", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"]),
    ("fmt", ["cargo", "fmt", "--all", "--", "--check"]),
    ("suite-gates", ["python3", "scripts/check-test-suite-gates.py"]),
]
results = []
for name, command in checks:
    with (out / f"{name}.log").open("w") as log:
        result = subprocess.run(command, stdout=log, stderr=log)
    results.append({"name": name, "command": command, "exit": result.returncode})
    (out / "checks.json").write_text(json.dumps(results, indent=2) + "\n")
    print(name, result.returncode, flush=True)
    result.check_returncode()

#!/usr/bin/env python3
"""Check backend size, test isolation, opt-in lanes, and retained fixture ownership."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
# Shared terminal owners remain outside the backend rewrite.
TUI_PREFIXES = (
    "crates/harness-tui/", "crates/harness/src/tui", "crates/harness/tests/tui",
    "crates/harness/tests/common/", "crates/harness/tests/config_schema_cli/",
    "crates/harness/tests/pty_", "crates/harness-testkit/tests/",
    "crates/harness-testkit/src/bin/", "crates/harness-testkit/examples/",
)
GLOBAL_MUTATION = re.compile(r"\b(?:std::)?env::(?:set_var|remove_var|set_current_dir)\s*\(")
SLEEP = re.compile(r"\b(?:std::thread|tokio::time)::sleep\s*\(")
SECRET = re.compile(r"\b(?:sk-(?:ant-)?[A-Za-z0-9_-]{20,}|ghp_[A-Za-z0-9]{20,}|AKIA[A-Z0-9]{16})\b|-----BEGIN [A-Z ]*PRIVATE KEY-----")


def backend(path):
    return path.startswith("crates/") and path.endswith(".rs") and not path.startswith(TUI_PREFIXES)


def rust_violations(path, text):
    if not backend(path):
        return []
    violations = []
    if len(text.splitlines()) >= 500:
        violations.append("backend Rust files must stay below 500 lines")
    if GLOBAL_MUTATION.search(text):
        violations.append("inject environment and cwd instead of mutating process-global state")
    native = "/native/" in path or Path(path).name in {"binary_smoke.rs", "live_proxy_e2e.rs"}
    tests = text if "/tests/" in path or path.endswith("_tests.rs") else text.partition("#[cfg(test)]")[2]
    if not native and SLEEP.search(tests):
        violations.append("use a bounded event wait instead of sleeping in tests")
    if native and "#[" in text and "test]" in text:
        gate = "HARNESS_MCP_LIVE_SIGNOFF" if path.endswith("live_proxy_e2e.rs") else "HARNESS_BINARY_SIGNOFF"
        if gate not in text or "return Err(" not in text:
            violations.append(f"native/live tests must reject runs without {gate}=1")
    return violations


def check(paths):
    violations = []
    for path in paths:
        file = ROOT / path
        if path.endswith(".rs"):
            for detail in rust_violations(path, file.read_text()):
                violations.append({"path": path, "detail": detail})
        elif path.endswith(".snap"):
            source = re.search(r"^source: (.+)$", file.read_text(), re.MULTILINE)
            if source and not (ROOT / source[1]).is_file():
                violations.append({"path": path, "detail": "snapshot source no longer exists"})
        elif "cassette" in path and file.suffix in {".json", ".jsonl"}:
            if SECRET.search(file.read_text()):
                violations.append({"path": path, "detail": "cassette contains a credential-shaped value"})
    config = tomllib.loads((ROOT / ".config/nextest.toml").read_text())
    default_filter = config["profile"]["default"]["default-filter"]
    for name in ("binary_smoke", "live_proxy_e2e", "pty_e2e", "native_visual_e2e"):
        if name not in default_filter:
            violations.append({"path": ".config/nextest.toml", "detail": f"default lane must exclude {name}"})
    return violations


def self_test():
    path = "crates/harness-core/tests/example.rs"
    assert not rust_violations(path, "#[test]\nfn behavior() {}\n")
    for source in ("// line\n" * 500, 'std::env::set_var("A", "B");', "std::thread::sleep(delay);"):
        assert rust_violations(path, source)
    assert not rust_violations("crates/harness/src/tui.rs", "// line\n" * 501)
    assert rust_violations("crates/harness-tools/tests/binary_smoke.rs", "#[test]\nfn native() {}")
    print("test-suite gate self-test: PASS")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--report-only", action="store_true")
    parser.add_argument("--format", action="store_true", help="Also check formatting of backend Rust files.")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    paths = subprocess.check_output(["rg", "--files", "--hidden", "-g", "!.git"], cwd=ROOT, text=True).splitlines()
    violations = check(paths)
    if args.format:
        result = subprocess.run(["rustfmt", "--edition", "2024", "--config", "skip_children=true", "--check", *[p for p in paths if backend(p)]], cwd=ROOT, check=False)
        if result.returncode:
            violations.append({"path": "crates/", "detail": "backend formatting check failed"})
    if args.json:
        print(json.dumps({"ok": not violations, "violations": violations}, indent=2))
    else:
        print(f"test-suite gates: {'FAIL' if violations else 'PASS'} ({len(violations)} violations)")
        for item in violations:
            print(f"- {item['path']}: {item['detail']}")
    return 0 if args.report_only or not violations else 1


if __name__ == "__main__":
    sys.exit(main())

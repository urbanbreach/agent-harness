from pathlib import Path
import json, subprocess
out = Path(__file__).resolve().parent
p = Path("crates/harness-tui/src/completion_controller/insertion.rs")
original = p.read_bytes()
old = b"Ok(ComposerEditor::from_buffer(next))"
assert original.count(old) == 1
command = ["cargo", "nextest", "run", "--profile", "ci", "-p", "harness-tui", "--all-features", "--no-fail-fast", "-E", "test(composer_editing::completion_tests) | binary(production_composer_reachability_test) | binary(completion_controller_test)"]
try:
    p.write_bytes(original.replace(old, b"Ok(editor.clone())"))
    (out / "red.patch").write_bytes(subprocess.check_output(["git", "diff", "--", str(p)]))
    with (out / "red.log").open("w") as log:
        result = subprocess.run(command, stdout=log, stderr=log)
    (out / "red.json").write_text(json.dumps({"command": command, "exit": result.returncode}, indent=2) + "\n")
    assert result.returncode != 0, "existing public tests must reject ignored completion insertion"
finally:
    p.write_bytes(original)

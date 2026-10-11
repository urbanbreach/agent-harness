#!/usr/bin/env python3
"""Exercise release validation and dry-run cleanup in disposable Git repositories."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

SOURCE = Path(__file__).resolve().parent


def run(args, cwd, **kwargs):
    return subprocess.run(args, cwd=cwd, text=True, capture_output=True, **kwargs)


def main():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        origin, repo, binaries = (root / name for name in ("origin", "repo", "bin"))
        repo.mkdir()
        binaries.mkdir()
        run(["git", "init", "--bare", str(origin)], root, check=True)
        run(["git", "init", "-b", "dev"], repo, check=True)
        for key, value in [("user.name", "Release test"), ("user.email", "release@example.invalid"), ("commit.gpgsign", "false"), ("tag.gpgsign", "false")]:
            run(["git", "config", key, value], repo, check=True)
        (repo / "scripts").mkdir()
        for name in ("release.sh", "release-notes.sh", "release-version.py"):
            shutil.copy2(SOURCE / name, repo / "scripts" / name)
        original = {
            "Cargo.toml": '[workspace.package]\nversion = "0.0.1"\n',
            "Cargo.lock": "# release fixture\n",
            "CHANGELOG.md": "# Changelog\n\n## [Unreleased]\n\n- A release fix.\n",
        }
        for name, body in original.items():
            (repo / name).write_text(body)
        cargo = binaries / "cargo"
        cargo.write_text('#!/bin/sh\nprintf "# changed\\n" >> Cargo.lock\nexit "${RELEASE_TEST_CARGO_EXIT:-0}"\n')
        cargo.chmod(0o755)
        env = dict(os.environ, PATH=f"{binaries}:{os.environ['PATH']}")
        run(["git", "add", "."], repo, check=True)
        run(["git", "commit", "-m", "fixture"], repo, check=True)
        run(["git", "remote", "add", "origin", str(origin)], repo, check=True)
        run(["git", "push", "-u", "origin", "dev"], repo, check=True)
        for tag in ("v0.0.1-rc.1", "v0.0.1-alpha.9", "v0.0.1-alpha.10", "v-not-a-release"):
            run(["git", "tag", tag], repo, check=True)
        before = run(["git", "show-ref"], repo, check=True).stdout
        for version, succeeds, cargo_exit in [
            ("0.0.1", True, "0"), ("0.0.1-rc.2", True, "0"),
            ("0.0.1-beta.1", False, "0"), ("0.0.1-rc.1+build", False, "0"),
            ("0.0.1-rc.01", False, "0"), ("00.0.2", False, "0"),
            ("0.0.2", False, "7"),
        ]:
            result = run(["bash", "scripts/release.sh", version, "--dry-run"], repo,
                         env=dict(env, RELEASE_TEST_CARGO_EXIT=cargo_exit))
            assert (result.returncode == 0) == succeeds, (version, result.stdout, result.stderr)
            assert all((repo / name).read_text() == body for name, body in original.items()), version
            assert run(["git", "show-ref"], repo, check=True).stdout == before
        # Prerelease numeric identifiers compare numerically, then stable sorts last.
        validator = repo / "scripts/release-version.py"
        for candidate, tags, succeeds in [("1.0.0-alpha.10", "v1.0.0-alpha.9", True), ("1.0.0-rc.9", "v1.0.0", False)]:
            result = run(["python3", str(validator), candidate], repo, input=tags)
            assert (result.returncode == 0) == succeeds, result.stderr
    print("Release version and dry-run checks passed")


if __name__ == "__main__":
    main()

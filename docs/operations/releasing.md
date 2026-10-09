# Releasing

A release is one command on your machine. GitHub Actions does the rest.

```bash
scripts/release.sh 0.1.0 --dry-run   # preview
scripts/release.sh 0.1.0             # release
```

## Before you release

- Add user-facing changes to `CHANGELOG.md` under `## [Unreleased]` as you merge
  them. The release script refuses to run while that section is empty.
- Check out `dev` (the default branch), commit everything and pull.
- Pick the version. Harness follows [Semantic Versioning](https://semver.org/):
  `0.1.0`, `0.1.1`, and so on. A suffix such as `0.2.0-rc.1` makes a prerelease,
  which the install script's "latest" download skips.

## What the script does

`scripts/release.sh <version>`:

1. Checks the version format, the branch, a clean tree, that `dev` is not behind
   `origin/dev`, that the tag doesn't exist yet and that the version is newer than
   the last `v*` tag.
2. Sets `[workspace.package] version` in `Cargo.toml` and refreshes `Cargo.lock`.
3. Renames the `[Unreleased]` changelog section to `[<version>] - <date>` and
   starts a new empty `[Unreleased]` above it.
4. Commits `chore: release v<version>`, creates the annotated tag `v<version>`,
   and pushes the branch and tag together (`git push --atomic`).

`--dry-run` prints the diff and the release notes, then restores the files.

## What CI does

The tag starts [`.github/workflows/release.yml`](../../.github/workflows/release.yml):

1. **metadata** checks that the tag matches the `Cargo.toml` version and extracts
   the release notes from `CHANGELOG.md`.
2. **ci** reruns the full CI workflow on the tagged commit.
3. **build** compiles static musl binaries on native runners: `x86_64` on
   `ubuntu-24.04` and `aarch64` on `ubuntu-24.04-arm`. `cargo-zigbuild` uses Zig
   as the musl C toolchain for the crates that compile C code, and musl builds
   use mimalloc as the global allocator. Each binary must be statically linked,
   report the tagged version, finish a mock run, and start in an Alpine container.
4. **publish** writes `SHA256SUMS`, attaches build provenance attestations,
   uploads everything to a draft release, checks the asset list, then publishes
   the release and marks it latest (prereleases are not marked latest).

Release assets:

| File | Contents |
| --- | --- |
| `harness-x86_64-unknown-linux-musl.tar.gz` | `harness`, `LICENSE`, `README.md`, `CHANGELOG.md` |
| `harness-aarch64-unknown-linux-musl.tar.gz` | The same for 64-bit ARM |
| `install.sh` | The installer (`scripts/install.sh`) |
| `harness.schema.json` | Runtime config schema (copy of `configs/config.json`) |
| `tui.schema.json` | Keyboard config schema (copy of `configs/tui.json`) |
| `SHA256SUMS` | Checksums of both archives, the installer, and both schemas |

Asset names carry no version, so
`https://github.com/urbanbreach/agent-harness/releases/latest/download/<asset>`
always points at the newest stable release.

Check where a downloaded archive was built with
`gh attestation verify harness-x86_64-unknown-linux-musl.tar.gz --repo urbanbreach/agent-harness`.

## When something fails

Don't run the release script again: the tag already exists.

- **CI or a build failed:** fix the cause on `dev`. If the fix changes code, cut
  the next patch version instead. If the failure was unrelated to the code (a
  flaky runner, an outage), open Actions > Release > Run workflow, enter the tag,
  and turn off dry run.
- **Publish failed:** rerun the workflow the same way. A leftover draft release is
  deleted and rebuilt. A release that is already public is never modified; cut a
  new version instead.
- **Wrong tag pushed and nothing published:** `git push --delete origin vX.Y.Z`,
  `git tag -d vX.Y.Z`, revert the release commit, and start again.

A manual run with dry run on (the default) builds and checks everything without
publishing. Use it to test the pipeline on an existing tag.

## Building a release binary locally

```bash
rustup target add x86_64-unknown-linux-musl
pip install cargo-zigbuild==0.23.4 ziglang==0.17.0
cargo zigbuild --release --locked -p harness --target x86_64-unknown-linux-musl
scripts/package-release.sh x86_64-unknown-linux-musl \
  target/x86_64-unknown-linux-musl/release/harness dist
```

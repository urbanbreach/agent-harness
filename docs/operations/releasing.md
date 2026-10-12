# Releasing

A release is one command on your machine. GitHub Actions does the rest.

```bash
scripts/release.sh 0.1.0 --dry-run   # preview
scripts/release.sh 0.1.0             # release
```

## Before you release

- Add user-facing changes to `CHANGELOG.md` under `## [Unreleased]` as you merge
  them. The release script refuses to run while that section is empty.
- Check out the branch named by `origin/HEAD`, commit everything and pull.
  The script falls back to `dev` if `origin/HEAD` is unavailable.
- Pick the version. Harness follows [Semantic Versioning](https://semver.org/):
  `0.1.0`, `0.1.1`, and so on. A suffix such as `0.2.0-rc.1` makes a prerelease,
  which the install script's "latest" download skips. Do not use SemVer build
  metadata (`+...`) for a release: the local validator accepts it, but the release
  workflow rejects such tags.

## What the script does

`scripts/release.sh <version>`:

1. Checks version format, the default branch, a clean tree, that HEAD contains
   `origin/<default-branch>`, and that the tag does not exist locally or remotely.
   The candidate must have higher SemVer precedence than every valid existing
   `v*` tag; build metadata does not increase precedence.
2. Sets `[workspace.package] version` in `Cargo.toml` and refreshes `Cargo.lock`.
3. Renames the `[Unreleased]` changelog section to `[<version>] - <date>` and
   starts a new empty `[Unreleased]` above it.
4. Commits `chore: release v<version>`, creates the annotated tag `v<version>`,
   and pushes the branch and tag together (`git push --atomic`).

`--dry-run` fetches tags, updates the lockfile and validates Cargo metadata, then
prints the diff summary and release notes. It restores `Cargo.toml`, `Cargo.lock`
and `CHANGELOG.md` on exit without committing or pushing.

## What CI does

The tag starts [`.github/workflows/release.yml`](../../.github/workflows/release.yml):

1. The `metadata` job checks the tag format, matches it against the `Cargo.toml`
   version and extracts release notes from `CHANGELOG.md`.
2. The `ci` job reruns the CI workflow on the tagged commit.
3. The `build` job compiles static musl binaries on native runners: `x86_64` on
   `ubuntu-24.04` and `aarch64` on `ubuntu-24.04-arm`. `cargo-zigbuild` uses Zig
   as the musl C toolchain for crates that compile C code. Musl binaries use
   mimalloc as the global allocator. Each binary must be statically linked,
   report the tagged version, finish a mock run and start in an Alpine container.
4. The `publish` job writes and verifies `SHA256SUMS`, attests the two archives,
   uploads the assets to a draft release and checks its asset list before
   publishing. Stable releases are marked latest; prereleases are not.

Release assets:

| File | Contents |
| --- | --- |
| `harness-x86_64-unknown-linux-musl.tar.gz` | `harness`, `LICENSE`, `LICENSES/`, `README.md`, `CHANGELOG.md` |
| `harness-aarch64-unknown-linux-musl.tar.gz` | The same for 64-bit ARM |
| `install.sh` | The installer (`scripts/install.sh`) |
| `harness.schema.json` | Runtime config schema (copy of `configs/config.json`) |
| `tui.schema.json` | Keyboard config schema (copy of `configs/tui.json`) |
| `SHA256SUMS` | Checksums of both archives, the installer, and both schemas |

Asset names carry no version, so
`https://github.com/urbanbreach/agent-harness/releases/latest/download/<asset>`
always points at the newest stable release.

Packaging requires `cargo-about 0.9.2`, the Rust `rust-docs` component and the
installed `ziglang` package. It checks locked dependencies against `about.toml`.
`LICENSES/` contains `dependencies.txt`, `syntax.txt`, `acorn.txt`,
`rust-stdlib.html`, `musl.txt` and `zig.txt`. Missing notices or an unaccepted
dependency license stop packaging.
The installer copies the project license to `harness-licenses/LICENSE` and
third-party notices to `harness-licenses/third-party/` beside the binary. Each
native build tests installation and checksum rejection in a temporary directory.
The installer accepts `--version` (`HARNESS_VERSION`, default `latest`) and
`--dir` (`HARNESS_INSTALL_DIR`, default `~/.local/bin`). It requires Linux on
x86-64 or aarch64, curl or wget, and sha256sum or shasum. It verifies the archive
against `SHA256SUMS` before extraction and replaces the binary by rename, so a
running process is not overwritten in place.

Check where a downloaded archive was built with
`gh attestation verify harness-x86_64-unknown-linux-musl.tar.gz --repo urbanbreach/agent-harness`.

## When something fails

If the tag was pushed, do not run the release script again for that version.

- If CI or a build fails because of code, fix it on the default branch and cut
  the next version. If the tagged code is unchanged and the failure came from
  a runner or service outage, open Actions > Release > Run workflow, enter the
  existing tag, and turn off dry run.
- If publishing fails, rerun the workflow with the same tag and dry run off.
  It deletes and rebuilds a leftover draft. It refuses to modify an already
  public release; cut a new version instead.
- If the wrong tag was pushed and nothing was published, delete only that tag
  (`git push --delete origin vX.Y.Z` and `git tag -d vX.Y.Z`), revert the release
  commit, and choose a version newer than all remaining release tags.
- If the atomic push fails, the local release commit and annotated tag remain.
  Resolve the push failure, then push the existing commit and tag together:
  `git push --atomic origin HEAD:refs/heads/<default-branch> refs/tags/vX.Y.Z`.
  Do not create a second release commit for the same version.

A manual run with dry run on (the default) builds and checks everything without
publishing. Use it to test the pipeline on an existing tag.

## Building a release binary locally

```bash
rustup target add x86_64-unknown-linux-musl
rustup component add rust-docs
python3 -m pip install cargo-zigbuild==0.23.4 ziglang==0.17.0
cargo install cargo-about --version 0.9.2 --locked
cargo zigbuild --release --locked -p harness --target x86_64-unknown-linux-musl
scripts/package-release.sh x86_64-unknown-linux-musl \
  target/x86_64-unknown-linux-musl/release/harness dist
bash scripts/test-package.sh dist/harness-x86_64-unknown-linux-musl.tar.gz
```

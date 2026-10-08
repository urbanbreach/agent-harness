#!/usr/bin/env bash
# Cut a release: stamp CHANGELOG.md, set the workspace version, commit, tag, push.
#
#   scripts/release.sh 0.1.0            # release
#   scripts/release.sh 0.1.0 --dry-run  # show the changes, then restore the tree
#
# Pushing the tag starts .github/workflows/release.yml, which reruns CI on the
# tagged commit, builds static binaries and publishes the GitHub Release.
# If that workflow fails, do not rerun this script: fix the cause and rerun the
# workflow (Actions > Release > Run workflow, tag vX.Y.Z, dry run off).
set -euo pipefail

usage() {
  echo "usage: $0 <version> [--dry-run]" >&2
  exit 2
}
[[ $# -ge 1 && $# -le 2 ]] || usage
version="${1#v}"
dry_run=false
if [[ $# -eq 2 ]]; then
  [[ "$2" == --dry-run ]] || usage
  dry_run=true
fi
tag="v$version"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

fail() {
  echo "release: $*" >&2
  exit 1
}

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] ||
  fail "'$version' is not a semantic version like 0.1.0 or 0.2.0-rc.1"

branch="$(git symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null || echo origin/dev)"
branch="${branch#origin/}"
[[ "$(git branch --show-current)" == "$branch" ]] || fail "check out $branch (the default branch) first"
[[ -z "$(git status --porcelain)" ]] || fail "the working tree has uncommitted changes"

git fetch --quiet --tags origin "$branch"
git merge-base --is-ancestor "origin/$branch" HEAD || fail "$branch is behind origin/$branch; pull first"
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null || git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null; then
  fail "tag $tag already exists"
fi
latest="$(git tag --list 'v*' --sort=-v:refname | head -n1)"
if [[ -n "$latest" ]]; then
  newest="$(printf '%s\n%s\n' "${latest#v}" "$version" | sort -V | tail -n1)"
  [[ "$newest" == "$version" ]] || fail "$version is not newer than $latest"
fi

notes="$(scripts/release-notes.sh Unreleased)" || fail "add your changes under '## [Unreleased]' in CHANGELOG.md"

# Version: [workspace.package] in Cargo.toml, then the workspace entries in Cargo.lock.
current="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)"
[[ -n "$current" ]] || fail "could not read [workspace.package] version from Cargo.toml"
if [[ "$current" != "$version" ]]; then
  sed -i "/^\[workspace.package\]/,/^\[/s/^version = \".*\"/version = \"$version\"/" Cargo.toml
fi
cargo update --workspace --quiet
cargo metadata --locked --format-version 1 >/dev/null

# CHANGELOG: the Unreleased body becomes the release section; a new empty Unreleased stays on top.
today="$(date -u +%Y-%m-%d)"
sed -i "0,/^## \[Unreleased\]\$/s//## [Unreleased]\n\n## [$version] - $today/" CHANGELOG.md
scripts/release-notes.sh "$version" >/dev/null

echo "Release $tag from $branch (Cargo version $current -> $version)"
git --no-pager diff --stat
echo
echo "Release notes:"
printf '%s\n' "$notes"

if $dry_run; then
  git checkout -- Cargo.toml Cargo.lock CHANGELOG.md
  echo
  echo "Dry run: changes restored, nothing committed or pushed."
  exit 0
fi

git commit --quiet -am "chore: release $tag"
git tag -a "$tag" -m "Harness $tag"
# Push branch and tag together so neither lands without the other.
git push --atomic origin "HEAD:refs/heads/$branch" "refs/tags/$tag"

echo
echo "Pushed $tag. Follow the release with:"
echo "  gh run watch \$(gh run list --workflow release.yml --limit 1 --json databaseId --jq '.[0].databaseId')"

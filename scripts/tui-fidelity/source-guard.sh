#!/usr/bin/env bash
set -euo pipefail

readonly PINNED_REVISION="500129c714ad1b10e6095481f4a8387a2ec52649"

fail() {
  printf 'source-guard: %s\n' "$1" >&2
  exit 1
}

usage() {
  cat <<'EOF'
usage: source-guard.sh verify --reference PATH --revision SHA [--input-root PATH]... [--receipt PATH]
EOF
}

resolve() {
  realpath -e -- "$1" 2>/dev/null || fail "cannot resolve path: $1"
}

git_value() {
  local root="$1"
  shift
  GIT_MASTER=1 git -C "$root" "$@" 2>/dev/null || fail "not a readable Git worktree: $root"
}

source_path_is_approved() {
  local kind="$1"
  local path="$2"
  case "$path" in
    Cargo.toml | */Cargo.toml | Cargo.lock | */Cargo.lock | rust-toolchain | */rust-toolchain | rust-toolchain.toml | */rust-toolchain.toml | *.rs)
      return 0
      ;;
  esac
  [[ "$kind" == "harness" ]] || return 1
  case "$path" in
    *.sh | *.py | *.json | *.jsonc | *.toml | *.yaml | *.yml)
      return 0
      ;;
  esac
  return 1
}

source_paths() {
  local root="$1"
  local kind="$2"
  local path
  while IFS= read -r -d '' path; do
    source_path_is_approved "$kind" "$path" || continue
    printf '%s\0' "$path"
  done < <(git_value "$root" ls-files -z)
}

tracked_source_hash() {
  local root="$1"
  local kind="$2"
  local path
  local -a paths=()
  while IFS= read -r -d '' path; do
    paths+=("$path")
  done < <(source_paths "$root" "$kind")
  (
    cd -- "$root"
    sha256sum -- "${paths[@]}"
  ) | LC_ALL=C sort | sha256sum | awk '{print $1}'
}

assert_tracked_symlinks_resolve() {
  local root="$1"
  local tracked
  while IFS= read -r tracked; do
    [[ -z "$tracked" ]] && continue
    realpath -e -- "$root/$tracked" >/dev/null 2>&1 \
      || fail "tracked symlink does not resolve: $root/$tracked"
  done < <(git_value "$root" ls-files -s | awk '$1 == "120000" {print $4}')
}

assert_reference_source_unchanged() {
  local root="$1"
  [[ -z "$(git_value "$root" diff --name-only HEAD --)" ]] \
    || fail "reference source mutation detected: $root"
}

reference_snapshot() {
  local root="$1"
  local status_hash
  status_hash="$(git_value "$root" status --porcelain=v1 --untracked-files=all | sha256sum | awk '{print $1}')"
  printf '%s|%s|%s|%s\n' \
    "$(git_value "$root" rev-parse HEAD)" \
    "$(git_value "$root" rev-parse 'HEAD^{tree}')" \
    "$status_hash" \
    "$(tracked_source_hash "$root" reference)"
}

path_is_within() {
  local path="$1"
  local root="$2"
  [[ "$path" == "$root" || "$path" == "$root/"* ]]
}

assert_approved_input_root() {
  local input="$1"
  local allowed_harness="$2"
  local allowed_evidence="$3"

  path_is_within "$input" "$allowed_evidence" && return 0
  case "/${input#"$allowed_harness"/}/" in
    */target/* | */node_modules/*)
      fail "excluded input root: $input"
      ;;
  esac
  case "$input" in
    "$allowed_harness"/Cargo.toml | "$allowed_harness"/Cargo.lock | "$allowed_harness"/rust-toolchain | "$allowed_harness"/rust-toolchain.toml)
      return 0
      ;;
    "$allowed_harness"/crates | "$allowed_harness"/scripts | "$allowed_harness"/configs | "$allowed_harness"/crates/* | "$allowed_harness"/scripts/* | "$allowed_harness"/configs/*)
      [[ -d "$input" ]] && return 0
      source_path_is_approved harness "${input#"$allowed_harness"/}" && return 0
      ;;
  esac

  fail "unapproved input root: $input"
}

[[ "${1:-}" == "verify" ]] || {
  usage >&2
  exit 2
}
shift

reference=""
revision=""
receipt=""
input_roots=()
while (($# > 0)); do
  case "$1" in
    --reference | --revision | --input-root | --receipt)
      (($# >= 2)) || fail "missing value for $1"
      case "$1" in
        --reference) reference="$2" ;;
        --revision) revision="$2" ;;
        --input-root) input_roots+=("$2") ;;
        --receipt) receipt="$2" ;;
      esac
      shift 2
      ;;
    *) fail "unknown argument: $1" ;;
  esac
done

[[ -n "$reference" ]] || fail "missing --reference"
[[ -n "$revision" ]] || fail "missing --revision"
[[ "$revision" == "$PINNED_REVISION" ]] \
  || fail "revision must equal pinned revision $PINNED_REVISION"

readonly script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
readonly harness_root="$(resolve "$script_dir/../..")"
readonly harness_common="$(resolve "$(git_value "$harness_root" rev-parse --path-format=absolute --git-common-dir)")"
readonly primary_root="$(dirname -- "$harness_common")"
readonly canonical_reference="$(resolve "$primary_root/inspirations/grok-build")"
if [[ ! -e "$reference" && ! -L "$reference" && "$reference" == "inspirations/grok-build" ]]; then
  reference="$canonical_reference"
fi
readonly reference_root="$(resolve "$reference")"
readonly canonical_reference_common="$(resolve "$(git_value "$canonical_reference" rev-parse --path-format=absolute --git-common-dir)")"
readonly reference_common="$(resolve "$(git_value "$reference_root" rev-parse --path-format=absolute --git-common-dir)")"
readonly evidence_root="$harness_root/.omo/evidence/task-1-grok-build-tui-experiential-parity"

[[ "$reference_common" == "$canonical_reference_common" ]] \
  || fail "reference is not an approved canonical worktree: $reference_root"
[[ "$(git_value "$reference_root" rev-parse HEAD)" == "$PINNED_REVISION" ]] \
  || fail "reference HEAD does not match pinned revision $PINNED_REVISION"
assert_reference_source_unchanged "$reference_root"
[[ -z "$(git_value "$reference_root" status --porcelain=v1 --untracked-files=all)" ]] \
  || fail "dirty reference source: $reference_root"

for input_root in "${input_roots[@]}"; do
  resolved_input="$(resolve "$input_root")"
  assert_approved_input_root "$resolved_input" "$harness_root" "$evidence_root"
done

assert_tracked_symlinks_resolve "$harness_root"
assert_tracked_symlinks_resolve "$reference_root"

readonly before="$(reference_snapshot "$reference_root")"
IFS='|' read -r reference_head reference_tree reference_status_sha256 reference_source_sha256 <<<"$before"
readonly harness_head="$(git_value "$harness_root" rev-parse HEAD)"
readonly harness_status_sha256="$(git_value "$harness_root" status --porcelain=v1 --untracked-files=all | sha256sum | awk '{print $1}')"
readonly harness_source_sha256="$(tracked_source_hash "$harness_root" harness)"
readonly harness_lock_sha256="$(sha256sum "$harness_root/Cargo.lock" | awk '{print $1}')"
readonly harness_toolchain_sha256="$(sha256sum "$harness_root/rust-toolchain.toml" | awk '{print $1}')"
readonly reference_lock_sha256="$(sha256sum "$reference_root/Cargo.lock" | awk '{print $1}')"
readonly reference_toolchain_path="$(git_value "$reference_root" ls-files | awk '/(^|\/)rust-toolchain(\.toml)?$/ {print; exit}')"
[[ -n "$reference_toolchain_path" ]] || fail "reference toolchain manifest is missing"
readonly reference_toolchain_sha256="$(sha256sum "$reference_root/$reference_toolchain_path" | awk '{print $1}')"
readonly rustc_version="$(rustc --version)"
readonly cargo_version="$(cargo --version)"
readonly after="$(reference_snapshot "$reference_root")"
[[ "$before" == "$after" ]] || fail "reference source mutation detected during verification"

receipt="${receipt:-$evidence_root/receipt.json}"
generated_receipt="$(mktemp)"
trap 'rm -f -- "$generated_receipt"' EXIT
jq -n \
  --arg schema "harness.tui-fidelity.source-guard.v1" \
  --arg reference "$reference_root" \
  --arg revision "$reference_head" \
  --arg tree "$reference_tree" \
  --arg reference_status_sha256 "$reference_status_sha256" \
  --arg reference_source_sha256 "$reference_source_sha256" \
  --arg reference_lock_sha256 "$reference_lock_sha256" \
  --arg reference_toolchain_sha256 "$reference_toolchain_sha256" \
  --arg harness "$harness_root" \
  --arg harness_revision "$harness_head" \
  --arg harness_status_sha256 "$harness_status_sha256" \
  --arg harness_source_sha256 "$harness_source_sha256" \
  --arg harness_lock_sha256 "$harness_lock_sha256" \
  --arg harness_toolchain_sha256 "$harness_toolchain_sha256" \
  --arg rustc "$rustc_version" \
  --arg cargo "$cargo_version" \
  '{schema: $schema, reference: {path: $reference, revision: $revision, tree: $tree, status_sha256: $reference_status_sha256, source_sha256: $reference_source_sha256, cargo_lock_sha256: $reference_lock_sha256, toolchain_sha256: $reference_toolchain_sha256, clean_pre: true, clean_post: true}, harness: {path: $harness, revision: $harness_revision, status_sha256: $harness_status_sha256, source_sha256: $harness_source_sha256, cargo_lock_sha256: $harness_lock_sha256, toolchain_sha256: $harness_toolchain_sha256}, tools: {rustc: $rustc, cargo: $cargo}}' \
  >"$generated_receipt"

if [[ -n "$receipt" ]]; then
  if [[ -e "$receipt" ]]; then
    cmp -s -- "$generated_receipt" "$receipt" || fail "existing receipt is stale or does not match current source"
  else
    mkdir -p -- "$(dirname -- "$receipt")"
    mv -- "$generated_receipt" "$receipt"
    trap - EXIT
  fi
else
  cat "$generated_receipt"
fi

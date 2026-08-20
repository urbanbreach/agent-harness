#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'tui-fidelity build-candidate: %s\n' "$1" >&2
  exit 64
}

sha256_file() {
  sha256sum -- "$1" | awk '{print $1}'
}

git_value() {
  GIT_MASTER=1 git -C "$repo_root" "$@"
}

source_path_is_approved() {
  case "$1" in
    Cargo.toml | */Cargo.toml | Cargo.lock | */Cargo.lock | rust-toolchain | */rust-toolchain | rust-toolchain.toml | */rust-toolchain.toml | *.rs | *.sh | *.py | *.json | *.jsonc | *.toml | *.yaml | *.yml)
      return 0
      ;;
  esac
  return 1
}

manifest_sha256() {
  local mode="$1"
  local path
  local digest
  if [[ "$mode" == tracked ]]; then
    while IFS= read -r -d '' path; do
      source_path_is_approved "$path" || continue
      if [[ -e "$repo_root/$path" ]]; then
        digest="$(sha256_file "$repo_root/$path")"
      else
        digest=deleted
      fi
      printf '%s  %s\0' "$digest" "$path"
    done < <(git_value ls-files -z)
  else
    while IFS= read -r -d '' path; do
      digest="$(sha256_file "$repo_root/$path")"
      printf '%s  %s\0' "$digest" "$path"
    done < <(git_value ls-files --others --exclude-standard -z)
  fi | LC_ALL=C sort -z | sha256sum | awk '{print $1}'
}

target_arg=""
receipt=""
authority_arg="configs/tui-fidelity-reference-authority.json"
reference_receipt_arg=""
diagnostic=0
while (($# > 0)); do
  case "$1" in
    --target-dir | --receipt | --authority | --reference-receipt)
      (($# >= 2)) || fail "missing value for $1"
      case "$1" in
        --target-dir) target_arg="$2" ;;
        --receipt) receipt="$2" ;;
        --authority) authority_arg="$2" ;;
        --reference-receipt) reference_receipt_arg="$2" ;;
      esac
      shift 2
      ;;
    --diagnostic-non-release)
      diagnostic=1
      shift
      ;;
    *) fail "unknown argument: $1" ;;
  esac
done

[[ -n "$target_arg" && -n "$receipt" ]] \
  || fail "usage: build-candidate.sh --target-dir PATH --receipt PATH [--authority PATH] [--reference-receipt PATH] [--diagnostic-non-release]"
repo_root="$(realpath -e -- "$(git rev-parse --show-toplevel)")"
status="$(git_value status --porcelain=v1 --untracked-files=all)"
if [[ -n "$status" && "$diagnostic" -eq 0 ]]; then
  fail "release candidate receipts require a clean worktree; use --diagnostic-non-release for immutable diagnostic evidence"
fi
target_dir="$(realpath -m -- "$repo_root/$target_arg")"
case "$target_dir" in
  "$repo_root/target"/*) ;;
  *) fail "target directory must be inside the worktree target directory" ;;
esac
if [[ "$receipt" = /* ]]; then
  receipt_path="$(realpath -m -- "$receipt")"
else
  receipt_path="$(realpath -m -- "$repo_root/$receipt")"
fi
if [[ "$authority_arg" = /* ]]; then
  authority_path="$(realpath -e -- "$authority_arg")"
else
  authority_path="$(realpath -e -- "$repo_root/$authority_arg")"
fi
if [[ -z "$reference_receipt_arg" ]]; then
  reference_receipt_arg="$(jq -er '.reference.receipt_path' "$authority_path")"
fi
if [[ "$reference_receipt_arg" = /* ]]; then
  reference_receipt_path="$(realpath -e -- "$reference_receipt_arg")"
else
  reference_receipt_path="$(realpath -e -- "$repo_root/$reference_receipt_arg")"
fi
authority_revision="$(jq -er '.reference.source_revision' "$authority_path")"
reference_root_arg="$(jq -er '.reference.canonical_checkout' "$authority_path")"
source_guard_dir="$(mktemp -d)"
source_guard_receipt="$source_guard_dir/receipt.json"
tmp_receipt="$(mktemp)"
trap 'rm -f -- "$source_guard_receipt" "$tmp_receipt"; rmdir -- "$source_guard_dir"' EXIT
bash "$repo_root/scripts/tui-fidelity/source-guard.sh" verify \
  --reference "$reference_root_arg" --revision "$authority_revision" \
  --receipt "$source_guard_receipt"

mkdir -p -- "$target_dir" "$(dirname -- "$receipt_path")"
CARGO_TARGET_DIR="$target_dir" cargo build -p harness --bin harness --locked
CARGO_TARGET_DIR="$target_dir" cargo build -p harness-testkit --bin tui-fidelity --locked
CARGO_TARGET_DIR="$target_dir" cargo build -p harness-testkit --bin tui_fidelity_aggregate --locked

head="$(git_value rev-parse HEAD)"
tree="$(git_value rev-parse 'HEAD^{tree}')"
clean=true
receipt_kind=release
release_eligible=true
clean_release=true
if [[ -n "$status" ]]; then
  clean=false
  receipt_kind=diagnostic_non_release
  release_eligible=false
  clean_release=false
fi
parity_acceptance_eligible=true
dirty_diff_sha256="$(git_value diff --binary HEAD -- | sha256sum | awk '{print $1}')"
cargo_config_sha256=null
if [[ -f "$repo_root/.cargo/config.toml" ]]; then
  cargo_config_sha256="\"$(sha256_file "$repo_root/.cargo/config.toml")\""
fi
jq -n \
  --arg schema_version "harness.tui-fidelity.candidate-binding.v2" \
  --arg receipt_kind "$receipt_kind" \
  --arg canonical_path "$repo_root" \
  --arg head "$head" \
  --arg tree "$tree" \
  --argjson clean "$clean" \
  --arg tracked_source_sha256 "$(manifest_sha256 tracked)" \
  --arg dirty_diff_sha256 "$dirty_diff_sha256" \
  --arg untracked_manifest_sha256 "$(manifest_sha256 untracked)" \
  --arg cargo_lock_sha256 "$(sha256_file "$repo_root/Cargo.lock")" \
  --arg toolchain_sha256 "$(sha256_file "$repo_root/rust-toolchain.toml")" \
  --argjson cargo_config_sha256 "$cargo_config_sha256" \
  --arg harness_sha256 "$(sha256_file "$target_dir/debug/harness")" \
  --arg runner_sha256 "$(sha256_file "$target_dir/debug/tui-fidelity")" \
  --arg aggregate_sha256 "$(sha256_file "$target_dir/debug/tui_fidelity_aggregate")" \
  --arg target_dir "$target_dir" \
  --arg authority_path "$authority_path" \
  --arg authority_revision "$authority_revision" \
  --arg authority_sha256 "$(sha256_file "$authority_path")" \
  --arg reference_receipt_path "$reference_receipt_path" \
  --arg reference_receipt_sha256 "$(sha256_file "$reference_receipt_path")" \
  --arg source_guard_receipt_sha256 "$(sha256_file "$source_guard_receipt")" \
  --argjson parity_acceptance_eligible "$parity_acceptance_eligible" \
  --argjson release_eligible "$release_eligible" \
  --argjson clean_release "$clean_release" \
  '{schema_version: $schema_version, receipt_kind: $receipt_kind, repository: {canonical_path: $canonical_path, head: $head, tree: $tree, clean: $clean, tracked_source_sha256: $tracked_source_sha256, dirty_diff_sha256: $dirty_diff_sha256, untracked_manifest_sha256: $untracked_manifest_sha256, cargo_lock_sha256: $cargo_lock_sha256, toolchain_sha256: $toolchain_sha256, cargo_config_sha256: $cargo_config_sha256}, binaries: {harness_sha256: $harness_sha256, runner_sha256: $runner_sha256, aggregate_sha256: $aggregate_sha256}, target_dir: $target_dir, authority: {path: $authority_path, revision: $authority_revision, sha256: $authority_sha256}, reference_receipt: {path: $reference_receipt_path, sha256: $reference_receipt_sha256}, source_guard_receipt_sha256: $source_guard_receipt_sha256, parity_acceptance_eligible: $parity_acceptance_eligible, release_eligible: $release_eligible, clean_release: $clean_release}' \
  >"$tmp_receipt"
mv -- "$tmp_receipt" "$receipt_path"
trap - EXIT
rm -f -- "$source_guard_receipt"
rmdir -- "$source_guard_dir"
printf 'receipt_kind=%s\nparity_acceptance_eligible=%s\nrelease_eligible=%s\nclean_release=%s\nhead=%s\ntarget_dir=%s\nreceipt=%s\n' \
  "$receipt_kind" "$parity_acceptance_eligible" "$release_eligible" "$clean_release" "$head" "$target_dir" "$receipt_path"

#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'tui-fidelity build-candidate: %s\n' "$1" >&2
  exit 64
}

target_arg=""
receipt=""
while (($# > 0)); do
  case "$1" in
    --target-dir)
      (($# >= 2)) || fail "missing value for --target-dir"
      target_arg="$2"
      shift 2
      ;;
    --receipt)
      (($# >= 2)) || fail "missing value for --receipt"
      receipt="$2"
      shift 2
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

[[ -n "$target_arg" && -n "$receipt" ]] || fail "usage: build-candidate.sh --target-dir PATH --receipt PATH"
repo_root="$(git rev-parse --show-toplevel)"
[[ -z "$(git status --porcelain=v1 --untracked-files=all)" ]] || fail "worktree must be clean before candidate build"
target_dir="$(realpath -m -- "$repo_root/$target_arg")"
case "$target_dir" in
  "$repo_root/target"/*) ;;
  *) fail "target directory must be inside the worktree target directory" ;;
esac
receipt_path="$(realpath -m -- "$repo_root/$receipt")"
mkdir -p -- "$target_dir" "$(dirname -- "$receipt_path")"

CARGO_TARGET_DIR="$target_dir" cargo build -p harness --bin harness --locked
CARGO_TARGET_DIR="$target_dir" cargo build -p harness-testkit --bin tui-fidelity --locked

candidate_sha="$(GIT_MASTER=1 git rev-parse HEAD)"
harness_bin="$target_dir/debug/harness"
runner_bin="$target_dir/debug/tui-fidelity"
harness_sha256="$(sha256sum "$harness_bin" | awk '{print $1}')"
runner_sha256="$(sha256sum "$runner_bin" | awk '{print $1}')"
tmp_receipt="$(mktemp "${receipt_path}.tmp.XXXXXX")"
trap 'rm -f -- "$tmp_receipt"' EXIT
jq -n \
  --arg candidate_sha "$candidate_sha" \
  --arg candidate_binary_sha256 "$harness_sha256" \
  --arg runner_sha256 "$runner_sha256" \
  --arg target_dir "$target_dir" \
  --arg freshness_relation 'current git HEAD + worktree-local isolated target' \
  '{candidate_sha: $candidate_sha, candidate_binary_sha256: $candidate_binary_sha256, runner_sha256: $runner_sha256, target_dir: $target_dir, freshness_relation: $freshness_relation}' \
  >"$tmp_receipt"
mv -- "$tmp_receipt" "$receipt_path"
printf 'candidate_sha=%s\nharness_sha256=%s\nrunner_sha256=%s\ntarget_dir=%s\nreceipt=%s\n' \
  "$candidate_sha" "$harness_sha256" "$runner_sha256" "$target_dir" "$receipt_path"

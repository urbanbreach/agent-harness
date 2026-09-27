#!/usr/bin/env bash
# Repeated local streaming checks, plus an explicitly opted-in provider smoke.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mode=all
artifact_root=""
harness_bin=""
config_path="${HARNESS_LIVE_PROXY_CONFIG-}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode|--artifact-dir|--harness-bin|--config)
      [[ $# -ge 2 && -n "$2" && "$2" != --* ]] || { printf 'Missing value for %s\n' "$1" >&2; exit 2; }
      case "$1" in
        --mode) mode="$2" ;;
        --artifact-dir) artifact_root="$(realpath -m "$2")" ;;
        --harness-bin) harness_bin="$(realpath -m "$2")" ;;
        --config) config_path="$(realpath -m "$2")" ;;
      esac
      shift 2 ;;
    --help)
      printf '%s\n' 'Usage: scripts/stress-harness.sh [--mode offline|live|all] [--artifact-dir PATH] [--harness-bin PATH] [--config PATH]'
      exit 0 ;;
    *) printf 'Unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
done
case "$mode" in offline|live|all) ;; *) printf 'Invalid mode: %s\n' "$mode" >&2; exit 2 ;; esac
cd "$root"
artifact_root="${artifact_root:-$root/target/harness-stress/$(date -u +%Y%m%d-%H%M%S)}"
mkdir -p "$artifact_root"
if [[ "$mode" != live ]]; then
  if [[ -z "$harness_bin" ]]; then
    cargo build --release -p harness --bin harness
    harness_bin="$root/target/release/harness"
  fi
  python3 scripts/measure-backend.py --binary "$harness_bin" --output "$artifact_root/backend.json" --repetitions 20
  python3 scripts/measure-delegation.py --binary "$harness_bin" --output "$artifact_root/delegation.json"
fi
if [[ "$mode" != offline ]]; then
  HARNESS_LIVE_PROXY_CONFIG="$config_path" bash scripts/harness-qa-live-smoke.sh --slug stress-live >"$artifact_root/live.log" 2>&1
fi
printf 'PASS mode=%s\nartifacts=%s\n' "$mode" "$artifact_root"

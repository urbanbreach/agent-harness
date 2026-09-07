#!/usr/bin/env bash
set -euo pipefail

before="${1:?usage: compare-probes.sh BEFORE AFTER}"
after="${2:?usage: compare-probes.sh BEFORE AFTER}"
workloads=(workspace)
for size in 100 500 1000 2000; do
  workloads+=("tui $size" "projection $size" "index $size")
done
for size in 100 1000 5000; do
  workloads+=("glob $size" "grep $size" "grep_sparse $size")
done
for size in 256 16384 65536 262144; do
  workloads+=("sse $size 64" "sse $size 4096")
done
for round in 1 2 3; do
  versions=(before after)
  if [[ "$round" == 2 ]]; then versions=(after before); fi
  for workload in "${workloads[@]}"; do
    read -r -a args <<< "$workload"
    for version in "${versions[@]}"; do
      "${!version}" "${args[@]}" | jq -c --arg version "$version" --argjson round "$round" '. + {version: $version, round: $round}'
    done
  done
done

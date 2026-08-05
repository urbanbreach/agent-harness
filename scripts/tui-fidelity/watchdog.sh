#!/usr/bin/env bash
set -euo pipefail

readonly DEADLINE_SECONDS=120
readonly TERM_GRACE_SECONDS=2

receipt_path=""
command=()
child_pid=""
watchdog_pid=""
timed_out=0
cancelled=0

usage() {
  cat <<'EOF'
usage: watchdog.sh [--receipt PATH] -- COMMAND [ARGUMENT]...
EOF
}

fail() {
  printf 'tui-fidelity watchdog: %s\n' "$1" >&2
  exit 64
}

while (($# > 0)); do
  case "$1" in
    --receipt)
      (($# >= 2)) || fail "missing value for --receipt"
      receipt_path="$2"
      shift 2
      ;;
    --)
      shift
      command=("$@")
      break
      ;;
    --help)
      usage
      exit 0
      ;;
    *)
      fail "expected -- before command"
      ;;
  esac
done

((${#command[@]} > 0)) || fail "missing command"
receipt_path="${receipt_path:-$PWD/watchdog-receipt.json}"

now_millis() {
  date +%s%3N
}

process_group_alive() {
  [[ -n "$child_pid" ]] && kill -0 -- "-$child_pid" 2>/dev/null
}

terminate_process_group() {
  if process_group_alive; then
    kill -TERM -- "-$child_pid" 2>/dev/null || true
    local remaining=$((TERM_GRACE_SECONDS * 10))
    while ((remaining > 0)) && process_group_alive; do
      sleep 0.1
      remaining=$((remaining - 1))
    done
    if process_group_alive; then
      kill -KILL -- "-$child_pid" 2>/dev/null || true
    fi
  fi
}

handle_timeout() {
  timed_out=1
  terminate_process_group
}

handle_cancel() {
  cancelled=1
  terminate_process_group
}

trap handle_timeout USR1
trap handle_cancel INT TERM

started_at="$(now_millis)"
setsid --wait -- "${command[@]}" &
child_pid=$!
(
  sleep "$DEADLINE_SECONDS"
  if kill -0 "$child_pid" 2>/dev/null; then
    kill -USR1 "$$" 2>/dev/null || true
  fi
) &
watchdog_pid=$!

raw_exit=0
if wait "$child_pid"; then
  raw_exit=0
else
  raw_exit=$?
fi
finished_at="$(now_millis)"

if [[ -n "$watchdog_pid" ]]; then
  kill "$watchdog_pid" 2>/dev/null || true
  wait "$watchdog_pid" 2>/dev/null || true
fi

status="failed"
exit_code="$raw_exit"
if ((timed_out)); then
  status="timed_out"
  exit_code=124
elif ((cancelled)); then
  status="cancelled"
  exit_code=130
elif ((raw_exit == 0)); then
  status="passed"
fi

surviving_group=false
if process_group_alive; then
  surviving_group=true
  terminate_process_group
fi

command_json="$(printf '%s\0' "${command[@]}" | jq -Rs 'split("\u0000")[:-1]')"
mkdir -p -- "$(dirname -- "$receipt_path")"
jq -n \
  --arg schema "harness.tui-fidelity.watchdog.v1" \
  --arg watchdog "scripts/tui-fidelity/watchdog.sh" \
  --arg status "$status" \
  --argjson command "$command_json" \
  --arg started_at "$started_at" \
  --arg finished_at "$finished_at" \
  --argjson duration_millis "$((finished_at - started_at))" \
  --argjson exit_code "$exit_code" \
  --argjson deadline_seconds "$DEADLINE_SECONDS" \
  --argjson timed_out "$([[ $timed_out -eq 1 ]] && printf true || printf false)" \
  --argjson cancelled "$([[ $cancelled -eq 1 ]] && printf true || printf false)" \
  --argjson surviving_process_group "$surviving_group" \
  '{schema_version: $schema, watchdog: $watchdog, status: $status, command: $command, started_at_millis: ($started_at | tonumber), finished_at_millis: ($finished_at | tonumber), duration_millis: $duration_millis, deadline_seconds: $deadline_seconds, exit_code: $exit_code, timed_out: $timed_out, cancelled: $cancelled, surviving_process_group: $surviving_process_group}' \
  >"$receipt_path"

exit "$exit_code"

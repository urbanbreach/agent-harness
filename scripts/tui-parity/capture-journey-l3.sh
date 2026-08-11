#!/usr/bin/env bash
# Capture nonvisual journey L3 evidence for a single journey row.
#
# Journeys exercise CLI/backend product surfaces (no terminal PNG), so each
# row's L3 capture directory is produced fresh by running the A-JOURNEYS
# offline owner test in `crates/harness/tests/journey_signoff_test.rs` in
# self-contained mode, then relocating the generated `journey-*-v1` evidence
# directory into the lane's fresh evidence root and writing a provenance
# `metadata.json` (behavior_id + generating_command) that the strict
# provenance validator (reference_parity_provenance) requires.
#
# Usage:
#   EVIDENCE_BASE=<actual-evidence-dir> bash scripts/tui-parity/capture-journey-l3.sh <journey-key>
#
# Journey keys (one stage per row, fail-closed, no silent skip):
#   worktree-owner           -> journey-worktree-owner-v1            (JOURNEY-WORKTREE-CTRL-W)
#   config-show-effective    -> journey-config-show-effective-v1     (JOURNEY-CONFIG-SHOW-EFFECTIVE)
#   config-sources-explain   -> journey-config-sources-explain-v1    (JOURNEY-CONFIG-SOURCES-EXPLAIN)
#   wait-any-all             -> journey-wait-any-all-v1              (JOURNEY-WAIT-ANY-ALL)
#   memory-cli               -> journey-memory-cli-v1                (JOURNEY-MEMORY-CLI)
#   folder-trust-deny        -> journey-folder-trust-deny-v1         (JOURNEY-FOLDER-TRUST-DENY)
#   always-approve-mode      -> journey-always-approve-mode-v1        (JOURNEY-ALWAYS-APPROVE-MODE)
#   settings-editor          -> journey-settings-editor-v1           (JOURNEY-SETTINGS-EDITOR)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

KEY="${1:-}"
EVIDENCE_BASE="${EVIDENCE_BASE:-artifacts/qa-evidence/20260717-tui-reference-parity/actual}"

case "$KEY" in
  worktree-owner)
    TESTS=(journey_worktree_ctrl_w_owner_is_env_gated_dual_binary_pty)
    DIR="journey-worktree-owner-v1"
    JID="JOURNEY-WORKTREE-CTRL-W"
    REQUIRE=("worktree-owner.json")
    ;;
  config-show-effective)
    TESTS=(journey_config_show_effective_cli_writes_artifact)
    DIR="journey-config-show-effective-v1"
    JID="JOURNEY-CONFIG-SHOW-EFFECTIVE"
    REQUIRE=("config-show-effective.stdout.txt" "config-show-effective.status.txt")
    ;;
  config-sources-explain)
    TESTS=(journey_config_sources_cli_writes_artifact journey_config_explain_cli_writes_artifact)
    DIR="journey-config-sources-explain-v1"
    JID="JOURNEY-CONFIG-SOURCES-EXPLAIN"
    REQUIRE=("config-sources.stdout.txt" "config-explain.stdout.txt")
    ;;
  wait-any-all)
    TESTS=(journey_wait_any_all_runs_owner_tests_and_writes_artifacts)
    DIR="journey-wait-any-all-v1"
    JID="JOURNEY-WAIT-ANY-ALL"
    REQUIRE=("wait-any-surface-receipt.json" "wait-any-owner-run.json")
    ;;
  memory-cli)
    TESTS=(journey_memory_cli_put_get_list_writes_artifacts)
    DIR="journey-memory-cli-v1"
    JID="JOURNEY-MEMORY-CLI"
    REQUIRE=("memory-cli-surface-receipt.json" "memory-put.stdout.txt" "memory-list.stdout.txt")
    ;;
  folder-trust-deny)
    TESTS=(journey_folder_trust_deny_documents_deny_path)
    DIR="journey-folder-trust-deny-v1"
    JID="JOURNEY-FOLDER-TRUST-DENY"
    REQUIRE=("folder-trust-deny-receipt.json" "folder-trust-deny.status.txt")
    ;;
  always-approve-mode)
    TESTS=(journey_always_approve_mode_appstate_render_writes_artifacts)
    DIR="journey-always-approve-mode-v1"
    JID="JOURNEY-ALWAYS-APPROVE-MODE"
    REQUIRE=("always-approve-surface-receipt.json" "always-approve-render.txt" "always-approve-state.json")
    ;;
  settings-editor)
    TESTS=(journey_settings_editor_appstate_render_writes_artifacts)
    DIR="journey-settings-editor-v1"
    JID="JOURNEY-SETTINGS-EDITOR"
    REQUIRE=("settings-editor-surface-receipt.json" "settings-editor-rows.txt" "settings-editor-render.txt" "settings-editor-state.json")
    ;;
  *)
    echo "blocked: unknown journey key '${KEY}'" >&2
    echo "usage: EVIDENCE_BASE=<dir> bash scripts/tui-parity/capture-journey-l3.sh <journey-key>" >&2
    exit 2
    ;;
esac

# Force self-contained (non-strict) mode: stable_l3_artifact_root writes under
# $HARNESS_JOURNEY_ARTIFACT_DIR/stable/... instead of the gitignored lab tree.
unset HARNESS_JOURNEY_STRICT || true
export TZ=UTC
export LANG=C.UTF-8
export LC_ALL=C.UTF-8

WORK="$(mktemp -d "${TMPDIR:-/tmp}/journey-l3-${KEY}-XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
export HARNESS_JOURNEY_ARTIFACT_DIR="$WORK"

echo "Capturing ${JID} L3 -> ${EVIDENCE_BASE}/${DIR}"
for TEST in "${TESTS[@]}"; do
  echo "  running owner test: ${TEST}"
  cargo nextest run -p harness --test journey_signoff_test -- "${TEST}"
done

# Relocate the generated stable evidence directory (basename match; the stable
# rel path differs per journey family but always ends in the capture dir name).
SRC="$(find "$WORK/stable" -maxdepth 12 -type d -name "$DIR" | head -1 || true)"
if [[ -z "$SRC" || ! -d "$SRC" ]]; then
  echo "FAIL: did not produce stable L3 directory ${DIR} under ${WORK}/stable" >&2
  exit 1
fi

DEST="${EVIDENCE_BASE:?}/${DIR}"
rm -rf "$DEST"
mkdir -p "$DEST"
cp -a "$SRC"/. "$DEST"/

for NEED in "${REQUIRE[@]}"; do
  if [[ ! -f "$DEST/$NEED" ]]; then
    echo "FAIL: ${JID} L3 missing required artifact: ${NEED}" >&2
    exit 1
  fi
done

cat >"$DEST/metadata.json" <<EOF
{
  "behavior_id": "${JID}",
  "journey_id": "${JID}",
  "row_kind": "journey",
  "surface": "nonvisual_cli_backend",
  "generating_command": "scripts/tui-parity/capture-journey-l3.sh ${KEY}",
  "owner_test": "crates/harness/tests/journey_signoff_test.rs::${TESTS[0]}",
  "capture_dir": "${DIR}"
}
EOF

if [[ ! -f "$DEST/metadata.json" ]]; then
  echo "FAIL: ${JID} L3 metadata.json was not written" >&2
  exit 1
fi
echo "OK: ${JID} (${#REQUIRE[@]} required artifacts + metadata.json)"

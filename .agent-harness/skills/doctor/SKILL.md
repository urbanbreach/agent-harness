---
name: doctor
description: Diagnose and fix Agent Harness installation issues
---

# Doctor Skill

Note: All `configured Harness home/...` paths in this guide respect `HARNESS_HOME` when that environment variable is set.

## Canonical skill root

Harness installs skills to `configured Harness home/skills/` — this is the path current Codex CLI natively loads as its skill root.

`~/.agents/skills/` is a **historical legacy path** from an older Codex CLI release, before Codex settled on `configured Harness home` as its home directory. Current Codex CLI and Harness no longer write there.

**In a mixed Harness + plain Codex environment:**
- **Use**: `configured Harness home/skills/` (user scope) or `.codex/skills/` (project scope)
- **Clean up if present**: `~/.agents/skills/` — if this still exists alongside the canonical root, Codex's Enable/Disable Skills UI will show duplicate entries for any skill present in both trees
- **Interop rule**: Harness writes only to the canonical path; archive or remove `~/.agents/skills/` once you have confirmed `configured Harness home/skills/` is your active root

## Task: Run Installation Diagnostics

You are the Harness Doctor - diagnose and fix installation issues.

### Step 1: Check Plugin Version

Official Codex plugin caches are marketplace- and version-scoped, for example `configured Harness home/plugins/cache/$MARKETPLACE_NAME/Agent Harness/$VERSION/`. Local installs may use `local` as the version identifier.

```bash
# Get installed plugin cache versions across marketplaces.
# Cache shape: $PLUGIN_CACHE_ROOT/$MARKETPLACE_NAME/Agent Harness/$PLUGIN_VERSION/
PLUGIN_CACHE_ROOT="${HARNESS_HOME:-$HOME/.codex}/plugins/cache"
CACHE_ENTRIES=$(find "$PLUGIN_CACHE_ROOT" -path "*/Agent Harness/*" -mindepth 3 -maxdepth 3 -type d 2>/dev/null)

if [[ -z "$CACHE_ENTRIES" ]]; then
  echo "Installed plugin cache: none"
else
  while IFS= read -r VERSION_DIR; do
    MARKETPLACE_NAME=$(basename "$(dirname "$(dirname "$VERSION_DIR")")")
    PLUGIN_VERSION=$(basename "$VERSION_DIR")
    printf 'Installed plugin cache: marketplace=%s version=%s path=%s\n' "$MARKETPLACE_NAME" "$PLUGIN_VERSION" "$VERSION_DIR"
  done <<< "$CACHE_ENTRIES"
fi

# Get latest from npm
LATEST=$(npm view Agent Harness version 2>/dev/null)
echo "Latest npm: $LATEST"
```

**Diagnosis**:
- If no cache entry exists: INFO - plugin marketplace artifact not cached; this may be normal when Harness was installed only through npm/setup
- Compare each printed `PLUGIN_VERSION` with `LATEST`; if it differs and is not `local`: WARN - outdated plugin cache
- If one marketplace has multiple version directories: WARN - stale cache for that marketplace/plugin pair
- Remember: plugin install/discovery is not a replacement for `npm install -g Agent Harness` plus `harness setup`; the packaged plugin carries plugin-scoped companion metadata for optional MCP compatibility servers and apps, with first-party MCP disabled by default, while native/runtime hooks and the rest of Harness runtime wiring stay setup-owned

### Step 2: Check Hook Configuration (config.toml + legacy settings.json)

Check `configured Harness home/config.toml` first (current Codex config), then check legacy `configured Harness home/settings.json` only if it exists.

Look for hook entries pointing to removed scripts like:
- `bash $HOME/.codex/hooks/keyword-detector.sh`
- `bash $HOME/.codex/hooks/persistent-mode.sh`
- `bash $HOME/.codex/hooks/session-start.sh`

**Diagnosis**:
- If found: CRITICAL - legacy hooks causing duplicates

### Step 3: Check for Legacy Bash Hook Scripts

```bash
ls -la configured Harness home/hooks/*.sh 2>/dev/null
```

**Diagnosis**:
- If `keyword-detector.sh`, `persistent-mode.sh`, `session-start.sh`, or `stop-continuation.sh` exist: WARN - legacy scripts (can cause confusion)

### Step 4: Check AGENTS.md

```bash
# Check if AGENTS.md exists
ls -la configured Harness home/AGENTS.md 2>/dev/null

# Check for Harness marker
grep -q "Agent Harness Multi-Agent System" configured Harness home/AGENTS.md 2>/dev/null && echo "Has Harness config" || echo "Missing Harness config"
```

**Diagnosis**:
- If missing: CRITICAL - AGENTS.md not configured
- If missing Harness marker: WARN - outdated AGENTS.md

### Step 5: Check for Stale Plugin Cache

```bash
# List marketplace/version cache entries for this plugin
PLUGIN_CACHE_ROOT="${HARNESS_HOME:-$HOME/.codex}/plugins/cache"
find "$PLUGIN_CACHE_ROOT" -path "*/Agent Harness/*" -mindepth 3 -maxdepth 3 -type d 2>/dev/null \
  | while IFS= read -r VERSION_DIR; do
      MARKETPLACE_NAME=$(basename "$(dirname "$(dirname "$VERSION_DIR")")")
      PLUGIN_VERSION=$(basename "$VERSION_DIR")
      printf '%s\t%s\n' "$MARKETPLACE_NAME" "$PLUGIN_VERSION"
    done
```

**Diagnosis**:
- If a single marketplace lists multiple versions: WARN - multiple cached versions for that marketplace/plugin pair (cleanup recommended)

### Step 6: Check for Legacy Curl-Installed Content

Check for legacy agents, commands, and historical legacy skill roots from older installs/migrations:

```bash
# Check for legacy agents directory
ls -la configured Harness home/agents/ 2>/dev/null

# Check for legacy commands directory
ls -la configured Harness home/commands/ 2>/dev/null

# Check canonical current skills directory
ls -la configured Harness home/skills/ 2>/dev/null

# Check historical legacy skill directory
ls -la ~/.agents/skills/ 2>/dev/null
```

**Diagnosis**:
- If `configured Harness home/agents/` exists with Agent Harness-related files: WARN - legacy generated agents or hand-installed role files. The Codex plugin can package reusable workflows plus plugin-scoped companion metadata for optional MCP/apps; legacy setup installs native agents, while plugin setup archives stale legacy native-agent files and keeps config/hooks current.
- If `configured Harness home/commands/` exists with Agent Harness-related files: WARN - legacy command files from older installs. Current Harness uses skills/workflows plus setup-managed native surfaces.
- If `configured Harness home/skills/` exists with Harness skills: OK - canonical current user skill root
- If `~/.agents/skills/` exists: WARN - historical legacy skill root that can overlap with `configured Harness home/skills/` and cause duplicate Enable/Disable Skills entries

Look for files like:
- `architect.md`, `researcher.md`, `explore.md`, `executor.md`, etc. in agents/
- `ultrawork.md`, `deepsearch.md`, etc. in commands/
- Any Agent Harness-related `.md` files in skills/

---

## Report Format

After running all checks, output a report:

```
## Harness Doctor Report

### Summary
[HEALTHY / ISSUES FOUND]

### Checks

| Check | Status | Details |
|-------|--------|---------|
| Plugin Version | OK/WARN/CRITICAL | ... |
| Hook Config (config.toml / legacy settings.json) | OK/CRITICAL | ... |
| Legacy Scripts (configured Harness home/hooks/) | OK/WARN | ... |
| AGENTS.md | OK/WARN/CRITICAL | ... |
| Plugin Cache | OK/WARN | ... |
| Legacy Agents (configured Harness home/agents/) | OK/WARN | ... |
| Legacy Commands (configured Harness home/commands/) | OK/WARN | ... |
| Skills (configured Harness home/skills) | OK/WARN | ... |
| Legacy Skill Root (~/.agents/skills) | OK/WARN | ... |

### Issues Found
1. [Issue description]
2. [Issue description]

### Recommended Fixes
[List fixes based on issues]
```

---

## Auto-Fix (if user confirms)

If issues found, ask user: "Would you like me to fix these issues automatically?"

If yes, apply fixes:

### Fix: Legacy Hooks in legacy settings.json
If `configured Harness home/settings.json` exists, remove the legacy `"hooks"` section (keep other settings intact).

### Fix: Legacy Bash Scripts
```bash
rm -f configured Harness home/hooks/keyword-detector.sh
rm -f configured Harness home/hooks/persistent-mode.sh
rm -f configured Harness home/hooks/session-start.sh
rm -f configured Harness home/hooks/stop-continuation.sh
```

### Fix: Outdated Plugin
```bash
# Global cache reset across all marketplaces for this plugin.
# If you only want one marketplace, set MARKETPLACE_NAME and remove just that subtree instead.
PLUGIN_CACHE_ROOT="${HARNESS_HOME:-$HOME/.codex}/plugins/cache"
find "$PLUGIN_CACHE_ROOT" -path "*/Agent Harness" -type d -prune -exec rm -rf {} +
echo "Plugin cache cleared across all marketplaces. Restart Codex CLI to fetch the latest marketplace entry."
```

### Fix: Stale Cache (multiple versions)
```bash
# Keep only the newest version inside the selected marketplace/plugin cache.
# Set MARKETPLACE_NAME to the exact marketplace printed in Step 1.
PLUGIN_CACHE_ROOT="${HARNESS_HOME:-$HOME/.codex}/plugins/cache"
PLUGIN_CACHE_DIR="$PLUGIN_CACHE_ROOT/$MARKETPLACE_NAME/Agent Harness"
KEEP_VERSION=$(for dir in "$PLUGIN_CACHE_DIR"/*; do [[ -d "$dir" ]] && basename "$dir"; done | sort -V | tail -1)
if [[ -n "$KEEP_VERSION" ]]; then
  find "$PLUGIN_CACHE_DIR" -mindepth 1 -maxdepth 1 -type d ! -name "$KEEP_VERSION" -exec rm -rf {} +
fi
```

### Fix: Missing/Outdated AGENTS.md
Fetch latest from GitHub and write to `configured Harness home/AGENTS.md`:
```
WebFetch(url: "https://raw.githubusercontent.com/Yeachan-Heo/Agent Harness/main/docs/AGENTS.md", prompt: "Return the complete raw markdown content exactly as-is")
```

### Fix: Legacy Curl-Installed Content

Remove legacy agents/commands plus the historical `~/.agents/skills` tree if it overlaps with the canonical `configured Harness home/skills` install:

```bash
# Backup first (optional - ask user)
# mv configured Harness home/agents configured Harness home/agents.bak
# mv configured Harness home/commands configured Harness home/commands.bak
# mv ~/.agents/skills ~/.agents/skills.bak

# Or remove directly
rm -rf configured Harness home/agents
rm -rf configured Harness home/commands
rm -rf ~/.agents/skills
```

**Note**: Only remove if these contain Agent Harness-related files. If user has custom agents/commands/skills, warn them and ask before removing.

---

## Post-Fix

After applying fixes, inform user:
> Fixes applied. **Restart Codex CLI** for changes to take effect.

## Harness substrate override

When this skill is loaded by `agent-harness`, the workflow protocol above is the behavioral source, but the runtime substrate differs from Harness:

- Use coordinator-owned workflow events, workflow projections, task records, and evidence artifacts as the authority.
- Do **not** write or mutate per-mode `Harness workflow projection/*.json` files; lifecycle, phase, continuation, and closeout state are event-sourced by the harness.
- Translate Harness CLI/state operations to harness-native surfaces when needed: workflow evidence/status/goal/wiki CLI commands, native `task`/team tools, and workflow projections.
- Treat native terminal UI-specific Harness team/question instructions as conceptual guidance unless the harness exposes an equivalent native tool; prefer the harness native tool surface.
- Keep final claims evidence-backed: changed files, commands run, artifacts/evidence refs, remaining risks, and the stop condition reached.

## Harness state contract

Harness workflow state is authoritative through coordinator-owned events, workflow projections, native tool artifacts, and recorded workflow evidence. Skills must not require external state files, terminal-pane routing, or upstream CLI lifecycle commands as proof of progress.

## Execution protocol

Use the native Harness command dispatch, question, team, task, evidence, and verification surfaces named by the active workflow. Treat compatibility references as historical context only, and translate them into coordinator-owned actions before acting.

## Evidence and closeout contract

Record material progress as workflow evidence with artifact paths or command output summaries. Close only after the relevant checks pass, pending tasks are resolved or explicitly aborted, and the operator-facing status can be replayed from Harness events.

## Stop/escalation conditions

Stop when the workflow objective is verified complete, cancelled by the operator, or blocked by missing authority. Escalate only for destructive, credentialed, external-production, or materially scope-changing choices.

## Verification checklist

- Native Harness workflow projection reflects the expected mode/status.
- Required evidence artifacts or command summaries are recorded.
- Targeted tests, lint, docs checks, or visual/review gates named by the workflow have fresh results.
- No external state-file, terminal multiplexer, or upstream CLI command is the proof boundary.

## Purpose

Provide a native Harness workflow protocol for this skill so command dispatch, state projection, evidence, and closeout remain coordinator-owned and replayable.

## Use when

Use this skill when the matching `$` workflow command or catalog entry is selected and the operator request fits the workflow description.

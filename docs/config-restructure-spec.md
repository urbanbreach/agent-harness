# Config Restructure Spec: Opencode-Style Agent Frontmatter Enrichment

**Created:** 2026-06-21
**Provenance:** Hyperplan adversarial process (5-member team, 3 rounds: analysis → cross-attack → defend/refine/concede), refined by plan agent.
**Status:** Ready for implementation

---

## 1. Executive Summary

The agent-harness config is "almost there but not quite" when compared to Opencode. The adversarial analysis discovered that **most features that seem missing already exist** — wildcard permissions, variable substitution, markdown frontmatter parsing. The real gaps are:

1. **The merge function is broken** — it silently drops 8 of 14 markdown frontmatter fields
2. **Discovery is first-wins** — project-level markdown can't override shipped agents
3. **Frontmatter struct is misaligned** — missing `enable`/`disable`/`use_small_model`/`PublicAgentTools`
4. **Permission field name mismatch** — `shell` vs `bash` across structs
5. **InertCompatibility keys pollute the JSON schema** — 10 keys accepted but silently ignored

**Answer to "should config + agent configs be split?":** No. Enrich the existing `.agent-harness/agents/*.md` files to carry full config in frontmatter, and fix the merge function so frontmatter fields actually take effect. Do NOT create a separate `agents.jsonc` file.

---

## 2. Context

### 2.1 Current State

The agent-harness config has a split agent definition model:
- `harness.jsonc`'s `agent` section: model, variant, tools, permissions, enable/disable
- `.agent-harness/agents/*.md`: description (frontmatter) + prompt body only

The merge function `merge_markdown_agent_with_config()` at `discovery.rs:136` is supposed to combine JSON config overrides with markdown frontmatter. But it gives JSON config winning precedence for 8 of 14 fields, even when JSON config has no value.

### 2.2 Opencode Comparison

Opencode (source in `inspirations/opencode/`) uses markdown files with full frontmatter (model, tools, permissions, prompt body all in one file). The key difference is that Opencode's markdown files ARE the primary agent definition, while agent-harness's markdown files are secondary to JSON config.

**What Opencode does that agent-harness already does (don't rebuild):**
- Markdown frontmatter parsing with typed structs
- Variable substitution (`{env:VAR}`, `{file:path}`)
- Wildcard permission selectors (`CatchAll`, `Prefix`, `Glob`, `Exact`)
- Config layering (global → project)

**What Opencode does that agent-harness should learn from:**
- Full agent config in markdown frontmatter (not just description + prompt)
- Last-wins discovery (project overrides global)
- Clean schema (no inert keys)

### 2.3 Key Code References

**Merge function** (`crates/harness-core/src/config/discovery.rs:136-178`):
```rust
fn merge_markdown_agent_with_config(
    config: &ProfileConfig,
    markdown: &MarkdownAgentFile,
) -> ProfileConfig {
    // ...
    ProfileConfig {
        // These fields CORRECTLY fall back to markdown:
        name: config.name.clone().or_else(|| markdown.frontmatter.name.clone()),
        system_prompt: prompt,  // falls back to markdown
        top_p: config.top_p.or(markdown.frontmatter.top_p),
        hidden: config.hidden || markdown.frontmatter.hidden.unwrap_or(false),
        color: config.color.clone().or_else(|| markdown.frontmatter.color.clone()),
        options: if config.options.is_empty() { markdown.frontmatter.options.clone() } else { config.options.clone() },

        // These fields INCORRECTLY ignore markdown frontmatter:
        description: config.description.clone(),           // BUG: no fallback
        model_ref: config.model_ref.clone(),                // BUG: no fallback
        model_ref_explicit: config.model_ref_explicit,      // BUG: no fallback
        variant: config.variant.clone(),                    // BUG: no fallback
        temperature: config.temperature,                    // BUG: no fallback
        permissions: config.permissions.clone(),             // BUG: no fallback
        max_iters: config.max_iters,                        // BUG: no fallback
        tool_failure_mode: config.tool_failure_mode,        // BUG: no fallback
        tools: config.tools.clone(),                        // BUG: no fallback
    }
}
```

**Discovery first-wins** (`crates/harness-core/src/config/discovery.rs:241-243`):
```rust
if agents.contains_key(&name) {
    continue;  // BUG: first discovery wins, project can't override shipped
}
```

**Frontmatter struct** (`crates/harness-core/src/config/discovery.rs:53-74`):
```rust
struct MarkdownAgentFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub system_prompt: Option<String>,
    pub model_ref: Option<String>,
    pub variant: Option<String>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub mode: Option<AgentMode>,
    pub hidden: Option<bool>,
    pub color: Option<String>,
    pub options: BTreeMap<String, serde_json::Value>,
    pub permissions: Option<ProfilePermissions>,
    pub max_iters: Option<usize>,
    pub tool_failure_mode: Option<ToolFailureMode>,
    pub tools: Option<Vec<String>>,  // Missing: should be PublicAgentTools enum
    // Missing: enable: Option<bool>
    // Missing: disable: bool
    // Missing: use_small_model: bool
}
```

**Permission field name mismatch** (`crates/harness-core/src/config.rs:797`):
```rust
pub struct ProfilePermissions {
    // ...
    pub shell: Option<PermissionMode>,  // No alias for "bash"
    // ...
}
```
While `PublicProfilePermissions` uses `bash` with `alias = "shell"`.

---

## 3. Scope

### 3.1 In Scope

1. Fix `merge_markdown_agent_with_config()` — 8 dropped frontmatter fields take effect when no JSON override
2. Fix `discover_markdown_agents()` — last-wins instead of first-wins
3. Add `#[serde(alias = "bash")]` to `ProfilePermissions::shell`
4. Align `MarkdownAgentFrontmatter` with `PublicAgentConfig` — add missing fields
5. Remove `InertCompatibility` keys from generated JSON schema
6. Document variable substitution and config layering
7. Update drift tests for all changes

### 3.2 Explicitly Out of Scope

- Creating a separate `agents.jsonc` file
- Switching from JSON5 to YAML frontmatter
- 5-layer or 3-layer config model restructuring
- Moving shipped agent defaults from Rust to markdown
- Renaming singular→plural keys (`agent`→`agents`, `provider`→`providers`)
- Dropping `default_agent` or `small_model`
- Compaction system redesign
- Permission system redesign to ordered array
- Adding CLI commands (`harness agent list`, `harness agent show`)
- Enterprise/MDM config support
- Plugin system
- Remote config support

### 3.3 Must Not Break

- Plan agent's `PermissionRuleSet` (agents.rs:304-317) must stay in Rust code, not markdown
- Hidden system agents (`title`, `summary`, `compaction`) must stay compiled-in Rust defaults
- `default_shipped_agents()` must stay called before markdown discovery
- All existing configs must continue to work without modification
- All existing tests must continue to pass
- `doctor --json` output must remain backward-compatible
- Support export must remain backward-compatible

---

## 4. Opencode Reference Guide

The implementer MUST actively refer to the Opencode source code in `inspirations/opencode/` throughout implementation. The following files are required reading:

### 4.1 Required Reading (read before starting)

| File | What to learn |
|------|---------------|
| `inspirations/opencode/packages/core/src/v1/config/agent.ts` | How Opencode defines agent config fields. Note the `mode`, `steps`, `permission`, `tools` fields. |
| `inspirations/opencode/packages/core/src/v1/config/permission.ts` | How Opencode structures permissions (simple map + catch-all). |
| `inspirations/opencode/packages/opencode/src/config/agent.ts` | How Opencode discovers and loads markdown agent files. Note `ConfigAgent.load()` and the `{agent,agents}/**/*.md` glob pattern. |
| `inspirations/opencode/.opencode/agent/triage.md` | A real example of an Opencode agent markdown file with full frontmatter. |
| `inspirations/opencode/specs/v2/config.md` | Opencode's V2 config spec. Learn what they're moving AWAY from (don't copy their mistakes). |

### 4.2 Reference Reading (consult as needed)

| File | What to learn |
|------|---------------|
| `inspirations/opencode/packages/core/src/v1/config/config.ts` | Full top-level config schema. Compare with `contract.rs`. |
| `inspirations/opencode/packages/opencode/src/config/config.ts` | Config loading and merge logic. Note `mergeDeep` usage. |
| `inspirations/opencode/packages/opencode/src/config/paths.ts` | Config path resolution and directory traversal. |
| `inspirations/opencode/packages/core/src/permission/schema.ts` | V2 permission ruleset schema (ordered array of rules). |
| `inspirations/opencode/packages/core/src/permission.ts` | V2 permission evaluation logic (`findLast` — last match wins). |
| `inspirations/opencode/packages/core/src/util/wildcard.ts` | Wildcard matching implementation. |

### 4.3 How to Use the Opencode Source

1. **Before each task**, read the relevant Opencode file to understand the pattern
2. **When designing tests**, check how Opencode tests similar behavior
3. **When making design decisions** (within your freedom), check what Opencode does and decide whether to follow or diverge
4. **When documenting**, reference Opencode patterns where they inform the design
5. **Do NOT copy Opencode's code** — this is a Rust workspace, not TypeScript. Learn the patterns, implement in idiomatic Rust.

---

## 5. Tasks

### Task 1: Fix merge function field precedence

**File:** `crates/harness-core/src/config/discovery.rs`
**Function:** `merge_markdown_agent_with_config()` (line 136)

**Problem:** 8 of 14 frontmatter fields are silently dropped because JSON config always wins, even when it has no value:

| Field | Current (broken) | Required (fixed) |
|-------|-----------------|-----------------|
| `description` | `config.description.clone()` | `config.description.clone().or_else(\|\| markdown.frontmatter.description.clone())` |
| `model_ref` | `config.model_ref.clone()` | `config.model_ref.clone().or_else(\|\| markdown.frontmatter.model_ref.clone())` — BUT see note below |
| `model_ref_explicit` | `config.model_ref_explicit` | `config.model_ref_explicit \|\| markdown.frontmatter.model_ref.is_some()` |
| `variant` | `config.variant.clone()` | `config.variant.clone().or_else(\|\| markdown.frontmatter.variant.clone())` |
| `temperature` | `config.temperature` | `config.temperature.or(markdown.frontmatter.temperature)` |
| `permissions` | `config.permissions.clone()` | `config.permissions.clone().or_else(\|\| markdown.frontmatter.permissions.clone())` |
| `max_iters` | `config.max_iters` | `config.max_iters.or(markdown.frontmatter.max_iters)` |
| `tool_failure_mode` | `config.tool_failure_mode` | See note below |
| `tools` | `config.tools.clone()` | `if config.tools.is_empty() { markdown.frontmatter.tools.clone().unwrap_or_default() } else { config.tools.clone() }` |

**Note on `model_ref`:** `ProfileConfig.model_ref` is a `String` (not `Option<String>`), so it always has a value. The fallback to markdown only matters when the config was built from `profile_from_markdown_agent()` which uses `"default:default"` as fallback. The implementer should check whether `config.model_ref == "default:default"` or use a different approach. **The implementer has freedom to decide the exact approach** as long as:
- When JSON config explicitly sets a model, JSON wins
- When JSON config doesn't set a model but markdown frontmatter does, markdown wins
- When neither sets a model, the existing fallback behavior is preserved

**Note on `tool_failure_mode`:** `ProfileConfig.tool_failure_mode` is a `ToolFailureMode` (not `Option<ToolFailureMode>`), and it defaults to `ContinueAsToolMessage`. The implementer should check whether the config's value is the default and fall back to markdown if so. **The implementer has freedom** to decide the exact approach.

**TDD requirement:** Write tests FIRST that verify:
1. Markdown `description` takes effect when JSON config has no `description`
2. Markdown `model_ref` takes effect when JSON config has no explicit model
3. Markdown `variant` takes effect when JSON config has no `variant`
4. Markdown `temperature` takes effect when JSON config has no `temperature`
5. Markdown `permissions` takes effect when JSON config has no `permissions`
6. Markdown `max_iters` takes effect when JSON config has no `max_iters`
7. Markdown `tool_failure_mode` takes effect when JSON config has the default
8. Markdown `tools` take effect when JSON config has empty tools
9. JSON config still wins when both are present (precedence preserved for ALL fields)
10. Existing behavior is preserved when no markdown exists

**No existing tests exist for this function** — tests must be written from scratch.

**Verification:**
```bash
cargo test -p harness-core -- config::discovery
cargo test -p harness-core
```

---

### Task 2: Fix discovery first-wins → last-wins

**File:** `crates/harness-core/src/config/discovery.rs`
**Function:** `discover_markdown_agents()` (line 217)

**Problem:** Line 241 has `if agents.contains_key(&name) { continue; }` which means the first discovered file wins. Project-level markdown can't override shipped agents.

**Fix:**
1. Remove the `if agents.contains_key(&name) { continue; }` guard at line 241
2. Reorder search dirs so that shipped/global dirs come FIRST and project-level dirs come LAST — this way last-wins gives project-level precedence

**Search dir ordering:** Check `agent_prompt_search_dirs()` (line 372) and `discovery_search_bases()` (line 400). The implementer must verify the current ordering and reverse it if needed so that:
- Shipped agents (`.agent-harness/agents/`) are discovered first
- Global config dir agents are discovered second
- Project-level agents are discovered last (and win)

**TDD requirement:** Write tests FIRST that verify:
1. Two markdown files with the same name in different dirs — the last-discovered one wins
2. Project-level markdown overrides a shipped agent with the same name
3. Shipped agents still load correctly when no project-level override exists
4. The search dir order is: shipped → global → project (so project wins)

**Verification:**
```bash
cargo test -p harness-core -- config::discovery
cargo test -p harness-core
cargo test -p harness --test config_schema_cli_test
```

---

### Task 3: Add bash alias to ProfilePermissions::shell

**File:** `crates/harness-core/src/config.rs`
**Struct:** `ProfilePermissions` (line 791)

**Problem:** `ProfilePermissions.shell` (line 797) has no `#[serde(alias = "bash")]`, while `PublicProfilePermissions` uses `bash` with `alias = "shell"`. Markdown frontmatter uses `ProfilePermissions`, so users must write `shell` in frontmatter but `bash` in JSON config.

**Fix:** Add `#[serde(alias = "bash")]` to the `shell` field:
```rust
#[serde(default, alias = "bash")]
pub shell: Option<PermissionMode>,
```

**TDD requirement:** Write a test FIRST that parses markdown frontmatter with `bash: "allow"` and verifies it maps to `ProfilePermissions.shell`.

**Verification:**
```bash
cargo test -p harness-core -- config
cargo test -p harness-core
```

---

### Task 4: Align MarkdownAgentFrontmatter with PublicAgentConfig

**File:** `crates/harness-core/src/config/discovery.rs`
**Struct:** `MarkdownAgentFrontmatter` (line 53)
**Depends on:** Task 1 (merge function must be fixed first)

**Problem:** `MarkdownAgentFrontmatter` is missing 3 fields from `PublicAgentConfig` and uses a different type for `tools`.

**Fields to add:**
1. `enable: Option<bool>` — with `#[serde(default, alias = "enabled")]`
2. `disable: bool` — with `#[serde(default)]`
3. `use_small_model: bool` — with `#[serde(default, alias = "smallModel")]`

**Field to change:**
4. `tools: Option<Vec<String>>` → `tools: Option<PublicAgentTools>` — to support both List and Map shapes. Import `PublicAgentTools` from `crates/harness-core/src/config/public/agents.rs`.

**Changes to `ProfileConfig`** (`crates/harness-core/src/config.rs:729`):
- Add `enabled: Option<bool>` field with `#[serde(default)]` and `#[schemars(skip)]` (internal-only, not in public schema)

**Changes to `profile_from_markdown_agent()`** (discovery.rs:180):
- Handle `enable`/`disable`: if `disable == true` or `enable == Some(false)`, return `Ok(None)` (agent is disabled)
- Store `enabled` in `ProfileConfig`
- Handle `use_small_model`: if true, use the small model ref (requires passing `small_model_ref` parameter to the function)
- Handle `PublicAgentTools`: call `.tool_ids()` to convert to `Vec<String>`

**Changes to `merge_markdown_agent_with_config()`** (discovery.rs:136):
- Merge `enabled` field: JSON config wins, markdown is fallback
- Handle `disable` from markdown

**Changes to `merge_configured_and_markdown_agents()`** (discovery.rs:99):
- Pass `small_model_ref` to `profile_from_markdown_agent()` — requires plumbing the small model ref through the call chain
- Filter out disabled agents after merge

**Changes to function signatures:**
- `profile_from_markdown_agent()` needs a new `small_model_ref: Option<&str>` parameter
- `merge_configured_and_markdown_agents()` needs to receive `small_model_ref` — either as a parameter or from the configured profiles

**TDD requirement:** Write tests FIRST that verify:
1. Markdown frontmatter with `enable: false` disables the agent
2. Markdown frontmatter with `disable: true` disables the agent
3. Markdown frontmatter with `use_small_model: true` selects the small model
4. Markdown frontmatter with `tools: { "read": true, "bash": false }` (Map shape) parses correctly
5. Markdown frontmatter with `tools: ["read", "grep"]` (List shape) still works
6. Disabled agents don't appear in the final agent map
7. JSON config `enable: false` still overrides markdown `enable: true`

**Verification:**
```bash
cargo test -p harness-core -- config::discovery
cargo test -p harness-core
cargo test -p harness --test config_docs_reference_test
cargo test -p harness --test config_schema_cli_test
cargo test -p harness --test bootstrap_profiles_test
```

---

### Task 5: Remove InertCompatibility keys from JSON schema

**File:** `crates/harness-core/src/config/public/contract.rs`
**Variable:** `PUBLIC_RUNTIME_TOP_LEVEL_CONFIG_KEYS` (line 191)

**Problem:** 10 `InertCompatibility` keys have `schema_property: true` and `docs_table_row: true`, meaning they appear in the generated JSON schema and docs but are silently ignored.

**Fix:** Change each `InertCompatibility` key from `schema, docs` to `no_schema, no_docs`:

| Key | Line (approx) |
|-----|---------------|
| `compaction` | 215 |
| `experimental` | 226 |
| `layout` | 229 |
| `logLevel` | 230 |
| `shell` | 260 |
| `snapshot` | 263 |
| `tool_output` | 264 |
| `tools` | 265 |
| `username` | 266 |
| `watcher` | 267 |

**TDD requirement:** Write a test FIRST that verifies these keys do NOT appear in the generated JSON schema properties.

**Verification:**
```bash
cargo test -p harness --test config_docs_reference_test
cargo test -p harness --test config_schema_cli_test
cargo run -p harness -- --config configs/harness.example.jsonc config validate
```

---

### Task 6: Document variable substitution and config layering

**Files:** `docs/config.md`, `configs/harness.example.jsonc`

**Problem:** Variable substitution (`{env:VAR}`, `{file:path}`, `${VAR:-fallback}`) exists in `loader.rs:345-405` but is undocumented. Config layering (XDG global → project local → agent markdown) is partially documented but not clearly explained.

**Fix:**
1. Add a "Variable Substitution" section to `docs/config.md` documenting:
   - `{env:VAR}` — environment variable substitution (returns empty string if missing)
   - `{file:path}` — file content substitution
   - `${VAR}` — shell-style environment variable
   - `${VAR:-fallback}` — environment variable with fallback value
   - Applied recursively to all config values via `resolve_config_value_references_with_lookup()`
   - Note: `apiKeyEnv` in provider config is a separate mechanism (multi-env fallback chain with credential redaction) — NOT the same as `{env:VAR}`

2. Add a "Config Layering" section to `docs/config.md` documenting:
   - XDG global config (`$XDG_CONFIG_HOME/harness/harness.jsonc`)
   - Project local config (`./harness.jsonc`)
   - Agent markdown files (`.agent-harness/agents/*.md`)
   - Merge precedence: project local overrides XDG global; agent markdown frontmatter overrides JSON config `agent` section (after Task 1 fix)
   - Discovery order: shipped → global → project (last-wins, after Task 2 fix)

3. Add variable substitution examples to `configs/harness.example.jsonc` in comments

4. Reference Opencode's config documentation at `inspirations/opencode/packages/web/src/content/docs/config.mdx` for style and structure inspiration

**Verification:**
```bash
cargo test -p harness --test config_docs_reference_test
```

---

### Task 7: Update drift tests and final verification

**Files:** `crates/harness/tests/config_docs_reference_test.rs`, `crates/harness/tests/config_schema_cli_test.rs`
**Depends on:** Tasks 1-6

**Problem:** Drift tests validate that docs, schema, and contract are in sync. Changes from Tasks 1-6 will cause drift test failures that must be resolved.

**Fix:**
1. Update `config_docs_reference_test.rs` to reflect:
   - InertCompatibility keys removed from schema (Task 5)
   - New frontmatter fields documented (Task 4)
   - Variable substitution documented (Task 6)
2. Update `config_schema_cli_test.rs` sub-tests as needed
3. Update any snapshot files that capture the generated JSON schema
4. Regenerate `configs/config.json` (generated schema) if needed

**Verification (run ALL — no skipping):**
```bash
cargo test -p harness-core
cargo test -p harness --test config_docs_reference_test
cargo test -p harness --test config_schema_cli_test
cargo test -p harness --test bootstrap_profiles_test
cargo run -p harness -- --config configs/harness.example.jsonc config validate
cargo run -p harness -- --config configs/harness.example.jsonc doctor --json
scripts/test-lanes.sh fast
scripts/test-lanes.sh quality-gates
```

---

## 6. Verification Protocol

### 6.1 Per-Task Verification

Each task MUST be verified before moving to the next. Verification means:

1. **Run the task-specific test command** — the test must PASS, not just compile
2. **Run `cargo test -p harness-core`** — all existing tests must still pass
3. **Run `cargo clippy -p harness-core -- -D warnings`** — no warnings
4. **Run `cargo fmt --check -p harness-core`** — no formatting issues
5. **Show the actual command output** — do not claim "should pass" without running

### 6.2 Final Verification (Task 7)

After all tasks are complete, run the FULL verification suite:

```bash
# Core tests
cargo test -p harness-core

# Drift tests
cargo test -p harness --test config_docs_reference_test
cargo test -p harness --test config_schema_cli_test
cargo test -p harness --test bootstrap_profiles_test

# CLI verification
cargo run -p harness -- --config configs/harness.example.jsonc config validate
cargo run -p harness -- --config configs/harness.example.jsonc doctor --json

# Test lanes
scripts/test-lanes.sh fast
scripts/test-lanes.sh quality-gates

# End-to-end runtime verification (Section 6.4)
# Run all 6 steps and show output
```

ALL commands must pass with zero failures. Any failure must be fixed before declaring completion. The E2E verification (Section 6.4) is especially critical — it proves that unit test fixes actually take effect at runtime, not just in isolation.

### 6.3 Evidence Requirements

For each task, the implementer must provide:

1. **Test output** — actual stdout/stderr from running tests, not summaries
2. **Diff** — `git diff` showing exactly what changed
3. **File list** — every file that was modified, created, or deleted
4. **Verification command output** — actual output from running the verification commands

Claims without evidence are invalid. "Should pass" is not evidence. "Tests pass" without showing output is not evidence.

### 6.4 End-to-End Runtime Verification

Unit tests and `config validate` are necessary but NOT sufficient. The implementer MUST also verify that enriched markdown frontmatter fields actually take effect at runtime by running the real harness binary against a test workspace with custom agent markdown files.

**This verification is mandatory after Task 4 (frontmatter alignment) and Task 7 (final verification).**

#### Setup

Create a temporary test workspace at `/tmp/harness-e2e-test/`:

```
/tmp/harness-e2e-test/
├── harness.jsonc          # minimal config pointing to mock provider
├── .agent-harness/
│   └── agents/
│       └── custom-test.md  # custom agent with FULL frontmatter
```

The `custom-test.md` file MUST include frontmatter that exercises every field fixed by Tasks 1-4:

```jsonc
---
{
  "description": "E2E test agent for frontmatter enrichment verification",
  "model": "umans-ai-coding-plan/umans-glm-5.2",
  "variant": "high",
  "temperature": 0.3,
  "permissions": {
    "bash": "ask",
    "edit": "allow",
    "rules": {
      "edit": [
        { "selector": "*", "mode": "deny" },
        { "selector": "prefix:/tmp/", "mode": "allow" }
      ]
    }
  },
  "tools": ["read", "glob", "grep", "list", "bash"],
  "max_iters": 5,
  "tool_failure_mode": "continue_as_tool_message"
}
---

You are a test agent for E2E verification.
```

The `harness.jsonc` in the test workspace MUST use the mock provider (no network calls):

```jsonc
{
  "$schema": "../../srv/samba/code/accela/agent-harness/configs/config.json",
  "provider": {
    "mock": {
      "type": "mock",
      "models": { "default": { "name": "Mock" } }
    }
  },
  "model": "mock/default",
  "default_agent": "custom-test",
  "permission": "ask"
}
```

**Note:** The implementer has freedom to adjust the exact test workspace structure, config contents, and agent frontmatter as long as it exercises the fields fixed by Tasks 1-4. If the mock provider requires cassettes, check `crates/harness-providers/` for existing cassette fixtures or create a minimal one.

#### Verification Steps

Run each of these commands and show actual output:

**Step 1: Doctor reports the custom agent with correct config**

```bash
cargo run -p harness -- --config /tmp/harness-e2e-test/harness.jsonc doctor --json 2>&1 | tee /tmp/harness-e2e-doctor.json
```

Verify in the output:
- The `custom-test` agent appears in the agent catalog
- Its model is `umans-ai-coding-plan/umans-glm-5.2` (from markdown frontmatter, NOT from JSON config)
- Its variant is `high` (from markdown frontmatter)
- Its temperature is `0.3` (from markdown frontmatter)
- Its tools list includes `read`, `glob`, `grep`, `list`, `bash` (from markdown frontmatter)
- Its permissions include `bash: ask`, `edit: allow` (from markdown frontmatter)
- Its `max_iters` is `5` (from markdown frontmatter)

**Step 2: Config validation passes**

```bash
cargo run -p harness -- --config /tmp/harness-e2e-test/harness.jsonc config validate
```

Must exit 0 with no errors.

**Step 3: Mock run uses the custom agent**

```bash
cargo run -p harness -- --config /tmp/harness-e2e-test/harness.jsonc run --mock "test" --out /tmp/harness-e2e-events.jsonl --print-run-dir 2>&1
```

If a mock cassette is missing and this fails, document the failure reason and proceed to Step 4. If it succeeds, verify in `/tmp/harness-e2e-events.jsonl`:
- The agent name is `custom-test`
- The model ref is `umans-ai-coding-plan/umans-glm-5.2`

**Step 4: Offline stress harness passes**

```bash
scripts/stress-harness.sh --mode offline --config /tmp/harness-e2e-test/harness.jsonc 2>&1 | tee /tmp/harness-e2e-stress.txt
```

Verify the stress harness completes without config-related failures.

**Step 5: Discovery last-wins works at runtime**

Create a second markdown file at `/tmp/harness-e2e-test/.agent-harness/agents/build.md` with a different model override. Run `doctor --json` again and verify the project-level `build.md` overrides the shipped `build` agent.

**Step 6: Disabled agent is hidden**

Add `"disable": true` to the `custom-test.md` frontmatter. Run `doctor --json` and verify the `custom-test` agent does NOT appear in the catalog. Then remove the `disable` field.

#### E2E Evidence Requirements

The implementer must provide:
1. The test workspace directory listing
2. The contents of `custom-test.md` and `harness.jsonc`
3. Actual stdout from each Step 1-6 command
4. For Step 1: a mapping showing each frontmatter field → doctor JSON output field, proving the value came from markdown (not JSON config)
5. For Step 3 (if successful): the relevant lines from `events.jsonl` showing agent name and model
6. For Step 5: doctor JSON showing the override took effect
7. For Step 6: doctor JSON showing the agent is hidden

**If any E2E step fails, the implementation is NOT complete.** A unit test passing but the runtime not reflecting the change means the merge function or discovery is still broken.

---

## 7. Anti-Gaming Rules

These rules are STRICTLY ENFORCED. Violations invalidate the work.

### 7.1 Forbidden Behaviors

1. **NO type suppression** — `as any`, `unwrap()`, `panic!`, `todo!`, `unreachable!`, `expect()` in production code. Use proper error handling with `Result` and `Option`. Tests may use `unwrap()` / `expect()`.

2. **NO deleting or weakening tests** — if a test fails after your change, FIX THE CODE, not the test. The only exception is when the test was testing wrong behavior (e.g., testing that a bug exists). In that case, replace the test with one that tests correct behavior AND explain why the old test was wrong.

3. **NO skipping verification** — every verification command must be actually run. Do not claim a command passes without showing its output. Do not skip a command because "it's the same as the previous one."

4. **NO fake tests** — tests must have real assertions that test real behavior. A test that just calls a function without asserting anything is not a test. A test that asserts `true == true` is not a test. A test that catches all errors and passes is not a test.

5. **NO circular reasoning** — "the code is correct because the test passes, and the test is correct because the code works" is circular. Tests must independently verify behavior.

6. **NO over-scoping** — do not add changes beyond what the task requires. If you find a bug unrelated to the task, note it but do not fix it in the same change.

7. **NO under-scoping** — do not skip hard parts. If a task requires changing 10 fields, change all 10. Do not change 5 and claim the rest are "out of scope."

8. **NO commenting out code** — if code needs to be removed, remove it. Commented-out code is dead code.

9. **NO `dbg!` or `println!` in production code** — use proper logging. The workspace lints deny `dbg_macro`.

10. **NO `unsafe` code** — the workspace lints deny `unsafe_code`.

11. **NO modifying shipped agent markdown files** (`.agent-harness/agents/*.md`) — these are prompt assets, not config test fixtures. If you need test fixtures, create them in the test directory.

12. **NO moving shipped agent defaults from Rust to markdown** — `default_shipped_agents()` in `agents.rs` must stay in Rust. Hidden system agents (`title`, `summary`, `compaction`) must stay compiled-in.

13. **NO renaming top-level config keys** — `agent` stays `agent`, not `agents`. `provider` stays `provider`, not `providers`. `permission` stays `permission`, not `permissions`.

14. **NO YAML frontmatter** — all frontmatter uses JSON5 (the existing format). Do not switch to YAML.

15. **NO creating new config files** — do not create `agents.jsonc` or any other new config file. Enrich existing markdown files, don't create new config surfaces.

### 7.2 Required Behaviors

1. **TDD** — write failing tests FIRST, then implement, then verify tests pass
2. **Read before write** — read the actual file before editing it. Do not edit from memory.
3. **Verify after every change** — run the task-specific verification command after each change
4. **Show evidence** — provide actual command output, not claims
5. **Reference Opencode** — read the relevant Opencode source file before each task
6. **Follow repo conventions** — read `AGENTS.md` files, follow lint rules, match code style
7. **Atomic commits** — each task is one commit, buildable and testable independently
8. **Update together** — follow the UPDATE TOGETHER table in root `AGENTS.md`

### 7.3 Self-Check Questions

Before declaring a task complete, answer these questions honestly:

1. Did I run the verification command and show its actual output?
2. Did I write tests that assert real behavior (not just "doesn't panic")?
3. Did I check that existing tests still pass?
4. Did I read the relevant Opencode source file?
5. Did I avoid all forbidden behaviors listed above?
6. Did I stay within scope (no over-scoping, no under-scoping)?
7. Did I provide a diff showing exactly what changed?
8. Would a skeptical reviewer be convinced by my evidence?

If any answer is "no," the task is not complete.

---

## 8. Implementer Freedom

The implementer has freedom to make decisions in these areas:

### 8.1 Test Structure
- Choose test names and test organization (inline mod tests vs separate test file)
- Choose test fixtures and test data
- Choose whether to use `#[test]` or `#[rstest]` or other test frameworks
- Choose assertion style (`assert_eq!` vs `assert!` vs `pretty_assertions`)

### 8.2 Implementation Details
- Choose exact implementation patterns (e.g., how to detect "default" model_ref for fallback)
- Choose whether to extract helper functions or inline
- Choose error message wording (within the style of existing error messages)
- Choose variable names (within Rust naming conventions)

### 8.3 Documentation Style
- Choose section organization within `docs/config.md`
- Choose example wording and tone
- Choose whether to use code blocks, tables, or prose

### 8.4 Commit Details
- Choose commit message wording (within the repo's style: `type(scope): description`)
- Choose whether to squash or keep separate commits within a task
- Choose branch name (if working on a branch)

### 8.5 Test File Location
- Choose whether to add tests inline in `discovery.rs` or in a separate `discovery_test.rs`
- Choose whether to add tests to existing test files or create new ones

### 8.6 Approach to model_ref Fallback
- The spec says `model_ref` is a `String` (not `Option`), so it always has a value
- The implementer may choose to: add a `model_ref_from_markdown: bool` flag, check for `"default:default"`, use a sentinel value, or restructure to use `Option<String>` internally
- Any approach is acceptable as long as the behavior is correct and tests verify it

---

## 9. Task Dependency Graph

```
Wave 1 (parallel — no dependencies):
├── Task 1: Fix merge function field precedence
├── Task 2: Fix discovery first-wins → last-wins
├── Task 3: Add bash alias to ProfilePermissions::shell
├── Task 5: Remove InertCompatibility keys from schema
└── Task 6: Document variable substitution and config layering

Wave 2 (after Wave 1):
└── Task 4: Align MarkdownAgentFrontmatter with PublicAgentConfig
    (depends on Task 1 — merge function must be fixed first)

Wave 3 (after Wave 2):
└── Task 7: Update drift tests and final verification
    (depends on all previous tasks)
```

**Critical path:** Task 1 → Task 4 → Task 7

---

## 10. Success Criteria

The implementation is complete when ALL of the following are true:

1. **Merge function fixed** — markdown frontmatter fields take effect when no JSON config override exists; JSON config precedence is preserved when both specify the same field. Verified by tests that check each of the 8 previously-dropped fields.

2. **Discovery last-wins** — project-level markdown overrides shipped agents with the same name. Verified by a test that creates two markdown files with the same name in different dirs.

3. **bash alias works** — both `shell` and `bash` field names work in markdown frontmatter permissions. Verified by a test that parses `bash: "allow"` in frontmatter.

4. **Frontmatter aligned** — `MarkdownAgentFrontmatter` has `enable`/`disable`/`use_small_model` and supports `PublicAgentTools` Map shape. Verified by tests for each new field.

5. **Schema cleaned** — InertCompatibility keys no longer appear in generated JSON schema. Verified by a test that checks schema properties.

6. **Documentation complete** — variable substitution and config layering are documented in `docs/config.md` and example config. Verified by drift test.

7. **All tests pass** — `cargo test -p harness-core`, `cargo test -p harness --test config_docs_reference_test`, `cargo test -p harness --test config_schema_cli_test`, `cargo test -p harness --test bootstrap_profiles_test`, `scripts/test-lanes.sh fast`, `scripts/test-lanes.sh quality-gates`.

8. **CLI verification** — `cargo run -p harness -- --config configs/harness.example.jsonc config validate` and `cargo run -p harness -- --config configs/harness.example.jsonc doctor --json` both pass.

9. **No security regression** — Plan agent's `PermissionRuleSet` remains in Rust code; hidden system agents remain compiled-in defaults.

10. **No breaking changes** — all existing configs continue to work without modification.

11. **End-to-end runtime verification** — a custom agent defined in markdown frontmatter (with model, variant, temperature, permissions, tools, max_iters, tool_failure_mode) is correctly loaded and reported by `harness doctor --json`. Discovery last-wins works at runtime (project markdown overrides shipped). Disabled agents are hidden. Verified per Section 6.4 with actual command output.

---

## 11. Commit Strategy

Each task is one atomic commit. Commits are buildable and testable independently.

| Commit | Task | Message format |
|--------|------|----------------|
| 1 | Task 1 | `fix(config): markdown frontmatter fields take effect when no JSON override` |
| 2 | Task 2 | `fix(config): discovery last-wins so project markdown overrides shipped agents` |
| 3 | Task 3 | `fix(config): accept bash alias in ProfilePermissions for markdown frontmatter` |
| 4 | Task 4 | `feat(config): align MarkdownAgentFrontmatter with PublicAgentConfig fields` |
| 5 | Task 5 | `chore(config): remove InertCompatibility keys from generated JSON schema` |
| 6 | Task 6 | `docs(config): document variable substitution and config layering` |
| 7 | Task 7 | `test(config): update drift tests for frontmatter enrichment and schema cleanup` |

**Do NOT commit until verification passes.** Each commit must be in a state where all tests pass.

---

## 12. File Change Summary

| File | Tasks | Type of change |
|------|-------|---------------|
| `crates/harness-core/src/config/discovery.rs` | 1, 2, 4 | Fix merge function, fix discovery, align frontmatter struct |
| `crates/harness-core/src/config.rs` | 3, 4 | Add bash alias, add enabled field to ProfileConfig |
| `crates/harness-core/src/config/public/contract.rs` | 5 | Remove InertCompatibility from schema |
| `crates/harness-core/src/config/public/agents.rs` | 4 | Import PublicAgentTools (if needed) |
| `docs/config.md` | 6 | Add variable substitution and config layering sections |
| `configs/harness.example.jsonc` | 6 | Add variable substitution examples in comments |
| `crates/harness/tests/config_docs_reference_test.rs` | 7 | Update drift tests |
| `crates/harness/tests/config_schema_cli_test.rs` | 7 | Update schema tests |
| `configs/config.json` | 7 | Regenerate schema (if needed) |

---

## 13. Reference: Key File Locations

| File | Purpose |
|------|---------|
| `crates/harness-core/src/config/discovery.rs` | Markdown discovery, frontmatter parsing, merge function |
| `crates/harness-core/src/config.rs` | Internal config types (ProfileConfig, ProfilePermissions, etc.) |
| `crates/harness-core/src/config/public.rs` | Public config types (PublicProfilePermissions, etc.) |
| `crates/harness-core/src/config/public/agents.rs` | Public agent types (PublicAgentConfig, PublicAgentMap, etc.) |
| `crates/harness-core/src/config/public/contract.rs` | Top-level config keys, aliases, schema contract |
| `crates/harness-core/src/config/loader.rs` | Config loading, variable substitution |
| `crates/harness-core/src/config/validation.rs` | Config validation |
| `crates/harness-core/src/config/aliases.rs` | Alias conflict detection |
| `crates/harness-core/src/perm.rs` | Permission evaluation (selectors, rules) |
| `crates/harness-core/src/agent_catalog.rs` | Agent catalog (runtime) |
| `crates/harness-core/AGENTS.md` | Core crate guidance |
| `configs/AGENTS.md` | Config directory guidance |
| `docs/config.md` | Public config documentation |
| `.agent-harness/AGENTS.md` | Runtime assets guidance |
| `inspirations/opencode/` | Opencode source code for reference |

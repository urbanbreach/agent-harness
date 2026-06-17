# Task: Bring Harness formatter to full OpenCode parity

## Context
- You are working in `/srv/samba/code/accela/agent-harness`, a Rust workspace.
- A partial OpenCode-parity formatter implementation already exists under `crates/harness-core/src/coord/formatter/`.
- **Goal**: make the formatter **behaviorally and functionally 1:1** with the OpenCode reference, not just structurally (config shape is already correct).
- Keep Harness invariants: coordinator-owned, replay-safe, non-fatal surfaced failures, default-on, workspace-path-safe.

## Required reading (read in this order, in full)
1. `inspirations/opencode/packages/opencode/src/format/formatter.ts` — every built-in formatter and its exact `enabled()` discovery rule.
2. `inspirations/opencode/packages/opencode/src/format/index.ts` — config merging, execution order, `$FILE` substitution, failure handling, `Format.status()`.
3. `crates/harness-core/src/coord/formatter/mod.rs` — current runner and resolver.
4. `crates/harness-core/src/coord/formatter/registry.rs` — current built-in registry (only 10 formatters).
5. `crates/harness-core/src/coord/formatter/discovery.rs` — current discovery trait (`which`-only).
6. `crates/harness-core/src/config.rs` — `FormatterConfig` / `FormatterOverride`.
7. `crates/harness-core/src/coord/task_lifecycle.rs` around line 390 — invocation site.
8. `docs/config.md` line 336 and `configs/harness.example.jsonc` lines 181-190 — public-facing docs/example.

## What "1:1 with OpenCode" means

### 1. Built-in registry must contain all 26 OpenCode formatters
Use the exact `name`, `extensions`, static default `command` vector, and built-in `environment` from `formatter.ts`. Every command must contain `$FILE`.

| name | extensions | environment | default command |
|---|---|---|---|
| `rustfmt` | `.rs` | — | `["rustfmt", "$FILE"]` |
| `gofmt` | `.go` | — | `["gofmt", "-w", "$FILE"]` |
| `mix` | `.ex`, `.exs`, `.eex`, `.heex`, `.leex`, `.neex`, `.sface` | — | `["mix", "format", "$FILE"]` |
| `prettier` | `.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`, `.mts`, `.cts`, `.html`, `.htm`, `.css`, `.scss`, `.sass`, `.less`, `.vue`, `.svelte`, `.json`, `.jsonc`, `.yaml`, `.yml`, `.toml`, `.xml`, `.md`, `.mdx`, `.graphql`, `.gql` | `BUN_BE_BUN=1` | `["prettier", "--write", "$FILE"]` |
| `oxfmt` | `.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`, `.mts`, `.cts` | `BUN_BE_BUN=1` | `["oxfmt", "$FILE"]` |
| `biome` | same large set as prettier | `BUN_BE_BUN=1` | `["biome", "format", "--write", "$FILE"]` |
| `zig` | `.zig`, `.zon` | — | `["zig", "fmt", "$FILE"]` |
| `clang-format` | `.c`, `.cc`, `.cpp`, `.cxx`, `.c++`, `.h`, `.hh`, `.hpp`, `.hxx`, `.h++`, `.ino`, `.C`, `.H` | — | `["clang-format", "-i", "$FILE"]` |
| `ktlint` | `.kt`, `.kts` | — | `["ktlint", "-F", "$FILE"]` |
| `ruff` | `.py`, `.pyi` | — | `["ruff", "format", "$FILE"]` |
| `air` | `.R` | — | `["air", "format", "$FILE"]` |
| `uvformat` | `.py`, `.pyi` | — | `["uv", "format", "--", "$FILE"]` |
| `rubocop` | `.rb`, `.rake`, `.gemspec`, `.ru` | — | `["rubocop", "--autocorrect", "$FILE"]` |
| `standardrb` | `.rb`, `.rake`, `.gemspec`, `.ru` | — | `["standardrb", "--fix", "$FILE"]` |
| `htmlbeautifier` | `.erb`, `.html.erb` | — | `["htmlbeautifier", "$FILE"]` |
| `dart` | `.dart` | — | `["dart", "format", "$FILE"]` |
| `ocamlformat` | `.ml`, `.mli` | — | `["ocamlformat", "-i", "$FILE"]` |
| `terraform` | `.tf`, `.tfvars` | — | `["terraform", "fmt", "$FILE"]` |
| `latexindent` | `.tex` | — | `["latexindent", "-w", "-s", "$FILE"]` |
| `gleam` | `.gleam` | — | `["gleam", "format", "$FILE"]` |
| `shfmt` | `.sh`, `.bash` | — | `["shfmt", "-w", "$FILE"]` |
| `nixfmt` | `.nix` | — | `["nixfmt", "$FILE"]` |
| `pint` | `.php` | — | `["./vendor/bin/pint", "$FILE"]` |
| `ormolu` | `.hs` | — | `["ormolu", "-i", "$FILE"]` |
| `cljfmt` | `.clj`, `.cljs`, `.cljc`, `.edn` | — | `["cljfmt", "fix", "--quiet", "$FILE"]` |
| `dfmt` | `.d` | — | `["dfmt", "-i", "$FILE"]` |

### 2. Discovery must implement OpenCode's per-formatter `enabled()` rules
Refactor `FormatterDiscovery` so it returns the resolved command vector, not a boolean. Each built-in formatter's discovery logic must match `formatter.ts` exactly:

- **which-only** (`rustfmt`, `gofmt`, `mix`, `zig`, `ktlint`, `rubocop`, `standardrb`, `htmlbeautifier`, `dart`, `terraform`, `latexindent`, `gleam`, `shfmt`, `nixfmt`, `ormolu`, `cljfmt`, `dfmt`): resolve the binary with `which`. If found, return the default command with the **resolved absolute path** as the first element. If not found, return `None`.
- **prettier**: `findUp("package.json")` from the target file's directory up to the worktree root. For each found `package.json`, read it and check `dependencies.prettier` or `devDependencies.prettier`. If present, resolve the `prettier` binary (prefer local `node_modules/.bin/prettier`, fall back to `which("prettier")`). If resolved, return `["<resolved>", "--write", "$FILE"]`.
- **biome**: `findUp("biome.json")` or `findUp("biome.jsonc")`. If found, resolve `@biomejs/biome` (prefer local `node_modules/.bin/biome`, fall back to `which("biome")`). Return `["<resolved>", "format", "--write", "$FILE"]`.
- **oxfmt**: gated by `config.experimental_oxfmt`. If true, `findUp("package.json")` and check `dependencies.oxfmt` or `devDependencies.oxfmt`. Resolve `oxfmt` locally or via `which`. Return `["<resolved>", "$FILE"]`.
- **clang-format**: `findUp(".clang-format")`. If found, `which("clang-format")`. Return `["<resolved>", "-i", "$FILE"]`.
- **ruff**: `which("ruff")` must pass. Then `findUp` one of `pyproject.toml`, `ruff.toml`, `.ruff.toml`. For `pyproject.toml`, also require the file contains `[tool.ruff]`. If no config found, fallback: `findUp` one of `requirements.txt`, `pyproject.toml`, `Pipfile` and require the file content contains `"ruff"`. If any match, return `["ruff", "format", "$FILE"]`.
- **uvformat**: if `ruff` would be enabled for this context, return `None`. Otherwise `which("uv")`, run `uv format --help`, and if exit code is 0 return `["uv", "format", "--", "$FILE"]`.
- **ocamlformat**: `which("ocamlformat")` and `findUp(".ocamlformat")`. Return `["ocamlformat", "-i", "$FILE"]`.
- **pint**: `findUp("composer.json")`; read it; check `require["laravel/pint"]` or `require-dev["laravel/pint"]`. If present, return `["./vendor/bin/pint", "$FILE"]`.
- **air** (`rlang`): `which("air")`, run `air --help`, verify first line contains both `"R language"` and `"formatter"` and exit code 0. Return `["air", "format", "$FILE"]`.

`findUp(name, start_dir, worktree_root)` walks from `start_dir` up to `worktree_root` (inclusive), returning all matching file paths. Do not search above the worktree root.

### 3. Config merging must match OpenCode
- `formatter: true` → enable all built-ins.
- `formatter: false` → disable all.
- Object form → global `enabled` and `experimentalOxfmt` plus per-formatter overrides keyed by formatter **name**.
- Merge rules:
  - `disabled: true` removes the formatter.
  - `command` override replaces the discovered command entirely and bypasses discovery.
  - `environment` merges with the built-in environment; override keys win.
  - `extensions` override replaces the built-in extension list.
- Ruff/uv coupling: if either `ruff` or `uvformat` is disabled, skip **both**.
- Custom formatter names (not in the built-in registry) are allowed if they provide `command` and `extensions`.

### 4. Execution semantics must match OpenCode
- Determine the file extension with `Path::new(path).extension()` (include the leading dot when matching).
- Collect all formatters whose effective extension list matches.
- Run them in **built-in registry declaration order**, appending any custom override-only formatters at the end (preserve OpenCode's `Object.values(formatters)` order).
- For each formatter, call discovery to resolve its command. If discovery returns `None`/`false`, skip it silently.
- Replace the **first** `$FILE` occurrence in each command argument with the target file path.
- Execute with:
  - cwd = workspace root,
  - env = process env + built-in env + override env (later wins),
  - 30 second timeout,
  - piped stdout/stderr.
- If a formatter exits non-zero, collect stderr (or stdout fallback) into a non-fatal warning string. Continue running the remaining formatters.
- Keep existing workspace-path validation: canonicalize workspace root and target, reject escapes.

### 5. `Format.status()` surface (optional but required for full parity)
OpenCode exposes `Format.status() -> { name, extensions, enabled }[]`. Add a coordinator query that returns the resolved formatter list with name, extensions, and whether discovery resolved a command. Wire it through `crates/harness-core/src/coord/handle.rs` if trivial. If it expands scope too much, document the omission explicitly in your final evidence.

### 6. Tests you must add or update
Do not weaken or delete existing tests. Add tests for every discovery rule and every edge case:

1. Built-in discovery: each of prettier/biome/ruff/uvformat/ocamlformat/clang-format/pint/air selects only when its project marker is present.
2. which-only formatters are selected only when binary is on PATH.
3. Multi-formatter run: two formatters match the same extension and run in registry order.
4. `$FILE` substitution works and falls back to appending the path when no placeholder is present.
5. Override command bypasses discovery and surfaces failures non-fatally.
6. Disable by name works, including ruff/uv coupling.
7. Config scalar `true`/`false` work.
8. Environment merge: built-in + override env present, override wins.
9. Extension override replaces the built-in extension list.
10. Path escape is rejected.
11. Custom formatter name with `command` + `extensions` runs.
12. `Format.status()` returns the resolved list (if implemented).

### 7. Files you will likely touch
- `crates/harness-core/src/coord/formatter/registry.rs` — add all 26 built-ins.
- `crates/harness-core/src/coord/formatter/discovery.rs` — refactor trait to return resolved commands; implement all discovery rules.
- `crates/harness-core/src/coord/formatter/mod.rs` — update resolver/runner to use returned commands, registry order, and environment merge.
- `crates/harness-core/src/coord/tests/formatter_discovery_tests.rs`, `formatter_execution_tests.rs` — add tests.
- `crates/harness-core/src/coord/tests.rs` — delegate new tests.
- `crates/harness-core/src/coord/handle.rs` — if adding status surface.
- `docs/config.md`, `configs/harness.example.jsonc`, `configs/config.json` — update public docs and regenerate schema.

### 8. Verification commands (must all pass)
```bash
cargo test -p harness-core formatter
cargo test -p harness-core --test coord_test
cargo test -p harness --test config_docs_reference_test
cargo test -p harness --test config_schema_cli_test
cargo run -p harness -- --config configs/harness.example.jsonc config validate
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/test-lanes.sh fast
```

### 9. Hard constraints
- No `unwrap`/`expect` outside tests.
- No `as any` / type suppression.
- Do not store raw formatter output or secrets in events.
- Do not rewrite `events.jsonl`.
- Do not weaken or `#[ignore]` existing tests.
- Do not hand-edit `configs/config.json`; regenerate via the Rust schema path.
- Keep files under 250 pure LOC; split if a file grows past the limit.

Deliver the fully 1:1 implementation. Do not stop at a partial or "good enough" subset.

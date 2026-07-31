# Harness / Grok Build Parity Loop Contract

> **Binding use:** This is the implementation contract for the next autonomous
> parity loop. It replaces completion claims, stale rollups, and the earlier
> `docs/grok-build-tui-implementation-prompt.md` contract when they conflict
> with observed behavior.
>
> **Stop condition:** Stop only when every required product surface, capability,
> action, journey, provider path, configuration path, and evidence gate passes
> on one identified revision, or when the user has explicitly approved the
> exact named divergence. A registry entry, test fixture, status banner, mock,
> unavailable result, or copied artifact is never completion.

## 1. Mission

Deliver a Harness-native implementation that is behaviorally and visually
parity-complete with the pinned local Grok Build reference across:

- the interactive Ratatui shell, overlays, input modes, themes, responsive
  states, keyboard and mouse actions, timing, and animation;
- CLI commands, output modes, agent handoffs, server transports, updates,
  sessions, worktrees, integrations, and operator workflows;
- coordinator-owned provider, authentication, tool, permission, persistence,
  replay, scheduling, plugin, workspace, and recovery behavior;
- configuration, schema, discovery, layering, migration, diagnostics, and
  redaction;
- real live-provider execution and agent-controlled offline dogfood journeys;
- semantic terminal cells, PTY traces, settled pixels, timing, side effects,
  rollback, cancellation, restart, and recovery evidence.

The result must remain a Harness product. Preserve Harness event authority,
permission-before-execution, replay purity, redaction, runtime/TUI config
separation, and append-only event semantics. Do not copy reference source,
tests, fixtures, architecture, identifiers, or harnesses.

## 2. Authority and reference access

### 2.1 Pinned reference executable

The only authorized executable reference is:

```text
inspirations/grok-build/target/debug/xai-grok-pager
sha256: 883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5
version: grok 0.1.220-alpha.4 (c1b5909) [stable]
reference revision: c1b5909ec707c069f1d21a93917af044e71da0d7
```

Before reference execution, verify the executable bit, digest, version, and
reference revision. Never search for, install, download, rebuild, or substitute
another executable. Never modify anything below `inspirations/grok-build`.

### 2.2 Required reference repository inspection

The user explicitly requires inspection of `inspirations/grok-build` contents.
The next loop **must** inspect the reference source and public documentation,
not only run the binary. Inspect the relevant files under these areas before
claiming a corresponding Harness row complete:

```text
inspirations/grok-build/crates/codegen/xai-grok-pager/
inspirations/grok-build/crates/codegen/xai-grok-shell/
inspirations/grok-build/crates/codegen/xai-grok-tools/
inspirations/grok-build/crates/codegen/xai-grok-config/
inspirations/grok-build/crates/codegen/xai-grok-pager-render/
inspirations/grok-build/crates/codegen/xai-grok-auth/
inspirations/grok-build/crates/codegen/xai-grok-mcp/
inspirations/grok-build/crates/codegen/xai-grok-agent/
inspirations/grok-build/crates/codegen/xai-grok-plugin-marketplace/
inspirations/grok-build/crates/codegen/xai-acp-lib/
inspirations/grok-build/crates/codegen/xai-grok-workspace/
inspirations/grok-build/crates/codegen/xai-grok-update/
inspirations/grok-build/crates/codegen/xai-grok-shell-session-support/
inspirations/grok-build/crates/codegen/xai-fast-worktree/
inspirations/grok-build/crates/codegen/xai-grok-sandbox/
inspirations/grok-build/crates/codegen/xai-prompt-queue/
inspirations/grok-build/crates/codegen/xai-codebase-graph/
inspirations/grok-build/crates/codegen/xai-tty-utils/
inspirations/grok-build/crates/codegen/xai-grok-pager/docs/user-guide/
```

Also inspect the reference workspace manifests and user-guide material needed
to enumerate public behavior. Source inspection is for an independent
behavioral crosswalk only. It must never become copied implementation or copied
test logic.

The reference source is authoritative for **what exists and how public
surfaces are organized**. The reference executable is authoritative for
the executable for acceptance until the discrepancy is resolved.

## 3. Audited starting truth

The previous loop's green claims are rejected as a completion baseline. The
following findings are mandatory work, not optional follow-up:

1. `AnthropicMessages` is marked supported in
   `crates/harness-core/src/provider_protocol.rs`, but the public provider
   config and `build_provider()` path only wire OpenAI-compatible providers.
   A protocol catalog row is not provider support.
2. The capability inventory marks all 85 rows `pass`, while at least 60 rows
   either lack a visible public action or admit residual missing behavior.
   Rows must be split or demoted until their real operation is proven.
3. `workspace_hub`, `mcp_oauth`, and `browser_oidc` currently expose
   availability-style results that are not proof of connectivity or an actual
   user workflow. Hardcoded example endpoints and unconditional `Available`
   states are not implementations.
4. Binary update currently checks a local manifest but does not complete the
   reference's download, apply, restart, and recovery workflow.
5. ACP stdio transport exists without the required public CLI server surface.
6. The CLI lacks reference-visible surfaces including dashboard, share/export,
   trace, best-of-n, check-loop, JSON schema output, streaming JSON, and the
   required approval/session/operator flags. The exact final list must come
   from the reference crosswalk, not this list alone.
7. The TUI lacks or has not proven reference parity for plan mode, vim mode,
   minimal mode, dashboard, voice, inline media, welcome flow, extensions,
   file completion/search, slash completion, notifications, tips, and the
   complete theme/system mode behavior.
8. Existing signoff paths can copy or reuse prior evidence and can report
   scaffolding as a journey. A copied artifact is stale even when its bytes
   are valid and a lane exits zero.
9. Live smoke currently proves only a minimal response path and may invoke a
   build command rather than the explicitly selected installed Harness binary.
   It does not prove a live provider/tool/permission/recovery matrix.
10. Hardcoded availability, local-only transports, mock success, diagnostic
    probes, and structured-unavailable results must be classified as incomplete
    unless the public contract explicitly requires unavailable behavior.

Refresh every path and status against the current checkout before using these
findings as evidence. Do not preserve historical pass counts.

## 4. Required status model

Every row in every machine-readable inventory uses exactly one status:

```text
incomplete | blocked | pass | diverged
```

- `incomplete`: required behavior or required evidence is missing.
- `blocked`: a named external dependency prevents execution; include the exact
  command, environment requirement, and owner. Blocked is never pass.
- `pass`: every applicable behavioral, visual, timing, side-effect, live,
  dogfood, and independent-review gate is present for the current revision.
- `diverged`: only the exact user-approved divergence ID may authorize this
  status. “Evidence-backed divergence” is not an approval.

The validator must reject:

- missing or unauthorized divergence IDs;
- empty evidence layers or missing owner paths;
- stale source revisions, stale manifest/reference/environment digests;
- copied artifacts, artifacts from another evidence root, or artifacts whose
  command metadata does not match the current run;
- a pass row with no visible/public action when one is required;
- protocol/config claims that do not reach a usable runtime path;
- a success result that is only a registry, probe, fixture, mock, banner,
  diagnostic, or structured-unavailable result.

## 5. Evidence contract

### 5.1 Freshness and provenance

Each signoff run creates a new evidence root. It must record:

- Harness source revision, worktree status, binary path, binary SHA-256, and
  binary `--version` output;
- reference path, SHA-256, version, and reference revision;
- manifest and scenario digests;
- OS, terminal emulator/parser, renderer, Chromium/xterm.js where used,
  fonts, locale, Unicode width, color mode, viewport, DPR, and terminal
  capability flags;
- provider name/model, auth mode, endpoint class, tool policy, permission
  policy, and redacted request metadata;
- exact commands, environment variable names without secret values, start/end
  times, exit status, timeouts, and artifact hashes;
- whether the run was offline mock, controlled local fake, live provider, or
  installed-binary dogfood.

The lane must generate files into the new root. It may not copy artifacts from
an older root, gitignored backup, target directory, or reference capture. A
previous artifact can be an input oracle only when explicitly named and hashed;
it cannot be the current run's output.

### 5.2 Required evidence layers

Use the following layers where applicable:

```text
L0  inventory/reference crosswalk and row ownership
L1  state/action and semantic terminal-cell result
L2  compiled public operation and external postcondition
L3  PTY/input trace, error, cancellation, restart, and recovery result
L4  settled pixel and fixed-tick animation comparison
L5  live-provider and agent-controlled dogfood result
L6  independent review and undisclosed holdout result
```

Missing applicable layers make the row incomplete. Expected outputs must come
from the frozen reference or an explicitly documented Harness invariant, never
from the Harness output compared with itself.

### 5.3 Visual and timing proof

For each visual row, capture full semantic cells including grapheme, width and
continuation, foreground, background, modifiers, hyperlinks, cursor, focus,
selection, dimensions, alternate-screen state, scrolling, wrapping, mouse,
paste, and enhanced-key behavior as applicable.

Capture full-frame settled images at the same viewport, font, renderer, DPR,
locale, and theme. Compare fixed canonical animation ticks separately from
settled frames. Settled means scripted external events are complete and three
consecutive semantic-cell ticks are unchanged. A timeout is failure, not a
reason to skip a frame. Masks are forbidden by default and may cover only an
exact identity field after independent review; never geometry, spacing, icons,
focus, cursor, color, or whole components.

Required viewport set includes `120x50`, `120x40`, `100x30`, `80x24`, `79x24`,
`60x20`, a width above 120, and the reference's observed extremes. Exercise
truecolor/reduced-color, enhanced/legacy keys, mouse, clipboard, Unicode,
long content, resizing, scrolling, and reduced-capability fallbacks.

## 6. Actual installed Harness and live-provider proof

### 6.1 Installed binary requirement

All final dogfood and PTY acceptance must invoke an explicit `HARNESS_BIN`.
The lane must fail if it is absent, not executable, outside the intended
workspace, or missing a recorded digest/version. `cargo run` is acceptable for
development checks but is not the final installed-binary proof. Record the
exact binary path and invoke that path directly.

The selected binary must run `--help`, `--version`, configuration validation,
doctor, one offline mock journey, and the live/dogfood journeys below. The lane
must capture stdout, stderr, exit codes, event artifacts, workspace status, and
external side effects.

### 6.2 Live provider matrix

For every provider that the public catalog calls supported, run the actual
configured path against a live provider or controlled provider proxy that
exercises the real transport and authentication boundary. The matrix includes:

- configuration discovery and effective provider selection;
- authentication success, missing credential, expired credential, and refresh
  or re-auth behavior;
- streaming text and tool-call response handling;
- permission approval and denial around a mutating tool;
- malformed/error response recovery;
- cancellation and interruption;
- session persistence and resume after a completed turn;
- redaction proof for persisted events and artifacts.

If a provider cannot be exercised, it is `blocked`, not `pass`. If a protocol
adapter is not config-reachable, remove its supported claim or implement the
complete public path before claiming support. In particular, do not claim
Anthropic support merely because an Anthropic transport module or enum exists.

Live evidence must never store API keys, auth headers, cookies, PEM material,
raw credentials, hidden reasoning, or unredacted provider payloads.

### 6.3 Agent-controlled dogfood journeys

Run the installed binary through actual Harness agent behavior, not a script
that injects final state or manually writes expected artifacts. At minimum,
the agent must complete these journeys in an isolated temporary workspace:

1. inspect a file, make a requested edit, run a verification command, and leave
   a persisted event/session record;
2. request a permissioned mutation, deny it, then recover with a safe action;
3. start a tool-enabled turn, cancel or interrupt it, and resume/recover;
4. create/select/use/clean up an isolated worktree without touching the source
   checkout;
5. resume or fork a saved session and prove the replay-derived state;
6. exercise one configured integration or MCP tool through its real boundary;
7. use a configuration/settings journey and prove effective value, source
   attribution, redaction, and persistence;
8. exercise a provider error or unavailable dependency and prove truthful,
   recoverable behavior.

Each journey needs an agent-visible prompt, actual tool calls, event evidence,
external postconditions, failure-path evidence, and before/after workspace
status. Seeded probes, synthetic destination `AppState`, direct event injection,
fixture-only success, or a status banner cannot be the sole proof.

## 7. Complete parity inventory

The implementer must build and maintain a crosswalk with one row per reference
public surface. The following families are mandatory starting scope; source and
binary inspection may add rows but may not remove them.

### 7.1 TUI and interaction

Cover startup/welcome, shell chrome, transcript blocks, markdown, syntax,
diffs, tools, composer, focus, cursor, selection, scrolling, resize, mouse,
permissions, questions, command palette, slash completion, model switching,
session picker/tree/fork/clone/rename, overlays, notifications, tips,
extensions/plugins, file search and `@` completion, inline media, dashboard,
plan mode, agent mode, vim mode, minimal mode, voice affordances, clipboard,
terminal hyperlinks, themes, auto/system theme selection, reduced-capability
fallbacks, and every visible shortcut.

The theme crosswalk must include all reference named themes and system/auto
behavior. A palette or glyph substitution is not a theme implementation.

### 7.2 CLI and operator surface

Compare `--help`, subcommand help, flags, exit codes, text/JSON/streaming JSON
schemas, errors, and side effects. At minimum inspect and account for:

```text
run, prompt, auth, doctor, config, sessions, update, agent stdio,
dashboard, share/export, trace, check, best-of-n, model/provider selection,
session fork/resume, minimal/fullscreen, approval/yolo, web/tool restrictions,
schema output, and streaming output.
```

Do not add a command solely to make help text match. Every advertised command
must perform its described operation and have a meaningful failure path.

### 7.3 Runtime, integrations, and persistence

Crosswalk and prove worktrees, workspace trust, sandbox profiles, supported VCS
flows, edit attribution, sessions, rewind, memory, foreign import, queued and
interjected input, cron execution, background tasks, teams/mailboxes,
plugins/marketplace lifecycle, hooks, ACP, MCP OAuth and remote transports,
workspace hub, browser/device/enterprise auth, credential refresh, non-OpenAI
provider protocols, binary update apply/restart, persistent code graph,
clipboard/hyperlinks, settings registry, schema generation, migrations,
redaction, export, crash recovery, and cleanup.

Every row must point to the real owner. A bookkeeping module is not the owner
of an integration, and a diagnostic result is not the capability.

## 8. Ordered implementation loop

Keep exactly one implementation packet active. Work in this order:

### Wave 0: establish truth and purity

- Freeze current branch, HEAD, worktree status, toolchain, and explicit binary
  paths. Preserve unrelated changes.
- Remove synthetic production startup probes and any write-capable replay path.
- Make replay root-explicit and side-effect free from a different process CWD.
- Run PTY and dogfood from temporary isolated workspaces with before/after
  checkout assertions.
- Remove duplicate test registration and all ordinary-test dependencies on
  pre-existing gitignored evidence.
- Fix current branding/config test failures without weakening assertions.

Exit only when startup, replay, and PTY tests leave the source checkout
byte-identical and the clean-checkout baseline is reproducible.

### Wave 1: repair evidence and manifests

- Replace permissive status semantics with the model in §4.
- Re-audit every capability and subsystem row; split rows with multiple
  operations or residual notes.
- Make signoff generate a fresh evidence root and validate every digest,
  command, scenario, environment, artifact, and review receipt.
- Delete copy/reuse paths and any `|| true`, skipped assertion, or self-comparison
  in parity-signoff lanes.
- Correct the checked-in manifest to reflect actual evidence, not target counts.

Exit only when a deliberately stale, copied, missing, wrong-revision, wrong-
viewport, or self-generated artifact causes the strict lane to fail.

### Wave 2: implement real backend and CLI capability rows

Process in dependency order: workspace/worktree/sandbox/trust/VCS; sessions and
rewind; scheduling/orchestration; plugins/ACP/MCP/workspace hub/hooks;
authentication/providers/updates; code graph/terminal/settings; then every
remaining inventory row. For every row:

1. inspect the reference source/docs and execute the pinned binary;
2. write an independently authored failing owner regression;
3. implement the real coordinator/provider/tool/config owner;
4. wire the public CLI/TUI/tool/protocol action;
5. prove success, invalid input, denial, cancellation, restart, rollback, and
   recovery where applicable;
6. drive the compiled surface and record external postconditions;
7. update the row only after fresh evidence exists.

### Wave 3: rebuild the TUI as complete behavior

Measure and freeze the reference shell, then implement complete state and
interaction parity. Do not stop at topology, color, punctuation, glyph, or
placement changes. Every visible control must route through the TUI action,
one mutating intent at most, CLI/coordinator authority, event projection, and
rendered result.

Exercise every focus state, overlay, mode, theme, viewport, terminal
capability, animation tick, error state, and recovery state in the crosswalk.

### Wave 4: live provider and dogfood closure

Run the §6 provider matrix and §6.3 agent-controlled journeys with the explicit
installed binary. Keep unavailable external dependencies blocked. Do not convert
an offline mock, local proxy, or doctor result into live proof.

### Wave 5: same-revision acceptance

From a dedicated clean worktree and a new evidence root, run the full relevant
workspace tests, quality gates, deterministic lanes, PTY lane, strict parity
lane, live-provider matrix, and dogfood journeys. Validate that all outputs
refer to the same source revision and that the worktree remains clean.

Then obtain two independent reviews: one visual/interaction reviewer and one
runtime/evidence reviewer. Run undisclosed holdouts for Unicode width, long
content, resizing, scrolling, timing perturbations, permission denial, provider
failure, cancellation, restart, and cleanup. Reviewer disagreement blocks.

## 9. Required acceptance commands

The exact current repository commands may be expanded when a changed surface
requires another owner, but none of these may be silently omitted:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --all-targets --all-features --workspace -- -D warnings
scripts/test-lanes.sh fast
scripts/test-lanes.sh quality-gates
scripts/test-lanes.sh integration
scripts/test-lanes.sh all-deterministic
scripts/test-lanes.sh signoff-pty
bash scripts/harness-qa-dogfood.sh --self-test
```

Run targeted owners for every changed crate and every changed public surface.
Run the strict parity/signoff lane only with a fresh evidence root. Run live
provider and installed-binary dogfood with the explicit environment and
`HARNESS_BIN`; missing credentials or proxy configuration is a recorded block,
not a pass.

## 10. Final report and stop rule

The final report must include:

- exact source revision, binary paths/digests, reference receipt, and
  environment receipt;
- manifest digest, row counts by truthful status, and every divergence/block;
- commands and artifact paths for each evidence layer;
- live-provider matrix results, model/auth mode, and redaction scan result;
- installed-binary dogfood prompts, tool calls, external postconditions, and
  workspace before/after status;
- semantic-cell, PTY, pixel, animation, timing, responsive, review, and holdout
  results;
- known residual risks and why each is blocked or user-approved;
- exact wording of the completion claim.

Stop only if all of the following are true on the same current revision:

- every required reference row is present and passes all applicable evidence
  layers;
- every advertised action invokes the intended real operation through the
  compiled product;
- all supported provider claims are config-reachable and live-proven;
- agent-controlled dogfood journeys succeed and recover through the installed
  binary;
- no placeholder, hardcoded availability, local-only stand-in, mock-only
  success, wrong dispatch, or stale artifact remains in a required path;
- Harness coordinator, event, replay, permission, cancellation, redaction,
  persistence, and cleanup invariants remain green;
- semantic cells, settled pixels, traces, timing, and responsive behavior have
  no unapproved differences;
- two independent reviews and undisclosed holdouts pass;
- any remaining divergence is explicitly approved by the user by exact ID.

Otherwise continue the loop. A green build, a passing subset, a complete
registry, a polished reskin, a copied capture, an unavailable result, or an
agent-authored claim is not a stop condition.

# Changelog

Notable changes to Harness. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/). Add entries under `[Unreleased]`;
`scripts/release.sh` turns that section into the next release.

## [Unreleased]

The first public release.

### Fixed

- Update rustls and rand to patched dependency versions.
- SIGTERM and SIGHUP restore terminal modes and cancel active CLI work before
  exiting.
- Recover interrupted queued and preparing subagents as failed without rerunning
  their providers, tools or hooks.
- Keep finalized provider requests, reasoning and raw tool results in memory;
  persist semantic state for continuation after restart.
- Anthropic reasoning variants enable thinking, and Anthropic and Gemini keep
  signed continuation state across tool calls in memory, outside the journal.
  Provider-native compaction payloads are stored separately (see Added).
- Release archives and installed binaries include third-party license notices.
- Closing a login dialog cancels OAuth and releases its callback listener.
  Late results from a previous login cannot change a newly opened dialog.
- Claude Code runs in the selected agent workspace. Switching projects rebuilds
  the TUI's cached runtime configuration.
- Signing in refreshes model discovery for subagents in the current session as
  well as future sessions.
- Claude subscription sessions compact at the model's catalog window. Claude
  Code used to compact only at its own window, so the context meter could pass
  100% without any compaction. Its compactions now leave a transcript row
  instead of only a two-second toast.
- `cacheRetention: "long"` sends the documented extended-retention request to
  direct OpenAI endpoints.
- MCP requests respect `network` and `bash` transport permissions, including
  tools discovered during a session.
- Multi-file LSP renames synchronize approved file snapshots before computing
  edits, preventing stale offsets from overwriting editor changes.
- Deterministic live TUI sessions keep separate histories. Scenario resets
  refuse ordinary sessions and active writers.
- TUI `--profile` selects the requested profile. Removed the unused `--minimal`
  and `--fullscreen` flags.
- HTTP provider timeouts measure idle reads, allowing long responses that keep
  making progress. Responses cut short by an output limit report failure while
  preserving their partial text and usage.
- Image attachments keep their message owners when skill instructions are added.
- Google API keys now discover the Google OpenAI-compatible provider and model.
- Dropping the last coordinator handle cancels active work and releases the
  session writer after cleanup.
- Release validation follows SemVer ordering, including RC-to-stable releases.
  Failed release dry runs restore the files they changed.
- File edits preserve private permissions from the moment staging files are
  created.
- Project storage keys use a fixed-length path digest, preventing different
  projects from sharing data and supporting long project paths. Existing
  `--path--` directories remain intact; see
  [storage migration](docs/architecture/sessions-and-replay.md#legacy-storage).
  The plan picker shows filenames so storage keys cannot hide them.
- Support exports and session archives omit reasoning summaries and signatures
  from legacy provider-completion metadata.
- Concurrent credential managers and Harness processes share an OAuth refresh
  lock. Logout can still invalidate a refresh in progress.
- The live status timer keeps counting while the main agent waits on subagents.
  It used to restart whenever a subagent sent a new request, and after you
  opened a subagent and came back it followed that subagent's clock.
- Tool calls stay with the turn that made them after you steer. Subagent tool
  calls no longer pile up under your steering message in the main transcript.
- Subagents without a persona, role or `[tag]` in their description are named
  after their agent type, such as Task or Scout, instead of "Subagent".
- A Claude subscription counts as an OAuth sign-in. The start screen used to say
  "Logged in with API key" for it, and session cost dropped its `(sub)` marker.
- Steering moves a running eval cell to the background so the agent reads your
  message. A queued follow-up or a finished background subagent used to do this
  instead, while steering waited for the cell to end.
- Steering that misses its turn runs before follow-ups you queued after it.
- A message the run refuses, such as one over 1 MiB, goes back to the editor
  with the reason. It used to disappear, and the TUI then dropped every later
  prompt, interrupt and permission answer.
- Commands the agent runs no longer share your terminal. A credential prompt,
  such as git asking for a GitHub username, used to draw over the TUI and stall
  the turn until the command timed out. It now fails at once with an error the
  agent can read. Bash commands, hooks and code kernels also receive HOME, USER,
  LOGNAME and SSH_AUTH_SOCK, so git finds your identity and credential helpers.

### Added

- Context maintenance rebuilt as an ordered chain of methods: provider-native
  remote compaction, an image archive of older history for vision models, a
  handoff document written by the live model, shake (heavy output moved to a
  recoverable artifact) and a summary. `runtime.compaction.method_order` picks
  the order. Compaction now also runs after a final answer, between tool
  batches, after provider overflow (up to three passes), after output-limit
  stops and, when `idle_enabled` is set, after idle time. Automatic failures
  are warnings instead of failed turns, and a pass that frees too little pauses
  instead of looping.
- Background summaries start shortly before the threshold and commit instantly
  when it is crossed. Superseded reads and uneventful tool results are pruned
  before measuring.
- `/compact [remote|archive|soft] [focus]`, `/handoff [focus]` and
  `/shake [elide|images|thinking]`. A manual pass interrupts a running turn and
  resumes it afterwards.
- An animated compaction card shows what is being compacted, streams the
  summary as it is written, prints archive frames as a film strip and collapses
  the context bar when the pass commits. Archived turns recede behind a divider
  labelled with the method and the token savings. Reduced-motion settings keep a
  static card.
- The breadcrumb's context segment reads `42.0%/200K`, colours by pressure
  (yellow, purple, red), shows `⟲` while automatic compaction is on and draws a
  gauge with the compaction threshold and the background-summary start. It
  sheds the gauge, then `⟲`, as the row narrows.
- Optional branch summaries when rewinding a conversation
  (`runtime.compaction.branch_summary.enabled`).
- The settings overlay names the compaction rows (Auto-Compact, Compaction
  Threshold, Idle Compaction Delay and so on) and offers choice menus for the
  method order, percentage and token thresholds, idle limits, archive system
  prompt and archive shape. Every menu starts with a Default entry that removes
  the key, and a value outside the list stays selectable as `Custom: <value>`.
  Appearance gains Collapse Compacted History.
- Ctrl+T in the model switcher edits the compaction threshold of the selected
  model: `80%`, `f150000` (fixed tokens), `200K` (base window) or an empty line
  to reset. The editor saves to `runtime.compaction.model_thresholds`, refuses a
  fixed trigger above the model window, flags a base window at or above it and
  previews where compaction would start.
- The default compaction threshold is the window minus the larger of 15% and
  `reserve_tokens`, capped by the input budget. The old 45% to 80% tiers are gone.
  `split_oversized_turns`, `structured_summary_contract` and
  `suppress_auto_compaction` are no longer accepted.
- A response cut off by the output limit is kept with a warning when it
  delivered text or tool calls; `harness prompt` and `harness run` print turn
  warnings on stderr after the answer.
- Token counts come from each model's own vocabulary where Harness bundles one
  (OpenAI o200k, four Claude generations, Qwen3, DeepSeek, Kimi and GLM), counted
  offline by the new `harness-tokenizer` crate. Other models count at four bytes
  per token, or with o200k when `HARNESS_TOKENIZER_ACCURATE=1`. The vocabularies
  ship with their license notices.
- Summaries serialize the transcript in the tool-call dialect of the summarizing
  model's family (XML, Anthropic, Harmony, Gemini, Gemma, GLM, DeepSeek, Kimi,
  Qwen3 or MiniMax). Unknown models get XML.
- The archive method falls back to the bundled Silver font for text its bitmap
  fonts cannot draw, so CJK history is archived instead of refused. With
  `archive.shape` on `auto`, the shape follows the model and switches to
  `silver16-bw` for text that needs Silver.
- `archive.system_prompt` (`none`, `agents_md` or `all`) and
  `archive.tool_results` send the system prompt, the repository rules or large
  tool results to vision models as images in the outgoing request only; the
  stored context does not change. Tokens saved by tool-result swaps are
  appended to `inline-savings.jsonl` in the Harness home.
- A model that does not read archived reasoning gets a reasoning-free view of
  the archive, saved once as a `compaction_patched` journal record. The original
  archive stays readable when you switch back.
- Provider-native compaction payloads are saved as private (0600) artifacts named
  `native-<blake3>.bin` and referenced from the journal by digest. They never
  enter `events.jsonl`, support exports or trace archives. After a restart a
  payload is reused only for the same provider, model and API; otherwise resume
  falls back to the previous readable compaction.
- Automatic summaries retry under `runtime.compaction.retry` (`enabled`,
  `max_retries`, `base_delay_ms`) and move to the next model when the wait would
  pass 30 seconds. `summary_models` picks a summarizer per live model; after it
  the walk tries the live model, `@smol`, `@slow` and the largest-window model
  in the catalog.
- `context_promotion` switches to the larger model only when that model's
  provider has a usable credential. Otherwise compaction runs.
- A manual pass resumes the turn it interrupted only after it commits or finds
  nothing to compact. A second `/compact` or `/handoff` while one is running is
  rejected with `Compaction already in progress`.
- `handoff_save_to_disk` writes the redacted handoff of an automatic pass to
  `handoff-<UTC time>.md` in the session's artifacts directory.
- Compaction hooks can print one JSON object on stdout. `compaction_requested`
  can cancel the pass or supply the whole compaction, `compaction_summarizing`
  adds context lines, a replacement prompt and data to preserve, and
  `compaction_applied` reports the saved entry. A handler that fails or times out
  contributes nothing, and only a `critical` hook stops the pass.
- `remote_endpoint` summaries authenticate with the summarizing model's provider
  key when the endpoint path ends in `/chat/completions`; other endpoints get no
  credentials. `remote_enabled` turns provider-native compaction off as a whole,
  and `remote_models` overrides its lane, API, endpoint and model per selector.
- With `notes_windows` on, a full window rolls over instead of being summarized.
  The agent keeps up to 16 KiB of notes with the `context_notes` tool, starts a
  fresh window with `new_context` and can read the whole branch at
  `history://current/full` through `read` and `grep`.
- Pruning leaves skill output alone and shake leaves artifact recovery reads
  alone. `session_search` flags a search with no hits as useless so compaction
  can drop it. A provider rejection for request size or image count is told
  apart from token overflow.
- Compacted history collapses behind the latest summary.
  `collapse_compacted_history` in `tui.jsonc` turns that off. Ctrl+Alt+K
  (`toggle_collapsed_history`) toggles it and Ctrl+Alt+O
  (`toggle_compaction_details`) expands the focused summary. `interrupt_turn` can
  bind a key that interrupts a turn or compaction; Ctrl+C stays the built-in
  interrupt key.
- The transcript shows compaction start and end, skipped and failed passes, and
  runtime notices as rows. Slash commands that find nothing to do print a
  warning row instead of a status banner.
- The footer's auto-compaction icon, threshold marker and speculation marker
  follow the coordinator's per-agent status, including per-model thresholds and
  a running or armed background summary. Before the first status arrives the
  footer shows neither.
- A bare number in `model_thresholds` is a base window and in `agent_thresholds`
  a token count. Write percentages as `"80%"`; settings writes convert old
  values from 1 to 100.
- The breadcrumb above the transcript shows the main agent's latest prompt cache
  hit rate, such as `CH96.4%`, once it reaches 10%.
- `monitor` watches command output or a file in the background and wakes the
  agent with matching lines and the exit, with a live watching status in the
  footer and pause/rearm when updates get noisy.
- The transcript shows a card while the agent waits on background subagents or
  commands. Each job gets a row with its status, a duration that ticks every
  tenth of a second and a preview of its result. Click a subagent's row to open
  it. A wait that returns with every job still running gives way to the next
  wait, so repeated polls read as one card.
- Steering ends long waits early. A foreground subagent moves to the
  background, `wait_commands_or_subagents` and `get_command_or_subagent_output`
  return with the work still running, and `bash` with `block_until_ms` leaves
  its command running in the background.
- Steering and queued follow-ups wait in a pending band above the status row,
  grouped as `Steering` then `After yield`, until the turn takes them.
- `Alt+Up` returns pending steering and queued follow-ups to the editor without
  stopping the turn.
- A terminal UI and a headless `harness run` for coding agents, with parallel
  tool calls, steering of a running turn, and cancellation. `harness run` and
  `harness prompt` print only the final answer (or stream events as JSON) and
  wait for the turn to finish unless `runtime.prompt.wait_timeout_ms` sets a cap.
- Providers: OpenAI Responses and Chat Completions, any OpenAI-compatible
  endpoint, Anthropic Messages, Codex and GitHub Copilot sign-in, and Claude
  subscriptions through an installed Claude Code.
- Setup without a config file: sign in from the `/login` picker, which opens on
  first run, with `harness auth login`, or by exporting a provider API key. The
  first sign-in writes a starter `~/.harness/harness.jsonc` with that provider's
  default model unless a user config already exists. `harness doctor` and the
  `harness config` commands work before any config file exists, and
  `config sources` lists every path it searched.
- One home directory, `~/.harness` (or `$HARNESS_HOME`), for user config,
  credentials, sessions, prompts, agents, commands, skills and per-project data,
  paired with a `.harness/` directory in each project.
- Tools: read, write, edit (string replace, patches and hashline anchors), bash,
  grep, glob, list, web fetch and search, todos, questions, LSP, ast-grep,
  GitHub, MCP servers over stdio and HTTP, and persistent JavaScript and Python
  eval. Eval runs without an approval prompt by default, like bash; tools it
  calls keep their own permission checks. Its description names the host's
  platform, CPU and core count, and `tool_schema('eval:helpers')` returns the
  option reference for the advanced helpers.
- Per-model edit formats. With `edit.mode` left at `auto`, GPT and Codex models
  edit through `apply_patch`; Kimi, DeepSeek, MiniMax, MiMo, StepFun and GLM Flash
  use plain string replacement; other models use hashline anchors.
  `edit.model_variants` overrides the choice by model name, and the legacy
  `hashline_edit` boolean is still read.
- A todo pane above the transcript. It opens on its own while the agent's todo
  list has open items, and `Ctrl+t` shows, focuses or hides it.
- Bundled subagents (`task`, `scout`, `reviewer`, `security-reviewer`, `clerk`)
  with their own permissions under the project policy. Your own agents in
  `~/.harness/agents/` or `.harness/agents/` can add to or replace them, and
  can pick a model `variant`.
  Scout, reviewer and security-reviewer answers are checked against their role
  output schemas; a caller's `output_schema` replaces the role's.
- `model_roles` with `smol` and `slow` models; agents refer to them as `@smol`
  and `@slow`.
- Custom slash commands from Markdown files in `~/.harness/commands/` and
  `.harness/commands/`, with `$ARGUMENTS` and `$1`..`$9`, and a bundled `/init`
  that writes or updates `AGENTS.md`.
- Skills from `.agents/skills` and `~/.agents/skills`, a personal
  `~/.harness/AGENTS.md`, and `CLAUDE.md` where a directory has no `AGENTS.md`.
- Allow, ask and deny permission rules, YOLO mode, and opt-in Landlock
  confinement for shell commands.
- Append-only session journals with resume, fork, rewind, offline replay and
  redacted support exports. Sessions live in `~/.harness`, not in the project.
- Automatic and manual context compaction, per-directory `AGENTS.md` loading,
  skills and lifecycle hooks.
- Static Linux binaries for x86_64 and aarch64, and an install script.
- JSON schemas for `harness.jsonc` and `tui.jsonc` attached to each release as
  `harness.schema.json` and `tui.schema.json`.
- A live tokens-per-second readout and a session cost counter beside the
  context meter in the TUI. The speed updates while the model streams and
  settles to billed counts. The cost shows the main agent's spend and, after
  it, the spend of all its subagents, nested ones included. Each subagent's row
  in the Tasks pane shows its own speed and spend next to its model. Costs use
  models.dev prices for catalog models and an optional `cost` on custom models;
  a request whose prompt passes a long-context tier bills entirely at that
  tier's prices, and `cost.timeBased` bills off-peak requests a share of the
  peak price. DeepSeek's peak schedule is built in and switches itself off when
  models.dev's DeepSeek price changes. Anthropic one-hour cache writes bill at
  twice the input price. Each request's cost is saved when it finishes, so price
  changes leave past requests alone. Configured models without a price take one
  from the downloaded models.dev catalog. Anthropic token counts include cached
  prompt tokens, as OpenAI's already do.
- The models.dev catalog cache refreshes in the background when the TUI or a
  `prompt`/`run` session starts and the cache is over two hours old, after every
  sign-in, and on the new `harness models refresh`. Startup no longer waits for
  a first download.

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
  signed continuation state across tool calls without writing it to disk.
- Release archives and installed binaries include third-party license notices.
- Closing a login dialog cancels OAuth and releases its callback listener.
  Late results from a previous login cannot change a newly opened dialog.
- Claude Code runs in the selected agent workspace. Switching projects rebuilds
  the TUI's cached runtime configuration.
- Signing in refreshes model discovery for subagents in the current session as
  well as future sessions.
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

### Added

- `monitor` watches command output or a file in the background and wakes the
  agent with matching lines and the exit, with a live watching status in the
  footer and pause/rearm when updates get noisy.
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
  settles to billed counts. Costs use models.dev prices for catalog models and
  an optional `cost` on custom models. Anthropic token counts include cached
  prompt tokens, as OpenAI's already do.

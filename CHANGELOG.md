# Changelog

Notable changes to Harness. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/). Add entries under `[Unreleased]`;
`scripts/release.sh` turns that section into the next release.

## [Unreleased]

The first public release.

### Added

- A terminal UI and a headless `harness run` for coding agents, with streamed
  output, parallel tool calls, steering of a running turn, and cancellation.
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
  eval.
- A todo pane above the transcript. It opens on its own while the agent's todo
  list has open items, and `Ctrl+t` shows, focuses or hides it.
- Bundled subagents (`task`, `scout`, `reviewer`, `security-reviewer`, `sonic`)
  with their own permissions under the project policy. Your own agents in
  `~/.harness/agents/` or `.harness/agents/` can add to or replace them, and
  can pick a model `variant`.
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

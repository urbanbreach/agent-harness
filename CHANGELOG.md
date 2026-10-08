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
- Tools: read, write, edit (string replace, patches and hashline anchors), bash,
  grep, glob, list, web fetch and search, todos, questions, LSP, ast-grep,
  GitHub, MCP servers over stdio and HTTP, and persistent JavaScript and Python
  eval.
- Bundled subagents (`task`, `scout`, `reviewer`, `security-reviewer`, `sonic`)
  with their own permissions under the project policy.
- Allow, ask and deny permission rules, YOLO mode, and opt-in Landlock
  confinement for shell commands.
- Append-only session journals with resume, fork, rewind, offline replay and
  redacted support exports. Sessions live in the user data directory, not in the
  project.
- Automatic and manual context compaction, per-directory `AGENTS.md` loading,
  skills and lifecycle hooks.
- Static Linux binaries for x86_64 and aarch64, and an install script.

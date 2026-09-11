# Model-dependent system prompts

These assets are compiled into Harness. A fresh workspace needs no local prompt
files. Both dispatch and UI status use `harness_core::model_resolution`.

## Source

GPT (including Astra), Codex, reasoning, Anthropic, Gemini, Kimi, and default are adapted from
`anomalyco/opencode`, branch `2.0`, pinned revision
`7a6ce05d0939826aa6c8e1c481489a713b2d633f`:
`packages/opencode/src/session/system.ts` and `prompt/{gpt,codex,beast,anthropic,gemini,kimi,default}.txt`.
This is a source-branch pin, not a claim that a 2.0 release exists.
The upstream MIT notice is retained in `LICENSE.upstream`.

Adaptations replace product identity, documentation links, and unavailable tool
names with Harness equivalents. Anthropic task tracking uses only supplied
task-management tools rather than assuming a specific API. Gemini uses Harness path/permission
contracts. Reasoning retains the upstream workflow without mandatory broad web
research, automatic credential-file creation, or unsupported memory assumptions.
GPT Astra uses upstream `gpt.txt`, not a separate Astra asset: upstream routes
non-Codex GPT models outside GPT-4 to this prompt. Harness substitutes its identity,
`glob`/`grep`, `batch`, and `edit` tool names and adds the supplied-tool and
coordinator-permission contract while retaining the upstream engineering workflow.
Kimi now uses the full upstream `kimi.txt` rather than the earlier Harness-written
family stub. Its adaptations replace product identity and editing-tool assumptions,
use Harness workspace/permission/sandbox configuration instead of assuming an
unsandboxed environment, and prevent tags embedded in tool results from claiming
system-instruction authority.

## Routing

Catalog family metadata takes precedence over model-name inference. GPT-4 and
OpenAI reasoning models use `reasoning.md` (upstream beast); GPT Codex uses
`codex.md`; other GPT models, including `gpt-6-astra`, use `gpt.md`. Claude uses
`anthropic.md`, Gemini uses `gemini.md`, and Kimi uses `kimi.md`.
Unrecognized models use `default.md`.

Upstream has no Meta/Llama-specific asset or route at this revision. `meta.md` is
a Harness extension derived from upstream default with explicit Meta/Llama
identity and compact evidence/tool-schema guidance, not an upstream model
optimization. Llama IDs and `meta` family metadata select it.

## Precedence and recomposition

1. An explicit agent `system_prompt` replaces the model base.
2. A nonempty workspace `.agent-harness/prompt-families/<family>.md` overrides
   that family's bundled base. Identical shipped copies are reported as bundled.
3. Missing, empty, or unreadable workspace assets use the same family's bundled
   base, never another family's default. Empty/unreadable files produce warnings.

Shipped agent role bodies are additive guidance, including when discovery loads
an exact shipped copy into `system_prompt`. Custom role bodies remain overrides.
Delegation, project/AGENTS.md instructions, enabled skill guidance, and environment
sections retain their precedence. Model switches and provider fallback dispatch
recompose the base and model environment from retained inputs. CLI `--rules`
remain appended; the existing full `--system-prompt` override stays verbatim.

## Model-switch feedback

After accepting a model selection, the TUI shows
`Optimized system prompt applied: <model>` for a bundled recognized family,
`Default system prompt applied: <model>` for the fallback, or
`Configured system prompt applied: <model>` for a custom override. Live profile
changes report success only after the coordinator accepts the change. Picker
navigation alone emits no notice. Long notices wrap to the available terminal
width.

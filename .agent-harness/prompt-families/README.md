# Model-dependent prompts

Harness compiles these prompt files into the binary. A new workspace does not
need local copies. `harness_core::model_resolution` selects the prompt family
and supplies the source shown in the TUI.

## Source and license

The GPT, Codex, reasoning, Anthropic, Gemini, Kimi, and default prompts derive
from `anomalyco/opencode`, branch `2.0`, revision
`7a6ce05d0939826aa6c8e1c481489a713b2d633f`. The source files are
`packages/opencode/src/session/system.ts` and
`prompt/{gpt,codex,beast,anthropic,gemini,kimi,default}.txt`.
This identifies a source branch, not a release. The MIT notice is in
[`LICENSE.upstream`](LICENSE.upstream).

The adaptations use Harness's name, links, and supplied tool IDs. Anthropic uses
the available task tools. Gemini follows Harness path and permission rules.
The reasoning prompt omits mandatory broad web research, credential-file
creation, and unsupported memory assumptions.

GPT Astra uses the upstream `gpt.txt` prompt. The adaptation names `glob`, `grep`,
`batch`, and `edit`, and keeps coordinator permission checks. Kimi uses the full
upstream `kimi.txt`, with Harness workspace and sandbox rules. Tags in tool
results cannot claim system-instruction authority.

## Routing

Catalog family metadata takes precedence over model-name inference.

| Model family | Prompt |
| --- | --- |
| GPT-4 and OpenAI reasoning models | [`reasoning.md`](reasoning.md), derived from upstream `beast` |
| GPT Codex | [`codex.md`](codex.md) |
| Other GPT models, including `gpt-6-astra` | [`gpt.md`](gpt.md) |
| Claude | [`anthropic.md`](anthropic.md) |
| Gemini | [`gemini.md`](gemini.md) |
| Kimi | [`kimi.md`](kimi.md) |
| Llama IDs or `meta` family metadata | [`meta.md`](meta.md) |
| Unrecognized models | [`default.md`](default.md) |

Upstream has no Meta/Llama prompt at this revision. `meta.md` is a Harness
extension of the default prompt with model identity and tool-schema guidance.
It is not an upstream model optimization.

## Overrides

1. An explicit agent `system_prompt` replaces the model base.
2. A nonempty `.agent-harness/prompt-families/<family>.md` overrides that family's
   bundled base. An identical shipped copy is reported as bundled.
3. Missing, empty, or unreadable overrides use the same family's bundled base.
   Empty and unreadable files produce warnings.

Shipped role bodies add guidance, including exact shipped copies discovered in
`system_prompt`. Custom role bodies replace the base. Delegation, project
instructions, skills, and environment sections keep their existing precedence.

Model switches and provider fallback rebuild the prompt from the retained inputs.
CLI `--rules` stays appended. A full `--system-prompt` override stays verbatim.

## Model-switch feedback

After the coordinator accepts a selection, the TUI reports the prompt source:

| Source | Notice |
| --- | --- |
| Recognized bundled family | `Optimized system prompt applied: <model>` |
| Fallback family | `Default system prompt applied: <model>` |
| Custom override | `Configured system prompt applied: <model>` |

Moving through the picker alone emits no notice. Long notices wrap to the
terminal width.

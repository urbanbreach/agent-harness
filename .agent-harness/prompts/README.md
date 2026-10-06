# Shared agent prompts

These assets adapt [Oh My Pi](https://github.com/can1357/oh-my-pi) at
[`3f000c524cf82279f804ffd7526280cc9a5f25fe`](https://github.com/can1357/oh-my-pi/tree/3f000c524cf82279f804ffd7526280cc9a5f25fe).
The upstream MIT notice is preserved in [LICENSE.upstream](LICENSE.upstream).

Sources under `packages/coding-agent/src/prompts` are
`system/system-prompt.md`, `system/subagent-system-prompt.md`,
`system/personalities/default.md`, and `agents/{task,scout,reviewer,security-reviewer}.md`.
The `sonic` agent reuses the task prompt, as upstream does. Agent defaults and
spawn restrictions come from `src/task/agents.ts`; delegation policies come from
the upstream model catalog and `src/task/prompt-policy.ts`.

## Editable model prompts

Harness selects a Markdown entry point in `models/` for the actual model.
Entry points extend `system.md`, which retains the existing shared workflow,
capability checks and permission instructions. Versions with the same guidance
extend another model file. Each version still has an editable entry point.
Native children add `subagent.md` and their resolved role and persona.

| Models | Entry points |
| --- | --- |
| GPT | `gpt.md`, `gpt-5.md`, `gpt-5.2.md`, `gpt-5.3-codex.md`, `gpt-5.4.md`, `gpt-5.5.md`, `gpt-5.6.md`, `gpt-5.6-sol.md`, `gpt-5.6-terra.md`, `gpt-5.6-luna.md` |
| GPT-6 | `gpt-6.md`, `gpt-6-astra.md`, `gpt-6-sol.md`, `gpt-6.1-sol.md`, `gpt-6-luna.md` |
| Other OpenAI reasoning models | `codex.md`, `openai-reasoning.md` |
| Claude | `claude.md`, `claude-opus-4.5.md`, `claude-opus-4.6.md`, `claude-opus-4.7.md`, `claude-opus-4.8.md`, `claude-opus-5.md`, `claude-opus-5.5.md`, `claude-sonnet-5.5.md`, `claude-fable-5.md`, `claude-fable-5.1.md` |
| GLM | `glm.md`, `glm-5.2.md`, `glm-5.3.md` |
| Kimi and SWE-2 | `kimi.md`, `kimi-k2.6.md`, `kimi-k2.7.md`, `kimi-k2.8.md`, `kimi-k3.md`, `swe-2.md` |
| Grok | `grok.md`, `grok-4.5.md`, `grok-4.6.md`, `grok-4.7.md` |
| DeepSeek | `deepseek.md`, `deepseek-v4-flash.md`, `deepseek-v4-flash-0731.md`, `deepseek-v4.1-flash.md`, `deepseek-v4-pro.md` |
| Other families | `gemini.md`, `minimax.md`, `mistral.md`, `llama.md` |
| Unmatched models | `default.md` |

Specific model versions win over broad families. Catalog `metadata.family`
resolves aliases when the model ID does not identify a known version. Dotted,
dashed and underscore version spellings are accepted. Matching checks token
boundaries, so GPT-5.60 does not accidentally receive GPT-5.6 instructions.
Kimi's rolling coding IDs select K2.7 or K2.8; Mythos aliases select the matching
Fable preset. `harness models --json` exposes `resolution.prompt_preset`.

GPT-6 variants share the GPT-6 guidance. GLM 5.2 and 5.3 share GLM guidance;
Kimi K2.7 and K2.8 share their coding guidance. Unknown versions use their family
prompt without assuming the tuning of a future release.

## Edit without rebuilding

For each requested template, Harness checks these locations in order:

1. `.agent-harness/prompts/` in the working directory, then ancestors up to the
   nearest Git root. Outside a Git repository it checks only the working directory.
2. `$XDG_CONFIG_HOME/harness/prompts/`, or `~/.config/harness/prompts/` when XDG is unset.
3. The defaults bundled into the binary.

Paths inside those directories match this bundle, such as `models/glm-5.3.md`,
`models/gpt-6.1-sol.md`, `system.md`, `personality.md`, `subagent.md` and
`eval/claude.md`. You only need to create files you want to override. Includes and
inheritance use the same precedence independently, so a user model template can
inherit a project base. Editing the files in this checkout takes effect directly
when running Harness in this repository.

For example, create `.agent-harness/prompts/models/glm-5.3.md`:

```markdown
${% extends "models/glm.md" %}
${% block model_guidance %}
${{ super() }}
For this project, report the observed result before explaining the implementation.
${% endblock %}
```

To replace the entire system body for that model, put ordinary Markdown in the
file instead of extending the base. Project instructions and command rules still
follow it. To change a shared policy, override `system.md`; to change only the
selected model's policy, override its `model_guidance` or `delegation_policy`
block. The existing `${{ ... }}` and `${% ... %}` syntax remains available.

Harness reloads templates at turn start, after tool execution and when switching
or falling back to another model. Native children inherit the root agent's
project and user prompt locations, including children running in worktrees.
Missing files use the next location. Empty, oversized, unreadable or invalid
selected files fail visibly. Files must be UTF-8, at most 256 KiB, and remain
inside their prompt directory; template includes cannot traverse outside it.
Startup and inspection do not create prompt directories or copy defaults.

A nonempty `agent.<name>.system_prompt` remains a literal system-body override.
Model-specific eval tool guidance still applies to the tool description. Custom
agent definitions keep their existing template syntax, `tools.by_kind` and
schema-derived `params` bindings. Only reachable tools populate those bindings.

## Eval instructions

The `eval/` files supply the same text to the system prompt and the eval tool
signature. GPT uses concise composition and continuation rules; Claude and GLM
use an explicit tagged decision procedure and a conditional workspace example;
Kimi uses positive action wording. Other reasoning models get bounded recovery
advice, and unmatched models get general routing rules.

Two or more independent calls with known arguments belong in one eval cell.
Isolated operations remain direct. Result-dependent calls require inspection
before choosing the next step. Edits and side effects execute in order. These
instructions do not hide tools or grant permissions. `eval.route_tools` remains
the explicit operator control for tools that must be reached through eval.

Native children use their own model's eval dialect. The shared subagent prompt
reinforces it for every batch and shows how to read discovered files together.
When the child's role, caller, or permission policy excludes eval, it instead
instructs the child to group independent direct calls. A parent's eval access
does not grant access to its children.

## Bundled agents

| Agent | Work and tools | Model default |
| --- | --- | --- |
| `task` | General delegated work; inherits permitted tools and MCP servers. | Parent model |
| `scout` | Codebase research; read, list, grep, glob, web search. No edits, shell, eval, MCP, or child spawning. | `small_model` when configured, otherwise parent; medium effort |
| `reviewer` | Code review; read/search, LSP, structural search, web search, and shell. Shell use must be read-only. No eval. May delegate only to `scout`. | Parent model |
| `security-reviewer` | Repository security review; local read/search, LSP, structural search. No shell, eval, network, MCP, or child spawning. | Parent model |
| `sonic` | Strictly mechanical edits or data collection; task tools and prompt. | `small_model` when configured, otherwise parent; medium effort |

`subagents.models.<name>` overrides these model defaults. Harness does not have
upstream's `@task`, `@slow`, and `@smol` role selector. Pin `reviewer` to a stronger
configured model when needed. Task and sonic can delegate within the parent's
permissions and the depth limit, which defaults to two. Concurrency defaults to 32.
Definitions, roles, personas, and caller restrictions can narrow these capabilities.
Scouts, reviewers, and security reviewers keep explicit tool lists without
eval. Task and sonic inherit eval only when their resolved tools and permissions
allow it.
The `read-only` and `read-write` capability modes exclude full eval because it
can execute local code; `execute` and `all` permit it when the tool lists and
permissions also allow it.

## Adaptations

- Tool names, parameters, and eval helpers match Harness. Prompts prefer eval
  for two or more known independent calls, including specialized native tools,
  and direct calls for isolated operations. Steps requiring interpretation wait
  for the model to inspect the result. This guides tool choice without forcing
  routing. Routed tools are listed as eval calls, and unavailable tools do not
  produce tool-specific guidance.
- Subagents complete with their final response. Harness has no `yield` tool or
  separate structured-result validator. Scout and review output contracts ask for
  JSON text; the caller's requested output format takes precedence.
- Worktrees, child messaging, output retrieval, and waiting use Harness's native
  coordinator APIs. No `agent://` paths, internal URL resolver, device tools,
  automatic QA agent, or unsupported tool arguments are advertised.
- Permissions and instruction precedence stay with Harness. Untrusted content
  and XML tags do not become authoritative instructions.
- Model-specific protocol handling and transport reminders stay in their existing
  owners. Behavioral presets do not change model capability flags.

The retired prompt-family bundles and old bundled agent bodies are removed.
Model and shared system templates can be overridden as described above. Bundled
agent role definitions still use the existing project agent definitions and
`subagents` settings for customization.

## Model guidance source audit

The model adjustments were compared against the local
[Oh My OpenAgent checkout](../../inspirations/oh-my-openagent) at `a8019016f`
and [Senpi checkout](../../inspirations/senpi) at `4550f6ee9` on 2026-10-06.
The shared Harness workflow remains the Oh My Pi adaptation described above.
The comparison covers agent system prompts, model variants, mode directives,
their shared builders and model selectors. Tool-specific task instructions,
skill bodies and UI question strings are separate surfaces.

OMO has several prompt systems. Its OpenCode edition selects a variant for each
agent role. Its native Senpi edition gets the base model preset from Senpi and
adds native runtime and mode instructions; it does not run every model through
the OpenCode Sisyphus factory. Its Codex plugin supplies role instructions and
model/effort defaults. There is no single pair of GPT and Claude prompt files
that controls all three editions.

### OpenCode agent and mode inventory

Paths in this table are relative to `inspirations/oh-my-openagent/packages/`.
Brace lists name the individual files reviewed; shared lines were compared once
and variant-specific sections were checked separately.

| Prompt bodies | Model selection and differences |
| --- | --- |
| `omo-opencode/src/agents/sisyphus/{default,gpt-5-4,gpt-5-5,claude-opus-4-7,claude-opus-4-8,claude-opus-5,claude-fable-5,glm-5-2,kimi-k2-6,kimi-k2-7,kimi-k3,grok-4,gemini}.ts` | GPT 5.3 Codex/5.4 share 5.4; GPT 5.5/5.6/6 share 5.5. Opus 5/5.5 share Opus 5. Fable variants share Fable 5. GLM, including 5.3, uses `glm-5-2.ts`. K2.7/K2.8 share K2.7. Gemini adds corrective sections to the fallback prompt. |
| `omo-opencode/src/agents/sisyphus-junior/{default,gpt,gpt-5-4,gpt-5-5,glm-5-2,gemini,kimi-k2-6,kimi-k2-7,kimi-k3}.ts` | The worker keeps the same model distinctions where useful, with a bounded assignment and less orchestration. |
| `omo-opencode/src/agents/hephaestus/{gpt,gpt-5-4,gpt-5-5,gpt-5-6}.ts` | Autonomous implementation prompts. GPT 5.6 and 6 share the 5.6 variant. This role's direct implementation policy differs from Sisyphus's orchestration policy even on the same model. |
| `omo-opencode/src/agents/oracle.ts` | Default, GPT and GPT-5.5 prompt bodies; newer GPT versions reuse the latter. Advisory scope and response shape vary. |
| `omo-opencode/src/agents/{momus,momus-gpt-5-6}.ts` | Default and GPT plan review, plus a shorter outcome-first prompt for GPT 5.6/6. |
| `omo-opencode/src/agents/metis.ts` | Default and Kimi K2.7/K2.8 pre-planning analysis. |
| `omo-opencode/src/agents/{explore,librarian,multimodal-looker}.ts` | Role-specific search, external research and visual inspection contracts, without separate model prompt bodies. |
| `prompts-core/prompts/atlas/{default,gpt,gemini,glm,kimi,kimi-k2-7,kimi-k3,opus-4-7}.md` | Plan execution variants. The K3 selection also handles SWE-2; common execution and evidence rules are shared. |
| `prompts-core/prompts/prometheus/default.md` and `omo-opencode/src/agents/prometheus/system-prompt.ts` | The current planner prompt delegates its detailed workflow to the planning skill. |
| `prompts-core/prompts/ultrawork/{default,gpt,gemini,glm,planner,codex}.md` | Default/Gemini strongly prescribe planning and delegation; GPT allows more inline work; GLM adds explicit scope, tool and completion rules. Planner and Codex are separate mode contracts. |
| `prompts-core/prompts/mode/{hyperplan,team}.md` | Mode instructions, not model base prompts. |

Selection and composition were traced through `sisyphus-agent-factory.ts`,
`sisyphus-runtime-prompt-reconciler.ts`, the Junior/Hephaestus/Atlas selectors,
the `dynamic-agent-*` and `sisyphus-dynamic-prompt-*` builders, Gemini fallback
overrides, GPT file-edit guidance, the Kimi loop guard, and the shared prompt
variant tables. Available agents, skills, categories and tools are inserted
dynamically. The runtime reconciler refreshes Sisyphus after an actual model
change rather than relying only on its original configured model.

The native edition was checked through
`omo-senpi/src/components/ultrawork/generated-directive.ts` and the gateway and
memory prompt composition paths. Its seven task prompt bodies are
`senpi-task/src/agents/builtin/{explore,librarian,plan-consultant,plan-reviewer,code-reviewer,qa-executor,gate-reviewer}.ts`.
The native plan reviewer explicitly uses the default role body, without the
OpenCode per-model reviewer variants. The Codex plugin's twelve role files in
`omo-codex/plugin/components/ultrawork/agents/` are `explorer`, `librarian`,
`metis`, `momus`, `plan`, `lazycodex-worker-{low,medium,high}`,
`lazycodex-{code-reviewer,qa-executor,gate-reviewer,clone-fidelity-reviewer}`
with the `.toml` extension. These vary assignment and effort; all currently
select GPT-6 Astra. None is a substitute for a general model-family selector.

### Senpi presets and transferable differences

Senpi's `packages/coding-agent/src/core/extensions/builtin/prompt-preset/`
contains 29 selectable presets. GPT 5, 5.2, 5.3 Codex, 5.4, 5.5, 5.6 and
GPT-6 Astra cover its GPT families; the last preset also covers Sol and Luna.
Claude has Opus 4.5, 4.6, 4.7, 4.8, 5 and 5.5; Sonnet 5.5; Fable 5 and 5.1.
The remaining presets are GLM 5.2/5.3; Kimi K2.6/K2.7/K2.8/K3 (also SWE-2);
Grok 4.5/4.6/4.7; and DeepSeek V4 Flash, V4 Flash 0731, V4.1 Flash and V4 Pro.
GLM's two entry files call the same builder; K2.7 and K2.8 also share one.
Some presets append tuning to a common core; others replace that core.
`presets.ts` selects them and `index.ts` refreshes them at agent start and
model selection, while respecting an explicit system prompt override.

| Models | Adjustment seen in the reference prompts | Harness adaptation |
| --- | --- | --- |
| GPT 5/5.2/5.3 Codex/5.4 | Outcome and output contracts, dependency-aware retrieval, concrete tool use and exact file edits. | Concise working rules; bounded exploration; read before editing. |
| GPT 5.5 | Less process narration; decide by outcome, uncertainty and cost. | Finish the requested outcome and use short evidence-based updates. |
| GPT 5.6 | Explicit completion conditions, autonomous execution, inspect each result, preserve required detail when shortening output. | Completion and evidence rules over the shared workflow; gated delegation. |
| GPT 6, including 6.1 Sol | Continue through failures and steering; avoid repeated permission requests, excess checking and small delegated errands. | Session continuity, routine choices inline, restrained delegation and checks sized to the change. |
| Claude Opus 4.5/4.6 | Evidence, deliberate planning and persistence. | Concrete evidence and completion guidance. |
| Claude Opus 4.7/4.8 and Fable | Literal coverage of all requested items, bounded exploration, fewer narrated choices, clearer tool triggers. | Coverage, action and claim-checking instructions. |
| Claude Opus 5/5.5 and Sonnet 5.5 | Newer full-core prompts curb over-delegation and repeated verification; Sonnet also emphasizes completing prerequisites. | Inline local work, bounded scope, useful parallel tracks only, actual verification before a completion claim. |
| GLM 5.2/5.3 | Tagged Claude-style directives plus outcome-first rules; explicit tool use, short action/evidence cycles and literal all/each coverage. | Dedicated GLM model files and explicit eval batching instructions with a workspace example. |
| Kimi K2.6/K2.7/K2.8/K3 | Positive action instructions, exploration limits and clear stopping conditions; K3 acts on decisive evidence instead of re-deriving it. | Positive eval wording, evidence-driven progress and scope control; K3/SWE-2 use restrained delegation. |
| Grok 4.5/4.6/4.7 | Concrete definition of done, real artifact checks, reuse before adding components, stronger end-to-end completion. | Deliverable checks, reuse, scope and completion guidance. |
| DeepSeek V4 variants | Tool-grounded task contracts, avoid oscillation, match reasoning effort to uncertainty. | Evidence-based decisions, scoped recovery and proportionate investigation. |
| Gemini | Strong reminders to use actual tools, distinguish investigation from implementation and verify observed results. | Intent and evidence guidance without unsupported workflow mandates. |

These are the upstream authors' calibration choices, not independently verified
claims about model training or universal model behavior. Harness's short model
sections apply the relevant choices to its existing base. They do not import
upstream's entire role and mode machinery, mandatory planner/reviewer chains,
automatic commit policy, browser APIs, monitor subscriptions or fixed retry caps.

Senpi's `packages/senpi-codemode/src/prompt/{eval-prompt,eval-prompt-template}.ts`
separately select five eval dialects: GPT, Claude/GLM, other OpenAI reasoning
models, Kimi and default. Both the tool description and system guidance change
with the model. Harness uses the same separation with editable `eval/*.md`.
Senpi also has an eval-only tool filter. Harness retains its explicit
`eval.route_tools` setting, so a prompt preference does not become a hidden
change in tool access.

OMO's root license is the Sustainable Use License. Its source was reviewed for
behavior and routing; its prompt bodies were not copied into this MIT bundle.
The model additions use original Harness wording informed by that comparison
and Senpi's MIT-licensed presets. The relevant MIT notices are retained in
`LICENSE.upstream`.

### Verification on 2026-10-06

All 50 model entry points rendered from the compiled defaults with full and
restricted tools. Provider-boundary checks cover actual model selection,
fallback and switching, literal overrides, native child roles, project/user
precedence, reload after a tool call, ancestor discovery and invalid files.
The workspace passed compilation, Clippy and 1,979 deterministic nextest tests.

Live CLI checks used GPT-6.1 Sol and GLM 5.3 with normal bundled prompts and
`read,list,grep,bash,eval` exposed. Both discovered three fixture files, batched
the reads in a second eval call, and returned the correct total, 87. In the
single-file case, GLM used one direct read; GPT used a direct listing followed
by a direct read. GLM's first trial batched only its opening checks; adding the
every-batch instruction produced correct routing on the repeated checks.
These observations verify the tested scenarios. Other models have deterministic
selection/rendering coverage only; prompting cannot guarantee every future
tool choice.

# Agent prompts

Every model Harness supports gets its own complete system prompt in `models/`,
written for Harness's tools and runtime. Bundled subagent roles live in `agents/`.
The `sonic` agent reuses the task prompt. Role bodies keep their scope, tool
limits and output fields; shared tool routing, hand-off and completion rules live
in `subagent.md` rather than being repeated in each role.

## Layout

| Path | Owns |
| --- | --- |
| `models/<preset>.md` | The whole behavioral prompt for one model: identity, intent routing, scope, how to work, verification, hard limits, style, delegation policy. |
| `partials/tools.md` | Tool inventory, specialized-tool routing and the permission facts. Lines appear only for tools the turn actually has. |
| `partials/delegation.md` | How Harness subagents work: blank-context children, one integration owner, concurrency, notifications. The model file decides when to delegate. |
| `partials/test-discipline.md` | Test rules shared by the presets that include them. |
| `partials/environment.md` | OS, working directory, date, active model, project-instruction precedence and skill loading. |
| `eval/<dialect>.md` | Eval routing for one model dialect. The model file places it once. |
| `subagent.md` | Appended for native children: role, tool use, hand-off rules, completion. |
| `agents/*.md` | Bundled subagent role bodies. |

There is no shared behavioral base. Models that use the same preset are one-line
aliases. Facts that do not depend on the model, such as the tool section, project
instructions and the environment block, live in the partials.

## Model presets

| Models | Standalone prompts | Aliases |
| --- | --- | --- |
| Claude | `claude.md`, `claude-opus-4.5.md`, `claude-opus-4.7.md`, `claude-opus-4.8.md`, `claude-opus-5.md`, `claude-opus-5.5.md`, `claude-sonnet-5.5.md`, `claude-fable-5.md`, `claude-fable-5.1.md` | `claude-opus-4.6.md` -> `claude.md` |
| GPT | `gpt.md`, `gpt-5.md`, `gpt-5.2.md`, `gpt-5.3-codex.md`, `gpt-5.4.md`, `gpt-5.5.md`, `gpt-5.6.md`, `gpt-6-astra.md` | `gpt-5.6-{sol,terra,luna}.md` -> `gpt-5.6.md`; `gpt-6-sol.md`, `gpt-6-luna.md`, `gpt-6.1-sol.md` -> `gpt-6-astra.md`; `gpt-6.md`, `openai-reasoning.md` -> `gpt.md`; `codex.md` -> `gpt-5.3-codex.md` |
| GLM | `glm.md` | `glm-5.2.md`, `glm-5.3.md` |
| Kimi and SWE-2 | `kimi.md`, `kimi-k2.6.md`, `kimi-k2.7.md`, `kimi-k3.md` | `kimi-k2.8.md` -> `kimi-k2.7.md`; `swe-2.md` -> `kimi-k3.md` |
| DeepSeek | `deepseek.md`, `deepseek-v4-flash.md`, `deepseek-v4.1-flash.md`, `deepseek-v4-pro.md` | `deepseek-v4-flash-0731.md` -> `deepseek-v4-flash.md` |
| Grok | `grok.md`, `grok-4.5.md`, `grok-4.6.md`, `grok-4.7.md` | |
| Gemini | `gemini.md` | |
| Unmatched models | `default.md` | `llama.md`, `minimax.md`, `mistral.md` |

Specific model versions win over broad families. Catalog `metadata.family`
resolves aliases when the model ID does not identify a known version. Dotted,
dashed and underscore version spellings are accepted. Matching checks token
boundaries, so GPT-5.60 does not accidentally receive GPT-5.6 instructions.
Kimi's rolling coding IDs select K2.7 or K2.8; Mythos aliases select the matching
Fable preset. `harness models --json` exposes `resolution.prompt_preset`.

Claude Opus 5 and 5.5, Sonnet 5.5, Fable 5 and 5.1, GPT-5.5, GPT-5.6, GPT-6 Astra,
Kimi K3 and Grok 4.5 to 4.7 have prompts written for that model. Claude Opus 4.x,
GPT-5 to 5.4, GLM, Kimi K2 and DeepSeek V4 use the default prompt in their family's
dialect, with version tuning written into the section it governs. Family fallbacks
(`claude.md`, `gpt.md`, `kimi.md`, `deepseek.md`, `grok.md`) carry the family
dialect without version tuning, so an unknown version gets no tuning meant for a
different release. Bare or unknown GPT-6 IDs use `gpt.md` rather than the Astra
preset. Llama, MiniMax and Mistral have no documented tuning and use `default.md`.
Gemini adds its own guidance to the default prompt.

Each preset names its model and tuning focus in a `${# ... #}` comment at the top
of the file.

## Edit without rebuilding

For each requested template, Harness checks these locations in order:

1. `.harness/prompts/` in the working directory, then ancestors up to the
   nearest Git root. Outside a Git repository it checks only the working directory.
2. `<home>/prompts/`, where `<home>` is a nonempty `$HARNESS_HOME` used as-is,
   otherwise `~/.harness/`.
3. The defaults in `crates/harness-core/prompts/`, bundled into the binary.

Paths inside those directories match this bundle, such as `models/glm-5.3.md`,
`partials/tools.md`, `subagent.md` and `eval/claude.md`. You only need to create
files you want to override. Includes and inheritance use the same precedence
independently, so a user model template can extend a bundled one and a project
partial replaces the bundled partial for every model.

Every `##` section of a model file is a block named after its heading in
snake_case (`intent_gate`, `working_the_task`, `verification`, `style`, ...), the
opening identity line is the `identity` block, and an empty `extra` block sits
before the environment section. To adjust one model, extend its file and
override a block. For example, `.harness/prompts/models/glm-5.3.md`:

```markdown
${% extends "models/glm.md" %}
${% block extra %}
For this project, report the observed result before explaining the implementation.
${% endblock %}
```

Use `${{ super() }}` inside a block to keep the original text and add to it. To
replace the entire system body for a model, put ordinary Markdown in its file;
project instructions and command rules still follow it. If such a file leaves
out `${{ eval_guidance }}`, Harness puts the eval routing in the eval tool
description instead, so the model still receives it exactly once.

Templates use `${% ... %}` blocks, `${{ ... }}` variables and `${# ... #}`
comments. Block tags on their own line leave no blank line behind, and runs of
blank lines collapse to one. The variables are `tools` (tool id to callable
name, or `eval: tool.<name>` for tools reachable only inside eval), `inventory`,
`model`, `prompt_preset`, `working_directory`, `os_name`, `current_date`,
`subagent`, `isolated`, `headless`, `max_concurrent`, `limit_behavior`, `eval_guidance` and
`behavior` (`behavior.command_notifications` and
`behavior.directory_instructions` mirror the `runtime.behavior` settings, so a
prompt only describes guidance Harness will actually give).

Harness reloads templates at turn start, after tool execution and when switching
or falling back to another model. Native children inherit the root agent's
project and user prompt locations, including children running in worktrees.
Missing files use the next location. Empty, oversized, unreadable or invalid
selected files fail visibly. Files must be UTF-8, at most 256 KiB, and remain
inside their prompt directory; template includes cannot traverse outside it.
Startup and inspection do not create prompt directories or copy defaults.

A nonempty `agent.<name>.system_prompt` remains a literal system-body override,
and the eval routing then goes to the eval tool description. Custom agent
definitions keep their existing template syntax, `tools.by_kind` and
schema-derived `params` bindings. Only reachable tools populate those bindings.

## Eval routing

`eval/` holds one routing text per dialect, chosen from the preset name:

| Dialect | Presets | Shape |
| --- | --- | --- |
| `claude.md` | Claude, GLM, DeepSeek V4.1 Flash | Tagged decision procedure with a few uppercase key words and a workspace example |
| `gpt.md` | GPT | Terse composition rules; detached cells notify instead of being polled |
| `kimi.md` | Kimi, SWE-2 | Positive action wording without uppercase prohibitions |
| `codex.md` | Codex, other OpenAI reasoning models | Bounded rules with a recovery path when cells fail |
| `default.md` | Everything else | General batching rules |

All five texts take the same position: independent reads and probes
go into one cell; edits, side effects and calls that depend on unseen results
run one at a time and get inspected; a result with a failed item or a truncated
tail is not evidence. They do not hide tools or grant permissions.
`eval.route_tools` remains the operator control for tools that must be reached
through eval. The eval tool's own description covers cell mechanics.

Native children use their own model's prompt and eval dialect. `subagent.md`
reminds them that the parent's eval calls do not cover their work, or, when the
child has no eval, tells it to group independent direct calls. A parent's eval
access does not grant access to its children.

## Bundled agents

| Agent | Work and tools | Model default |
| --- | --- | --- |
| `task` | General delegated work; inherits permitted tools and MCP servers. | Parent model |
| `scout` | Codebase research; read, list, grep, glob, web search. No edits, shell, eval, MCP, or child spawning. | `@smol`, otherwise parent; medium effort |
| `reviewer` | Code review; read/search, LSP, structural search, web search, and shell. Shell use must be read-only. No eval. May delegate only to `scout`. | `@slow`, otherwise parent |
| `security-reviewer` | Repository security review; local read/search, LSP, structural search. No shell, eval, network, MCP, or child spawning. | `@slow`, otherwise parent |
| `sonic` | Strictly mechanical edits or data collection; task tools and prompt. | `@smol`, otherwise parent; medium effort |

`subagents.models.<name>` overrides these model defaults. `model_roles.smol` and
`model_roles.slow` configure concrete `provider/model[/variant]` references for
`@smol` and `@slow`; an unset role inherits the parent model. Those selectors
also work in definition and per-call models. Other `@names` are invalid.
Optional definition frontmatter `variant` chooses a model variant. Per-call,
role, persona, definition and parent variants apply in that order; unknown
variants are ignored with a runtime warning. Task and sonic can delegate within
the parent's permissions and the depth limit, which defaults to two.
Concurrency defaults to 32.
Definitions, roles, personas, and caller restrictions can narrow these capabilities.
Scouts, reviewers, and security reviewers keep explicit tool lists without
eval. Task and sonic inherit eval only when their resolved tools and permissions
allow it.
Scout, reviewer and security-reviewer also carry default output schemas that
match their role prompts. The runtime checks their final answers against them and
sends bounded corrections, as it does for a caller's `output_schema`; a caller's
schema replaces the role's.
The `read-only` and `read-write` capability modes exclude full eval because it
can execute local code; `execute` and `all` permit it when the tool lists and
permissions also allow it.

## Behavior shared by every preset

- Identity is Harness. The main agent opens each turn with a short routing line
  whose stop condition is binding. Children have no routing line; their
  assignment's completion condition is the stop condition. In one-shot
  `harness run`/`prompt` (`headless`), presets print no opening line and set the
  stop condition silently, because only the final message reaches stdout.
- The final message gives the outcome in complete sentences, then how it was
  verified. There is no labeled hand-off block, because Harness has no parser or
  view for one. Fable 5.1 adds a brief progress line.
- Asking the user goes through the `question` tool, and only when it is
  available. Todo guidance renders only for the main agent when `todowrite` is
  available. Every tool name in a prompt is rendered from the active tool set.
- `monitor` watches command output or a file condition in the background. Its
  events and background shell completion notices follow
  `runtime.behavior.command_notifications`; when disabled, wait or retrieve
  output with `wait_commands_or_subagents` / `get_command_or_subagent_output`.
  Background subagents and detached eval cells notify when they finish.
- The user can steer a running turn from the TUI: Enter while the turn runs
  sends the message into it, and the follow-up key queues one for after the
  turn. A steering message joins the turn before its next model request; the
  environment partial says so. Interrupting the turn returns undelivered
  steering and follow-ups to the editor. Harness compacts context
  automatically.
- Harness never commits on its own. Commit rules apply only when the user asks
  for commits.
- Children skip the main agent's verification tiers and report shape; the
  hand-off rules in `subagent.md` decide who runs checks.
- The prompts ask for direct answers without moralizing or reflexive hedging.
  None of them tells the model never to refuse.

## Provenance

The prompt text here was written for Harness from behavior specs. One set of
authors described each earlier prompt as rules, conditions and template
structure without its wording. Different authors, who never saw the earlier
text, wrote these files from those specs. Reviewers then compared old and new
rule by rule and reported gaps in abstract terms, and the writers closed them.

## Verification on 2026-10-11

The bundled-template test renders all 50 model entry points from the compiled
defaults four ways: main agent with every tool, main agent with tools reachable
only through eval, a child without the question and todo tools, and no tools.
Each rendering must be free of template syntax, carry the eval routing exactly
once when eval is available, and never name a tool the turn does not have.
Provider-boundary tests cover model fallback and switching, literal overrides,
native child roles and their output fields, project and user precedence,
reload, and the eval routing falling back to the tool description when a
template omits it. The same 200 renderings were also scanned for merged
paragraphs, headings run into text, doubled spaces and stray punctuation left
by conditional branches; none remained.

Compared with the earlier text, 1.1% of six-word sequences and 0.3% of
eight-word sequences in the new files also appear in the old ones. The longest
shared runs are output field lists, the fixed literals that tests and parsers
depend on, and stock phrases such as a rule against committing unasked.

Live CLI runs used Claude Opus 5.5 through the configured Claude subscription,
in a fixture repository whose three text files hold 17, 25 and 45:

| Request | Observed |
| --- | --- |
| Total of the numbers in the text files | Routing line with intent, approach and stop condition; one glob, three reads in one response; answered 87 and stopped |
| Use a scout subagent to find the numbers, then report the total | Routing line; one scout with a complete brief; the scout batched a glob and a listing, then three reads, and returned JSON with `summary` and `files`; the parent answered 87 |

Without an explicit permission rule or YOLO, a headless run denies the subagent
launch; the model then stopped and asked rather than reading the files itself
against the request.

### Side by side with the earlier prompts

A binary built from the earlier prompts and one built from these got the same
six tasks, each in a fresh copy of a small Python project with one failing test:
explain a function, fix the failing tests, "clean up utils.py", rename a
function everywhere, delegate an environment-variable search to a scout, and a
question in Finnish. Claude Haiku 5.5 (`medium`, `claude.md`) and GPT-6 Luna
(`gpt-6-astra.md`) ran every task four times per version.

| Check | Haiku before | Haiku now | Luna before | Luna now |
| --- | --- | --- | --- | --- |
| Explain, fix, rename and scout tasks done correctly | 16/16 | 16/16 | 16/16 | 16/16 |
| Fix report cites test evidence | 3/4 | 4/4 | 4/4 | 4/4 |
| "Clean up utils.py" edited instead of proposed | 0/4 | 0/4 | 4/4 | 1/4 |
| Scout returned its JSON object | 3/4 | 4/4 | 4/4 | 4/4 |
| Finnish question answered correctly | 4/4 | 4/4 | 2/4 | 4/4 |
| Finnish reply, opening line included | 3/4 | 3/4 | 4/4 | 0/4 |
| Opening line states when the work is done | 21/24 | 24/24 | 23/24 | 22/24 |
| Model calls across all runs | 145 | 135 | 172 | 191 |

The first rewrite lost two behaviors that these runs exposed: Haiku often left
the done condition out of its opening line, and the scout answered in Markdown
when the brief described the content it wanted. The default presets now give a
fill-in opening line, and the scout's output rule says that only an explicit
format instruction replaces its JSON object. Claude presets use a sentence
pattern, which models translate. The other presets keep a labelled line,
because Luna dropped the done condition from a translated sentence; Luna
therefore keeps the English `Intent:` label in other languages. Luna's cleanup
runs now mostly follow the rule that a loosely specified cleanup gets a
proposal first.

These runs check the tested scenarios only. Other models have deterministic
selection and rendering coverage; prompting cannot guarantee every future tool
choice.

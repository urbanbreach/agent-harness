# Musings on an Opencode Shell with an OMX Soul

This is not a roadmap, not an acceptance spec, and not a promise that the
harness is already equivalent to either of its inspirations. It is a broad read
of the project as it sits now: a Rust event-sourced agent harness that wants the
immediacy and legibility of an Opencode-style terminal UI, while adopting the
workflow discipline and single-operator force multiplication of OMX.

The strongest impression is that the project has chosen the right center of
gravity. The visible product should not become a many-primary-agent machine in
the OMO sense. The repo already says the current product model is
single-operator workflow orchestration, with specialists, teams, and
compatibility lanes exposed as explicit operator-owned escalation rather than
alternate defaults (`README.md`, `docs/omo-parity-spec.md`,
`docs/parity-ledger.json`). That framing matters more than any feature checklist.
It says the human should feel like they are driving one capable cockpit, not
mediating a committee of autonomous personalities.

The current state is best described as strong foundations with uneven lived
workflow. The workflow inventory lists 80 `present`, 22 `partial`, 22 `missing`,
0 `clashing`, and 4 `non_applicable` entries across 128 classified items
(`docs/harness-omx-workflow-inventory.md`). Those numbers are useful accounting,
but they are not a score. They say the project has mapped the surface honestly:
some things are real, some are staged, some are intentionally blocked, and some
do not belong in the harness objective. The dirty workspace at the time of this
read also means any analysis should be careful about declaring completion. This
is a moving project with many active edits, not a frozen evidence bundle.

What feels most promising is that the harness is not trying to graft OMX on as a
pile of command aliases. The load-bearing architecture is already harness-native:
events are the source of truth, the coordinator is the scheduling authority,
permissions happen before tool execution, replay derives state rather than
performing side effects, tool outputs are capped and redacted, and teams,
background tasks, persistent tasks, continuation events, and workflow evidence
all have places in the event model (`docs/architecture.md`). That means OMX can
be translated into durable coordinator-owned facts instead of copied as a set of
shell tricks.

That translation is the whole game.

## The real objective

The target I keep coming back to is: one operator, one visible working surface,
many controlled powers behind it. The operator should be able to clarify,
inspect, plan, execute, branch, pause, escalate, review, resume, and close out
without losing the thread. The system should make the current workflow state
obvious. It should say when work is running, when it is blocked, when it is
waiting for the user, when it failed, when it has evidence, and when it is merely
staged. It should resist the very agentic failure mode that OMX is obsessed with:
partial work getting narrated as completion.

The harness README already encodes this shift. `operator` is the default-visible
profile. `plan`, `discipline`, compatibility build lanes, specialist subagents,
and category agents exist, but they are hidden or explicit escalation paths
instead of normal startup choices (`README.md`, `docs/config.md`). That is the
right relationship. The user should not need to think, "Which main agent am I
talking to now?" They should think, "I am in my harness session, and I can ask
for Plan, Team, Ralph-like continuation, or a specialist when the work deserves
it."

This is also why the OMO artifacts should remain background migration evidence.
They are valuable because they show what has been ported, what was learned, and
where old compatibility vocabulary came from. But the product direction should
not be OMO's multi-main-agent theater. OMO's agent taxonomy, category routing,
skills, and background machinery are useful ingredients; they are not the dish.
The dish is an OMX-style operator workflow presented through an Opencode-like
terminal shell.

The danger is not that the harness lacks features. The danger is that it could
accumulate too many surfaces that are individually plausible but collectively
make the operator feel less in control. The harness should prefer one coherent
operator path with visible escalation over a wide shelf of commands that all
sound powerful but do not share state, evidence, and closeout semantics.

## What Opencode and Shuvcode contribute

Opencode's gift is not merely that it has commands, modes, sessions, providers,
or a TUI. Its gift is that it treats the terminal as a primary product surface.
The TUI starts where the user is, accepts prompts directly, supports file
references, makes slash commands discoverable, gives keybindings a coherent
leader-key grammar, and keeps model/session/status controls close to the compose
surface (`inspirations/shuvcode/packages/web/src/content/docs/tui.mdx`,
`inspirations/shuvcode/packages/web/src/content/docs/keybinds.mdx`). The user
does not have to feel like they are operating a debug console. They are in a
terminal-native application.

Shuvcode's fork is especially interesting because it is not just a static copy
of Opencode docs. It is a testbed for small UX improvements: session sidebars,
subsession navigation, command-palette affordances, mobile/desktop review
surfaces, IDE selection context, spinner/density controls, ask-question wizard
dialogs, and status visibility for plugins and MCP/LSP surfaces
(`inspirations/shuvcode/README.md`). Not all of that belongs in this harness,
and some of it belongs only as inspiration. But the pattern is useful: the
product earns trust by making state and choices visible in the interface, not by
requiring the user to remember hidden incantations.

The harness TUI guidance already points in that direction. Its shell contract is
compose-first, transcript-first, and operator-sidebar-oriented. The home screen
should start from the composer, live sessions should prioritize transcript
rendering, the right sidebar should persist operator state, file context, and
tool status, and debug/inspector surfaces should stay secondary
(`crates/harness-tui/AGENTS.md`). That is the right visual grammar for an
Opencode-like surface. The question is whether workflow state is visible enough
inside that grammar.

Right now, the gap I feel is discoverability and emotional certainty. The README
lists many slash commands: `/model`, `/status`, `/toggles`, `/resume`, `/new`,
`/tree`, `/fork`, `/clone`, `/compact`, continuation controls, workflow commands,
goal/mission/wiki surfaces, and staged helper aliases (`README.md`). The CLI has
workflow `run/status/signoff/cancel/dossier/snapshot/plan-consensus/goal/mission/wiki/evidence/init`
surfaces (`README.md`, `docs/config.md`). But a user sitting in the TUI should
not have to read the README to know what the harness can do next. The shell
needs to communicate the operator's current workflow lane, available next moves,
blocked conditions, and evidence state as part of its normal posture.

The Opencode lesson is: a good terminal product feels small even when it is
large. The harness should use that lesson ruthlessly. The command palette should
not be a junk drawer. Status should not be a generic system dump. The sidebar
should not merely be decorative context. It should answer: what am I doing, who
or what is helping me, what evidence exists, what is blocked, and what can I do
next without breaking the workflow contract?

## What OMX contributes

OMX's gift is workflow discipline. Its README describes a default rhythm:
clarify with `$deep-interview`, approve a plan with `$ralplan`, execute through
`$team` or `$ralph`, track durable goals with `$ultragoal`, and use doctor/HUD/
wiki/status surfaces as support rather than as the main mental model
(`inspirations/oh-my-codex/README.md`). The best part of OMX is not the exact
spelling of those commands. It is that work has phases, state, transition rules,
and completion evidence.

The `STATE_MODEL.md` reference is very clear about this. OMX has authoritative
per-mode state files under `.omx/state/`, compatibility state for hooks/HUD, a
session/root precedence model, explicit terminal lifecycle vocabulary, and
allowlisted transitions between planning-like and execution-like modes
(`inspirations/oh-my-codex/docs/STATE_MODEL.md`). Planning should not silently
roll backward from execution. Simultaneous skill requests should preserve
planning when planning is the safer primary branch. State reconciliation should
terminalize old modes before activating new ones so stale state does not
resurrect. That is all deeply relevant to the harness.

But the harness should not adopt `.omx/state/` as its authority. That would fight
the architecture. In this repo, the durable authority should be coordinator-owned
events and replay-derived projections (`docs/architecture.md`). The OMX state
model should become a semantic inspiration: explicit workflow state, explicit
terminal outcomes, transition policy, state reconciliation, user-visible
messages, and no silent resurrection of stale modes. The storage mechanism should
remain harness-native.

Ralph is the clearest example of the OMX philosophy. The Ralph skill exists
because complex tasks fail silently: partial implementations get described as
done, tests are skipped, and edge cases disappear. Ralph requires context intake,
continued execution, parallel delegation when useful, fresh verification,
architect review, optional deslop, regression re-verification, and completion
audits before final success (`inspirations/oh-my-codex/skills/ralph/SKILL.md`).
The precise loop mechanics may not map one-to-one. The principle absolutely
does: completion is not a feeling; it is an evidence-backed state transition.

Ultrawork adds the parallelism discipline under Ralph. It says to ground the
task, define pass/fail criteria, classify independent lanes, choose local versus
delegated work deliberately, run independent work in parallel, and close with
evidence (`inspirations/oh-my-codex/skills/ultrawork/SKILL.md`). That maps almost
perfectly onto the harness's coordinator and background task model. The harness
does not need to mimic OMX's exact text prompt. It needs to make this discipline
easy to follow and hard to accidentally bypass.

Team mode is trickier. OMX's team mode is operationally sensitive: tmux panes,
worker sessions, shared `.omx/state/team/...` files, inboxes, mailboxes,
worker lifecycle, status, resume, and shutdown
(`inspirations/oh-my-codex/skills/team/SKILL.md`). The harness already has an
event-sourced team orchestration model with `TeamCreated`, `TeamMemberSpawned`,
team messages, team tasks, shutdown requests, approvals, rejections, and deletion
events (`docs/architecture.md`). That is a better native substrate than copying
tmux/worktree mechanics into the core. The OMX lesson is not "make tmux the
truth." The lesson is "parallel workers need shared task state, lifecycle
control, startup evidence, terminal evidence, and clean shutdown." The harness
can express that with events and projections.

So the cornerstones of OMX, translated into harness language, look like this:

- one visible operator authority;
- named workflow modes with explicit transitions;
- clarify-before-execute when ambiguity is material;
- plan/consensus before expensive or risky work;
- continuation loops that preserve context instead of restarting;
- parallelism only when work is actually independent or team-shaped;
- evidence-gated closeout;
- HUD/status/readiness surfaces that tell the truth;
- durable goal, mission, and wiki memory;
- honest blocked, failed, waiting, and question states.

That is the soul worth taking.

## Where the harness already has the right bones

The event store is the most important bone. Events are the source of truth, and
replay is supposed to derive state without side effects (`docs/architecture.md`).
That one decision keeps the harness from becoming a bag of shell state. It means
workflow status, team status, background completions, permission decisions,
compaction, transcripts, run summaries, session catalogs, and closeout readiness
can all become inspectable and replayable.

The coordinator boundary is the second bone. The coordinator is the single
scheduling authority. Permission checks happen before tool execution. Background
child task completion wakeups are coordinator-owned. `background_output` resolves
lineage and status through replay projection rather than a local in-memory
manager (`docs/architecture.md`). That is exactly the kind of authority model a
serious workflow layer needs.

The config model is another good sign. The harness public contract uses
harness-centered names, keeps runtime config separate from TUI config, makes
`operator` the default, treats `plan` as an escalation, and rejects active
OpenCode-style server/command/plugin/share/autoupdate areas while accepting
empty disabled placeholders for migration round-tripping (`docs/config.md`).
That is the right compromise: compatibility without surrendering authority.

The workflow CLI and native tools are already further along than a superficial
read might suggest. The harness exposes workflow status, signoff, cancel,
dossier export, snapshots, plan consensus, goal ledger, research mission, wiki,
and evidence recording. Status, dossier, snapshot, goal, and mission reads are
intended to derive from recorded events rather than rerunning hooks or tools
(`README.md`). Evidence recording has categories and status metadata so closeout
can block on failed findings. This is exactly the kind of substrate OMX would
want if it were rebuilt inside an event-sourced Rust harness.

Testing also has the right shape. There is a fast lane, integration lane,
workflow replay/dossier evidence guidance, deterministic PTY signoff, live
provider opt-in, browser/media signoff, native screenshot signoff, and stress
lanes (`docs/testing.md`, `scripts/test-lanes.sh`). The presence of lanes is not
the same as UX quality, but it shows a culture of evidence. It also gives the
workflow layer something concrete to point at when it says a run is ready or not
ready.

Finally, the inventory is honest. It does not hide partial and missing entries.
It names staged aliases, explicit escalation lanes, and blockers
(`docs/harness-omx-workflow-inventory.md`). That honesty should be preserved in
the UI. If a command is staged, say staged. If a workflow is blocked until it can
write workflow-owned evidence, hide it or label it plainly. A half-visible
compatibility command is worse than no command because it trains the user not to
trust the surface.

## Where the lived experience still feels thin

The gap is less about raw capability and more about how much of that capability
is visible as a coherent operator experience. The repo can describe workflow
state. The CLI can expose workflow state. The event model can persist workflow
state. But the product feeling depends on whether the TUI makes that state
ambiently legible.

The slash command surface is one pressure point. A long command list is not the
same as discoverability. Opencode-style `/` menus work when commands are obvious,
grouped, described in the user's current context, and backed by keybindings or
status affordances (`inspirations/shuvcode/packages/web/src/content/docs/tui.mdx`,
`inspirations/shuvcode/packages/web/src/content/docs/commands.mdx`). In the
harness, workflow commands should probably feel different from navigation
commands, and staged compatibility aliases should not appear beside mature
operator commands without a visible warning. `/workflow-status`, `/status`,
`/toggles`, `/goal`, `/mission`, `/wiki`, `/plan-consensus`, and continuation
commands all belong to the same mental neighborhood, but the user needs the TUI
to show that neighborhood.

The HUD/sidebar story is another pressure point. OMX has a HUD/status culture:
not as the primary workflow, but as a live truth surface. The harness TUI already
wants a persistent operator sidebar (`crates/harness-tui/AGENTS.md`). That
sidebar is the natural place to show the active workflow id, mode, owner,
terminal outcome, background children, team lanes, pending questions, current
goal, dossier/evidence state, closeout blockers, and legal next actions. Without
that, workflow features become CLI subcommands the user must remember.

Closeout specificity is a third pressure point. The codebase has a real
closeout evaluator and policies, and it is good that status/signoff/dossier
surfaces use replay-derived readiness (`crates/harness-core/src/workflow_closeout.rs`,
`crates/harness/src/workflow_cli.rs`, `crates/harness-tools/src/workflow_tools.rs`).
But the recurring use of `WorkflowSignoffPolicy::simulator_default()` in CLI/tool
paths is a reminder that some closeout semantics may still be generic compared
with the richness of OMX workflows. A Ralph-like coding loop, a team execution,
a research mission, a wiki refresh, and a visual QA loop should not all feel like
the same abstract checklist. They can share the same evaluator machinery, but
their evidence expectations should be intelligible at the workflow level.

Cancel/status/dossier semantics need special care. These are the surfaces that
teach the user whether the harness is serious about authority. A status read
should not mutate. A dossier export should not mark work complete. A cancel
should mean a specific workflow id or continuation id was terminalized, not just
that a nearby audit run recorded a cancellation. The docs already lean this way:
workflow reads must be projection-only, dossier exports are regenerated from
`events.jsonl`, and staged dossiers must name pending gates instead of implying
false completion (`docs/testing.md`). The UI should make those distinctions
plain.

The tests prove safety and regression coverage, not delight. That is not a
criticism of the tests. It is a reminder that a green fast lane does not mean
the operator understands what happened. PTY and native visual signoff can prove
rendering contracts, and integration lanes can prove replay and permission
contracts (`docs/testing.md`). They cannot by themselves prove that a tired user
at 1 a.m. can glance at the TUI and understand whether the current workflow is
blocked on evidence, waiting for a question, running a team, or safe to sign off.

The harness should measure and inspect that separately, even if only through
curated signoff scenarios and human review notes.

## Compatibility should stay constrained

One of the better decisions in the harness is refusing to let compatibility
become authority. The config docs accept many OpenCode-shaped fields only as
inactive compatibility input, while active `server`, configured `command`,
`plugin`, sharing, and updater behavior remain rejected (`docs/config.md`). That
will disappoint anyone expecting arbitrary OpenCode plugin semantics, but it is
the correct tradeoff for this harness.

Plugins and hooks are seductive because they make a product look extensible.
They are also where authority leaks. OMX itself uses hooks and `.omx/` state
because it is layered around an existing CLI. Opencode has commands/plugins as
part of its ecosystem. Pi Rust invests heavily in capability-gated extension
hostcalls, trust lifecycle, kill switches, command mediation, runtime risk
ledgers, and extension conformance (`inspirations/pi_agent_rust/README.md`,
`inspirations/pi_agent_rust/docs/extension-architecture.md`). Senpi similarly
keeps its harness layer and extension surfaces deliberate, with state snapshots,
queued writes, operation phases, and warnings about raw hook reentrancy
(`inspirations/senpi/packages/agent/docs/agent-harness.md`).

The shared lesson is not "load every plugin." It is "if you expose extension
power, make capability, phase, settlement, and provenance explicit." For this
harness, that means OpenCode plugin/command compatibility should remain
manifest-only, disabled, or carefully translated until it can be expressed as
coordinator-owned tool/evidence/workflow events. A plugin should not get to
become a second scheduler. A hook should not get to mutate hidden workflow state
behind replay's back. A configured command should not bypass the same permission
and evidence model the rest of the harness uses.

If there is a future extension story, Pi Rust is probably the better inspiration
than raw OpenCode plugin loading: capability-gated host connectors, trusted host
side, untrusted extension side, explicit policies, telemetry, kill switches, and
claim-gated conformance (`inspirations/pi_agent_rust/docs/extension-architecture.md`).
Even then, the harness should ask whether the extension enriches the operator
workflow or merely adds another place for hidden behavior to live.

## Other inspirations, selectively

Senpi is useful as a warning and an encouragement. It describes itself as a
light version of OMO that keeps the surface close to upstream Pi while adding
opinionated builtin extensions, dynamic prompts, compaction, permission systems,
todo tools, and parallel tool routing (`inspirations/senpi/README.md`). The
valuable lesson is restraint: keep the core small, track deviations, and make
additions earn their place. The harness should not become a museum of every
interesting agent feature in `inspirations/`. It should extract only the ideas
that serve the operator cockpit.

Senpi's `AgentHarness` docs are also relevant because they separate harness
config, turn snapshots, session persistence, pending writes, and operation
phases (`inspirations/senpi/packages/agent/docs/agent-harness.md`). That maps to
the same concern as this Rust harness: hooks, listeners, config setters, and
session writes must not corrupt in-flight turns or reorder persisted state. The
exact TypeScript implementation is not the point. The point is phase-aware
mutation semantics.

Pi Rust is useful for its Rust-native seriousness: single binary, explicit
runtime substrate, no unsafe code, session persistence, process cleanup,
capability-gated extension runtime, evidence artifacts, and claim-integrity
discipline (`inspirations/pi_agent_rust/README.md`). Some of Pi Rust's scope is
far beyond what this harness needs. But the posture is relevant: performance and
security claims are tied to artifacts, extension behavior is measured, and
compatibility is not treated as magic. For the harness, the lesson is to keep the
Rust core boringly authoritative while making the operator surface feel fast and
alive.

Opencode/Shuvcode gives the shell. OMX gives the workflow soul. Senpi and Pi
Rust give cautionary engineering instincts: minimal core, explicit extension
seams, strict capability boundaries, observable state, and evidence-backed
claims. OMO gives a historical map of powerful machinery, but not the product
center.

## The shape I would want to feel as a user

I would want to launch the harness and immediately feel that I am in one
operator session. The composer is primary. The transcript is primary. The
sidebar tells me the active state without demanding a command. If no workflow is
active, it says so and suggests normal next moves. If a workflow is active, it
names it. If Plan is active, it shows the plan file and the return path. If a
team is active, it shows worker lanes and pending tasks. If a continuation loop
is active, it shows iteration, evidence, and stop/cancel controls. If a question
blocks progress, it is impossible to miss. If closeout is blocked, the blocker is
specific.

I would want slash commands to feel like a palette of typed intentions. `/status`
should answer "what is going on?" `/workflow-status` should answer "what is the
workflow state?" `/signoff` should say why it can or cannot approve. `/dossier`
should feel like an evidence export, not a magic completion ritual. `/team`
should feel like an explicit escalation. `/plan-consensus` should feel like an
intentional planning branch. Staged commands should either be hidden or be
labeled as not yet workflow-owned, matching the inventory's honesty
(`docs/harness-omx-workflow-inventory.md`).

I would want the system to be comfortable saying "not ready." OMX's best trait
is that it distrusts premature completion. The harness should adopt that
temperament. A workflow with failed QA evidence is not ready. A workflow with an
unanswered blocking question is not ready. A workflow that has not exported or
recorded required dossier evidence is not ready. A team with in-progress worker
tasks is not ready for shutdown unless the operator explicitly aborts. A
continuation loop without fresh verification is not ready to call itself done.

I would also want the system to be interruptible. Single-operator does not mean
single-threaded or rigid. It means authority is legible. The operator can pause,
cancel, redirect, ask for Plan, spawn a team, record a waiver, or inspect a
dossier. Those actions should become events. The UI should reflect them. Replay
should explain them later.

## The architectural north star

The north star is not "copy OMX." The north star is "make OMX's discipline
native." Every important workflow fact should become a coordinator-owned event
or replay-derived projection. Every important external inspiration should be
filtered through the harness invariants:

- events are the source of truth;
- replay stays side-effect free;
- the coordinator owns scheduling and permission resolution;
- tool outputs and artifacts are capped, redacted, and inspectable;
- config compatibility does not grant execution authority;
- TUI rendering presents structured state rather than opaque dumps;
- deterministic lanes produce evidence proportional to the claim.

That gives the project a clean way to absorb inspiration without becoming a
forked clone of anything. Opencode's file references, command palette,
keybindings, session/model dialogs, sidebar affordances, and terminal-first
confidence can become harness TUI affordances. OMX's deep-interview, ralplan,
ralph, ultrawork, team, ultragoal, HUD, wiki, and doctor concepts can become
workflow semantics, evidence categories, projections, and operator-visible
states. Pi Rust's extension security can inform any future extension seam.
Senpi's minimal-core fork discipline can keep scope from sprawling.

The harness is closest where it already uses evented authority instead of prompt
theater. It is furthest where the user must infer workflow state from docs,
hidden commands, or generic status output. That is the main gap between "the
architecture can support this" and "the operator feels it working."

## Closing reflection

The project feels like it has passed the hardest conceptual fork. It no longer
needs to become OMO. It does not need many main agents competing for the user's
mental model. It needs one visible operator shell with explicit, powerful,
honest escalation. It needs Opencode's composure and terminal product sense. It
needs OMX's suspicion of premature completion. It needs its own Rust evented core
to stay the authority.

The work ahead is therefore less about collecting feature names and more about
making state legible. A user should be able to see, at a glance, whether the
harness is idle, planning, executing, waiting, blocked, verifying, ready for
signoff, or deliberately cancelled. They should know which commands are mature,
which are staged, and which are compatibility fossils. They should trust that a
dossier is evidence, a signoff is a guarded decision, a team is an explicit
escalation, and a continuation loop is not allowed to declare victory without
proof.

If the harness can make that feel natural, then the combination becomes
coherent: Opencode's UI/UX as the hand on the controls, OMX's workflow as the
discipline behind every move, and the harness coordinator/event model as the
machine that keeps the whole thing honest.

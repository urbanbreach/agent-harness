// allow: SIZE_OK — dynamic prompt context (variable interpolation + asset resolution + template assembly)
use crate::UnwrapOrAbort;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use harness_core::config::ResolvedModelTarget;
use harness_core::model_resolution::PromptFamily;
use harness_core::workspace::WorkspaceEnvironment;

#[derive(Clone, Copy)]
pub struct DynamicPromptContext<'a> {
    pub configured_prompt: Option<&'a str>,
    pub model: &'a ResolvedModelTarget,
    pub instruction_prompt: Option<&'a str>,
    pub skill_tool_enabled: bool,
}

#[derive(Clone, Copy)]
pub struct DynamicPromptEnvironment<'a> {
    pub workspace: &'a WorkspaceEnvironment,
    pub platform: &'a str,
    pub today: &'a str,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptSectionModule {
    pub name: &'static str,
    pub purpose: &'static str,
}

#[cfg(test)]
pub const PROMPT_SECTION_MODULES: [PromptSectionModule; 6] = [
    PromptSectionModule {
        name: "base_model",
        purpose: "base provider-family prompt or configured prompt override",
    },
    PromptSectionModule {
        name: "delegation_reminder",
        purpose: "task sync/background behavior and background_output guidance",
    },
    PromptSectionModule {
        name: "project_instructions",
        purpose: "AGENTS.md and configured project instruction prompt",
    },
    PromptSectionModule {
        name: "skill_guidance",
        purpose: "skill tool progressive-disclosure reminder",
    },
    PromptSectionModule {
        name: "intent_gate",
        purpose: "primary prompt section requiring interpreted intent before ambiguous tool use",
    },
    PromptSectionModule {
        name: "environment",
        purpose: "workspace, model, platform, date, and git context",
    },
];

#[cfg(test)]
pub fn registered_prompt_sections() -> &'static [PromptSectionModule] {
    &PROMPT_SECTION_MODULES
}

pub fn render_prompt_section_with_environment(
    name: &str,
    ctx: DynamicPromptContext<'_>,
    environment: DynamicPromptEnvironment<'_>,
) -> Option<String> {
    match name {
        "base_model" => Some(
            ctx.configured_prompt
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| {
                    provider_prompt(
                        ctx.model.resolution.prompt_family,
                        &environment.workspace.workspace_root,
                    )
                }),
        ),
        "environment" => Some(environment_prompt(ctx.model, environment)),
        "delegation_reminder" => Some(task_delegation_prompt().to_string()),
        "project_instructions" => ctx.instruction_prompt.map(ToOwned::to_owned),
        "skill_guidance" => ctx.skill_tool_enabled.then(skills_prompt),
        "intent_gate" => Some(intent_gate_prompt().to_string()),
        _ => None,
    }
}

pub fn compose(ctx: DynamicPromptContext<'_>) -> String {
    let workspace = WorkspaceEnvironment::current();
    let today = today_date_string();
    compose_with_environment(
        ctx,
        DynamicPromptEnvironment {
            workspace: &workspace,
            platform: std::env::consts::OS,
            today: &today,
        },
    )
}

pub fn compose_with_environment(
    ctx: DynamicPromptContext<'_>,
    environment: DynamicPromptEnvironment<'_>,
) -> String {
    let sections = [
        "base_model",
        "delegation_reminder",
        "project_instructions",
        "skill_guidance",
        "environment",
    ]
    .into_iter()
    .filter_map(|name| render_prompt_section_with_environment(name, ctx, environment))
    .collect::<Vec<_>>();

    sections.join("\n\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PromptFamilyAssetStatus {
    pub family: &'static str,
    pub status: &'static str,
    pub source: &'static str,
    pub path: Option<PathBuf>,
    pub warning: Option<String>,
}

pub(crate) fn prompt_family_asset_status(
    prompt_family: PromptFamily,
    workspace_root: &Path,
) -> PromptFamilyAssetStatus {
    let family = prompt_family.id();
    let Some(file_name) = prompt_family.data_asset_file() else {
        return PromptFamilyAssetStatus {
            family,
            status: "builtin",
            source: "rust_builtin_prompt",
            path: None,
            warning: None,
        };
    };
    let relative = Path::new(".agent-harness")
        .join("prompt-families")
        .join(file_name);
    let path = workspace_root.join(&relative);
    match std::fs::read_to_string(&path) {
        Ok(body) if !body.trim().is_empty() => PromptFamilyAssetStatus {
            family,
            status: "available",
            source: "data_asset",
            path: Some(relative),
            warning: None,
        },
        _ => PromptFamilyAssetStatus {
            family,
            status: "fallback",
            source: "default_prompt_fallback",
            path: Some(relative.clone()),
            warning: Some(format!(
                "missing or empty prompt-family asset {}; using default prompt",
                relative.display()
            )),
        },
    }
}

#[cfg(test)]
pub fn family_prompt_asset_families() -> &'static [PromptFamily] {
    PromptFamily::data_asset_families()
}

#[cfg(test)]
pub fn render_family_prompt_for_test(prompt_family: PromptFamily, workspace_root: &Path) -> String {
    provider_prompt(prompt_family, workspace_root)
}

fn provider_prompt(prompt_family: PromptFamily, workspace_root: &Path) -> String {
    if let Some(file_name) = prompt_family.data_asset_file() {
        let path = workspace_root
            .join(".agent-harness")
            .join("prompt-families")
            .join(file_name);
        return std::fs::read_to_string(&path)
            .ok()
            .filter(|body| !body.trim().is_empty())
            .unwrap_or_else(|| PROMPT_DEFAULT.to_string());
    }
    match prompt_family {
        PromptFamily::Reasoning => PROMPT_REASONING.to_string(),
        PromptFamily::Codex => PROMPT_CODEX.to_string(),
        PromptFamily::Gpt => PROMPT_GPT.to_string(),
        PromptFamily::Default
        | PromptFamily::Anthropic
        | PromptFamily::Gemini
        | PromptFamily::Kimi
        | PromptFamily::Trinity => PROMPT_DEFAULT.to_string(),
    }
}

fn environment_prompt(
    model: &ResolvedModelTarget,
    environment: DynamicPromptEnvironment<'_>,
) -> String {
    let branch = environment
        .workspace
        .git_branch
        .as_deref()
        .map(|branch| format!("\n  Git branch: {branch}"))
        .unwrap_or_default();
    format!(
        "You are powered by the model named {model_name}. The exact model ID is {provider}/{model_name}\nHere is some useful information about the environment you are running in:\n<env>\n  Working directory: {cwd}\n  Workspace root folder: {worktree}\n  Is directory a git repo: {is_git}{branch}\n  Platform: {platform}\n  Today's date: {date}\n</env>",
        model_name = model.model,
        provider = model.provider,
        cwd = environment.workspace.working_directory.display(),
        worktree = environment.workspace.workspace_root.display(),
        is_git = if environment.workspace.is_git_repository {
            "yes"
        } else {
            "no"
        },
        platform = environment.platform,
        date = environment.today,
    )
}

fn task_delegation_prompt() -> &'static str {
    "Task delegation reminder: if the `task` tool is available, use a structured delegation body with context, goal, downstream use, request, required tools, must-do, and must-not-do. `run_in_background=false` is synchronous; the child result returns directly in the current tool response and no `[BACKGROUND TASK ...]` reminder is emitted. Use `run_in_background=true` when testing background subagents, wakeups, completion reminders, or `background_output`. For `run_in_background=true`, use `background_output` for interim status checks, user-requested interim updates, or `cancel=true` anytime; for the final background result, wait for the coordinator/system completion notification before retrieving with `background_output`."
}

fn intent_gate_prompt() -> &'static str {
    "## Intent Gate\nBefore tool use on an ambiguous request, state the interpreted intent, then route to exactly one of: explain, investigate, implement, plan, or ask exactly one blocking question."
}

fn today_date_string() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let weekday = weekday_name((days + 4).rem_euclid(7));
    format!(
        "{weekday} {month} {day:02} {year}",
        month = month_name(month),
    )
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 }.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era = (day_of_era - day_of_era.div_euclid(1_460) + day_of_era.div_euclid(36_524)
        - day_of_era.div_euclid(146_096))
    .div_euclid(365);
    let mut year = year_of_era + era * 400;
    let day_of_year =
        day_of_era - (365 * year_of_era + year_of_era.div_euclid(4) - year_of_era.div_euclid(100));
    let month_prime = (5 * day_of_year + 2).div_euclid(153);
    let day = day_of_year - (153 * month_prime + 2).div_euclid(5) + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (
        year,
        u32::try_from(month).unwrap_or(0),
        u32::try_from(day).unwrap_or(0),
    )
}

fn weekday_name(index: i64) -> &'static str {
    match index {
        0 => "Sun",
        1 => "Mon",
        2 => "Tue",
        3 => "Wed",
        4 => "Thu",
        5 => "Fri",
        _ => "Sat",
    }
}

fn month_name(month: u32) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        _ => "Dec",
    }
}

fn skills_prompt() -> String {
    [
        "Skills provide specialized instructions and workflows for specific tasks.",
        "Use the `skill` tool to load a skill when a task matches its description.",
        "The `skill` tool description lists available project and global skills with their descriptions. Call `skill` with one exact `name`; wildcards are not skill names.",
    ]
    .join("\n")
}

const PROMPT_GPT: &str = r#"You are agent-harness, You and the user share the same workspace and collaborate to achieve the user's goals.

You are a deeply pragmatic, effective software engineer. You take engineering quality seriously, and collaboration comes through as direct, factual statements. You communicate efficiently, keeping the user clearly informed about ongoing actions without unnecessary detail. You build context by examining the codebase first without making assumptions or jumping to conclusions. You think through the nuances of the code you encounter, and embody the mentality of a skilled senior software engineer.

- When searching for text or files, prefer using `glob` and `grep` tools. Use `list` for directory trees and `read` for file contents.
- Parallelize tool calls whenever possible - especially file reads. Use the `batch` tool for independent parallel tool calls. Never chain together `bash` commands with separators like `echo "====";` as this renders to the user poorly.

## Editing Approach

- The best changes are often the smallest correct changes.
- When you are weighing two correct approaches, prefer the more minimal one (less new names, helpers, tests, etc).
- Keep things in one function unless composable or reusable
- Do not add backward-compatibility code unless there is a concrete need, such as persisted data, shipped behavior, external consumers, or an explicit user requirement; if unclear, ask one short question instead of guessing.

## Autonomy and persistence

Unless the user explicitly asks for a plan, asks a question about the code, is brainstorming potential solutions, or some other intent that makes it clear that code should not be written, assume the user wants you to make code changes or run tools to solve the user's problem. In these cases, it's bad to output your proposed solution in a message, you should go ahead and actually implement the change. If you encounter challenges or blockers, you should attempt to resolve them yourself.

Persist until the task is fully handled end-to-end within the current turn whenever feasible: do not stop at analysis or partial fixes; carry changes through implementation, verification, and a clear explanation of outcomes unless the user explicitly pauses or redirects you.

If you notice unexpected changes in the worktree or staging area that you did not make, continue with your task. NEVER revert, undo, or modify changes you did not make unless the user explicitly asks you to. There can be multiple agents or the user working in the same codebase concurrently.

## Editing constraints

- Default to ASCII when editing or creating files. Only introduce non-ASCII or other Unicode characters when there is a clear justification and the file already uses them.
- Add succinct code comments that explain what is going on if code is not self-explanatory. You should not add comments like "Assigns the value to the variable", but a brief comment might be useful ahead of a complex code block that the user would otherwise have to spend time parsing out. Usage of these comments should be rare.
- Always use the `edit` tool for manual code edits. Do not use `cat` or any other commands when creating or editing files. Formatting commands or bulk edits can be run with `bash` when appropriate.
- Do not use Python to read/write files when `read` or `edit` would suffice.
- You may be in a dirty git worktree.
  * NEVER revert existing changes you did not make unless explicitly requested, since these changes were made by the user.
  * If asked to make a commit or code edits and there are unrelated changes to your work or changes that you didn't make in those files, don't revert those changes.
  * If the changes are in files you've touched recently, you should read carefully and understand how you can work with the changes rather than reverting them.
  * If the changes are in unrelated files, just ignore them and don't revert them.
- Do not amend a commit unless explicitly requested to do so.
- While you are working, you might notice unexpected changes that you didn't make. It's likely the user made them, or were autogenerated. If they directly conflict with your current task, stop and ask the user how they would like to proceed. Otherwise, focus on the task at hand.
- **NEVER** use destructive commands like `git reset --hard` or `git checkout --` unless specifically requested or approved by the user.
- You struggle using the git interactive console. **ALWAYS** prefer using non-interactive git commands.

## Special user requests

If the user makes a simple request (such as asking for the time) which you can fulfill by running a terminal command (such as `date`), you should do so.

If the user pastes an error description or a bug report, help them diagnose the root cause. You can try to reproduce it if it seems feasible with the available tools and skills.

If the user asks for a "review", default to a code review mindset: prioritise identifying bugs, risks, behavioural regressions, and missing tests. Findings must be the primary focus of the response - keep summaries or overviews brief and only after enumerating the issues. Present findings first (ordered by severity with file/line references), follow with open questions or assumptions, and offer a change-summary only as a secondary detail. If no findings are discovered, state that explicitly and mention any residual risks or testing gaps.

## Frontend tasks

When doing frontend design tasks, avoid collapsing into generic or safe, average-looking layouts.
- Ensure the page loads properly on both desktop and mobile
- For React code, prefer modern patterns including useEffectEvent, startTransition, and useDeferredValue when appropriate if already used locally. Do not add useMemo/useCallback by default unless already used; follow the repo's React Compiler guidance.
- Overall: Avoid boilerplate layouts and interchangeable UI patterns. Vary themes, type families, and visual languages across outputs.

Exception: If working within an existing website or design system, preserve the established patterns, structure, and visual language.

# Working with the user

## General

Do not begin responses with conversational interjections or meta commentary. Avoid openers such as acknowledgements ("Done —", "Got it", "Great question, ") or framing phrases.

Balance conciseness to not overwhelm the user with appropriate detail for the request. Do not narrate abstractly; explain what you are doing and why.

Never tell the user to "save/copy this file", the user is on the same machine and has access to the same files as you have.


## Formatting rules

Your responses are rendered as GitHub-flavored Markdown.

Never use nested bullets. Keep lists flat (single level). If you need hierarchy, split into separate lists or sections or if you use : just include the line you might usually render using a nested bullet immediately after it. For numbered lists, only use the `1. 2. 3.` style markers (with a period), never `1)`.

Headers are optional, only use them when you think they are necessary. If you do use them, use short Title Case (1-3 words) wrapped in **…**. Don't add a blank line.

Use inline code blocks for commands, paths, environment variables, function names, inline examples, keywords.

Code samples or multi-line snippets should be wrapped in fenced code blocks. Include a language tag when possible.

Don’t use emojis or em dashes unless explicitly instructed.

## Response channels

Use commentary for short progress updates while working and final for the completed response.

### `commentary` channel

Only use `commentary` for intermediary updates. These are short updates while you are working, they are NOT final answers. Keep updates brief to communicate progress and new information to the user as you are doing work.

Send updates when they add meaningful new information: a discovery, a tradeoff, a blocker, a substantial plan, or the start of a non-trivial edit or verification step.

Do not narrate routine reads, searches, obvious next steps, or minor confirmations. Combine related progress into a single update.

Do not begin responses with conversational interjections or meta commentary. Avoid openers such as acknowledgements ("Done —", "Got it", "Great question") or framing phrases.

Before substantial work, send a short update describing your first step. Before editing files, send an update describing the edit.

After you have sufficient context, and the work is substantial you can provide a longer plan (this is the only user update that may be longer than 2 sentences and can contain formatting).

### `final` channel

Use final for the completed response.

Structure your final response if necessary. The complexity of the answer should match the task. If the task is simple, your answer should be a one-liner. Order sections from general to specific to supporting.

If the user asks for a code explanation, include code references. For simple tasks, just state the outcome without heavy formatting.

For large or complex changes, lead with the solution, then explain what you did and why. For casual chat, just chat. If something couldn’t be done (tests, builds, etc.), say so. Suggest next steps only when they are natural and useful; if you list options, use numeric lists.
"#;

const PROMPT_CODEX: &str = r#"You are agent-harness, the best coding agent on the planet.

You are an interactive CLI tool that helps users with software engineering tasks. Use the instructions below and the tools available to you to assist the user.

## Editing constraints
- Default to ASCII when editing or creating files. Only introduce non-ASCII or other Unicode characters when there is a clear justification and the file already uses them.
- Only add comments if they are necessary to make a non-obvious block easier to understand.
- Use `edit` for manual file edits, but it is fine to use `bash` for generated changes, formatting commands, or bulk scripted transformations when that is more efficient.

## Tool usage
- Prefer specialized tools over shell for file operations:
  - Use `read` to view files and directories, and `edit` to create, modify, rename, or delete files.
  - Use `glob` to find files by name and `grep` to search file contents.
- Use `bash` for terminal operations (git, bun, builds, tests, running scripts).
- Run tool calls in parallel when neither call needs the other’s output; otherwise run sequentially.

## Git and workspace hygiene
- You may be in a dirty git worktree.
    * NEVER revert existing changes you did not make unless explicitly requested, since these changes were made by the user.
    * If asked to make a commit or code edits and there are unrelated changes to your work or changes that you didn't make in those files, don't revert those changes.
    * If the changes are in files you've touched recently, you should read carefully and understand how you can work with the changes rather than reverting them.
    * If the changes are in unrelated files, just ignore them and don't revert them.
- Do not amend commits unless explicitly requested.
- **NEVER** use destructive commands like `git reset --hard` or `git checkout --` unless specifically requested or approved by the user.

## Frontend tasks
When doing frontend design tasks, avoid collapsing into bland, generic layouts.
Aim for interfaces that feel intentional and deliberate.
- Typography: Use expressive, purposeful fonts and avoid default stacks (Inter, Roboto, Arial, system).
- Color & Look: Choose a clear visual direction; define CSS variables; avoid purple-on-white defaults. No purple bias or dark mode bias.
- Motion: Use a few meaningful animations (page-load, staggered reveals) instead of generic micro-motions.
- Background: Don't rely on flat, single-color backgrounds; use gradients, shapes, or subtle patterns to build atmosphere.
- Overall: Avoid boilerplate layouts and interchangeable UI patterns. Vary themes, type families, and visual languages across outputs.
- Ensure the page loads properly on both desktop and mobile.

Exception: If working within an existing website or design system, preserve the established patterns, structure, and visual language.

## Presenting your work and final message

You are producing plain text that will later be styled by the CLI. Follow these rules exactly. Formatting should make results easy to scan, but not feel mechanical. Use judgment to decide how much structure adds value.

- Default: be very concise; friendly coding partner tone.
- Default: do the work without asking questions. Treat short tasks as sufficient direction; infer missing details by reading the codebase and following existing conventions.
- Questions: only ask when you are truly blocked after checking relevant context AND you cannot safely pick a reasonable default. This usually means one of:
  * The request is ambiguous in a way that materially changes the result and you cannot disambiguate by reading the repo.
  * The action is destructive/irreversible, touches production, or changes billing/security posture.
  * You need a secret/credential/value that cannot be inferred (API key, account id, etc.).
- If you must ask: do all non-blocked work first, then ask exactly one targeted question, include your recommended default, and state what would change based on the answer.
- Never ask permission questions like "Should I proceed?" or "Do you want me to run tests?"; proceed with the most reasonable option and mention what you did.
- For substantial work, summarize clearly; follow final‑answer formatting.
- Skip heavy formatting for simple confirmations.
- Don't dump large files you've written; reference paths only.
- No "save/copy this file" - User is on the same machine.
- Offer logical next steps (tests, commits, build) briefly; add verify steps if you couldn't do something.
- For code changes:
  * Lead with a quick explanation of the change, and then give more details on the context covering where and why a change was made. Do not start this explanation with "summary", just jump right in.
  * If there are natural next steps the user may want to take, suggest them at the end of your response. Do not make suggestions if there are no natural next steps.
  * When suggesting multiple options, use numeric lists for the suggestions so the user can quickly respond with a single number.
- The user does not command execution outputs. When asked to show the output of a command (e.g. `git show`), relay the important details in your answer or summarize the key lines so the user understands the result.

## Final answer structure and style guidelines

- Plain text; CLI handles styling. Use structure only when it helps scannability.
- Headers: optional; short Title Case (1-3 words) wrapped in **…**; no blank line before the first bullet; add only if they truly help.
- Bullets: use - ; merge related points; keep to one line when possible; 4–6 per list ordered by importance; keep phrasing consistent.
- Monospace: backticks for commands/paths/env vars/code ids and inline examples; use for literal keyword bullets; never combine with **.
- Code samples or multi-line snippets should be wrapped in fenced code blocks; include an info string as often as possible.
- Structure: group related bullets; order sections general → specific → supporting; for subsections, start with a bolded keyword bullet, then items; match complexity to the task.
- Tone: collaborative, concise, factual; present tense, active voice; self‑contained; no "above/below"; parallel wording.
- Don'ts: no nested bullets/hierarchies; no ANSI codes; don't cram unrelated keywords; keep keyword lists short—wrap/reformat if long; avoid naming formatting styles in answers.
- Adaptation: code explanations → precise, structured with code refs; simple tasks → lead with outcome; big changes → logical walkthrough + rationale + next actions; casual one-offs → plain sentences, no headers/bullets.
- File References: When referencing files in your response follow the below rules:
  * Use inline code to make file paths clickable.
  * Each reference should have a stand alone path. Even if it's the same file.
  * Accepted: absolute, workspace‑relative, a/ or b/ diff prefixes, or bare filename/suffix.
  * Optionally include line/column (1‑based): :line[:column] or #Lline[Ccolumn] (column defaults to 1).
  * Do not use URIs like file://, vscode://, or https://.
  * Do not provide range of lines
  * Examples: src/app.ts, src/app.ts:42, b/server/index.js#L10, C:\repo\project\main.rs:12:5
"#;

const PROMPT_DEFAULT: &str = r#"You are agent-harness, an interactive CLI tool that helps users with software engineering tasks. Use the instructions below and the tools available to you to assist the user.

IMPORTANT: You must NEVER generate or guess URLs for the user unless you are confident that the URLs are for helping the user with programming. You may use URLs provided by the user in their messages or local files.

If the user asks for help or wants to give feedback inform them of the following:
- /help: Get help with using agent-harness
- To give feedback, users should use the project issue tracker.

When the user directly asks about agent-harness (eg 'can agent-harness do...', 'does agent-harness have...') or asks in second person (eg 'are you able...', 'can you do...'), first use the available documentation and workspace files to answer the question accurately.

# Tone and style
You should be concise, direct, and to the point. When you run a non-trivial bash command, you should explain what the command does and why you are running it, to make sure the user understands what you are doing (this is especially important when you are running a command that will make changes to the user's system).
Remember that your output will be displayed on a command line interface. Your responses can use GitHub-flavored markdown for formatting, and will be rendered in a monospace font using the CommonMark specification.
Output text to communicate with the user; all text you output outside of tool use is displayed to the user. Only use tools to complete tasks. Never use tools like `bash` or code comments as means to communicate with the user during the session.
If you cannot or will not help the user with something, please do not say why or what it could lead to, since this comes across as preachy and annoying. Please offer helpful alternatives if possible, and otherwise keep your response to 1-2 sentences.
Only use emojis if the user explicitly requests it. Avoid using emojis in all communication unless asked.
IMPORTANT: You should minimize output tokens as much as possible while maintaining helpfulness, quality, and accuracy. Only address the specific query or task at hand, avoiding tangential information unless absolutely critical for completing the request. If you can answer in 1-3 sentences or a short paragraph, please do.
IMPORTANT: You should NOT answer with unnecessary preamble or postamble (such as explaining your code or summarizing your action), unless the user asks you to.
IMPORTANT: Keep your responses short, since they will be displayed on a command line interface. You MUST answer concisely with fewer than 4 lines (not including tool use or code generation), unless user asks for detail. Answer the user's question directly, without elaboration, explanation, or details. One word answers are best.

# Proactiveness
You are allowed to be proactive, but only when the user asks you to do something. You should strive to strike a balance between doing the right thing when asked and not surprising the user with actions you take without asking.

# Following conventions
When making changes to files, first understand the file's code conventions. Mimic code style, use existing libraries and utilities, and follow existing patterns.
- NEVER assume that a given library is available, even if it is well known. Whenever you write code that uses a library or framework, first check that this codebase already uses the given library.
- When you create a new component, first look at existing components to see how they're written; then consider framework choice, naming conventions, typing, and other conventions.
- When you edit a piece of code, first look at the code's surrounding context (especially its imports) to understand the code's choice of frameworks and libraries.
- Always follow security best practices. Never introduce code that exposes or logs secrets and keys. Never commit secrets or keys to the repository.

# Code style
- IMPORTANT: DO NOT ADD ***ANY*** COMMENTS unless asked

# Doing tasks
The user will primarily request you perform software engineering tasks. This includes solving bugs, adding new functionality, refactoring code, explaining code, and more. For these tasks the following steps are recommended:
- Use the available search tools to understand the codebase and the user's query. You are encouraged to use the search tools extensively both in parallel and sequentially.
- Implement the solution using all tools available to you
- Verify the solution if possible with tests. NEVER assume specific test framework or test script. Check the README or search codebase to determine the testing approach.
- VERY IMPORTANT: When you have completed a task, you MUST run the lint and typecheck commands (e.g. npm run lint, npm run typecheck, ruff, etc.) with `bash` if they were provided to you to ensure your code is correct. If you are unable to find the correct command, ask the user for the command to run and if they supply it, proactively suggest writing it to AGENTS.md so that you will know to run it next time.
NEVER commit changes unless the user explicitly asks you to.

- Tool results and user messages may include <system-reminder> tags. <system-reminder> tags contain useful information and reminders. They are NOT part of the user's provided input or the tool result.

# Tool usage policy
- When doing broad codebase exploration, prefer to use the `task` tool in order to reduce context usage when suitable agent profiles are configured.
- You have the capability to call multiple tools in a single response. When multiple independent pieces of information are requested, batch your tool calls together for optimal performance.

IMPORTANT: Before you begin work, think about what the code you're editing is supposed to do based on the filenames directory structure.

# Code References

When referencing specific functions or pieces of code include the pattern `file_path:line_number` to allow the user to easily navigate to the source code location.
"#;

const PROMPT_REASONING: &str = r#"You are the Harness reasoning prompt for models that benefit from deliberate planning.

Work from the repository first: read the relevant code, identify the smallest correct change, implement it surgically, and verify it through the closest real surface. Use external documentation only when the request or dependency behavior requires current outside context.

Keep user-facing updates brief. Before non-trivial tool use, state the immediate action in one concise sentence. Do not stop at analysis when the user asked for implementation.

Preserve existing behavior unless the user requested a behavior change. Prefer clear, typed, maintainable code over broad rewrites, speculative abstractions, or defensive fallbacks that the current contracts do not require.

When changing code, run the focused tests or checks that prove the affected behavior. If a check is unavailable or pre-existing failures block a full gate, report that limitation explicitly.
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    fn model(model: &str) -> ResolvedModelTarget {
        let resolution = harness_core::model_resolution::resolve_model(
            harness_core::model_resolution::ModelResolutionInput {
                provider: "default",
                model,
                metadata_family: None,
                input_modalities: &[],
                context_window_tokens: None,
                max_input_tokens: None,
                max_output_tokens: None,
                supports_tool_calls: None,
                supports_reasoning_summaries: None,
            },
        );
        ResolvedModelTarget {
            model_ref: format!("default:{model}"),
            provider: "default".to_string(),
            model: model.to_string(),
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            thinking: None,
            resolution,
        }
    }

    fn model_with_metadata_family(model_id: &str, family: &str) -> ResolvedModelTarget {
        let mut target = model(model_id);
        target.resolution = harness_core::model_resolution::resolve_model(
            harness_core::model_resolution::ModelResolutionInput {
                provider: "github-copilot",
                model: model_id,
                metadata_family: Some(family),
                input_modalities: &[],
                context_window_tokens: None,
                max_input_tokens: None,
                max_output_tokens: None,
                supports_tool_calls: None,
                supports_reasoning_summaries: None,
            },
        );
        target.provider = "github-copilot".to_string();
        target.model_ref = format!("github-copilot:{model_id}");
        target
    }

    #[test]
    fn provider_prompt_uses_gpt_prompt_for_gpt_models() {
        let prompt = compose(DynamicPromptContext {
            configured_prompt: None,
            model: &model("gpt-5.4-mini"),
            instruction_prompt: None,
            skill_tool_enabled: false,
        });
        assert!(prompt.starts_with("You are agent-harness, You and the user"));
        assert!(prompt.contains("The exact model ID is default/gpt-5.4-mini"));
    }

    #[test]
    fn provider_prompt_uses_resolved_metadata_family_not_model_substrings() {
        let prompt = compose(DynamicPromptContext {
            configured_prompt: None,
            model: &model_with_metadata_family("enterprise-alpha", "gemini-pro"),
            instruction_prompt: None,
            skill_tool_enabled: false,
        });

        assert!(prompt.starts_with("# Harness Prompt Family: gemini"));
        assert!(prompt.contains("The exact model ID is github-copilot/enterprise-alpha"));
    }

    #[test]
    fn family_prompt_missing_asset_falls_back_to_default_with_status_warning() {
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let prompt = render_family_prompt_for_test(PromptFamily::Gemini, temp_dir.path());
        let status = prompt_family_asset_status(PromptFamily::Gemini, temp_dir.path());

        assert!(prompt.starts_with("You are agent-harness, an interactive CLI tool"));
        assert_eq!(status.status, "fallback");
        assert_eq!(status.source, "default_prompt_fallback");
        assert!(status
            .warning
            .as_deref()
            .unwrap_or_abort()
            .contains(".agent-harness/prompt-families/gemini.md"));
    }

    #[test]
    fn family_prompt_assets_are_structured_branding_free_and_tool_safe() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let forbidden = [
            "reference implementation",
            "model reference",
            "model reference",
            "claude code",
            "gemini cli",
            "todowrite",
            "todoread",
        ];

        for family in family_prompt_asset_families() {
            let body = render_family_prompt_for_test(*family, &repo_root);
            assert!(
                !body.trim().is_empty(),
                "{} prompt-family asset must not be empty",
                family.id()
            );
            for required in [
                "## Identity",
                "## Shared Skeleton",
                "## Harness Seams",
                "## Family Guidance",
                "## Coding Workflow",
                "## Communication",
            ] {
                assert!(
                    body.contains(required),
                    "{} prompt-family asset missing {required}",
                    family.id()
                );
            }
            let lowered = body.to_ascii_lowercase();
            for marker in forbidden {
                assert!(
                    !lowered.contains(marker),
                    "{} prompt-family asset contains forbidden marker {marker}",
                    family.id()
                );
            }
        }
    }

    #[test]
    fn dynamic_prompt_does_not_include_source_branding() {
        let prompt = compose(DynamicPromptContext {
            configured_prompt: None,
            model: &model("gpt-5.3-codex"),
            instruction_prompt: Some("Instructions from: AGENTS.md\nProject rules."),
            skill_tool_enabled: true,
        });
        assert!(!prompt.to_lowercase().contains(&["open", "code"].concat()));
        assert!(prompt.contains("Instructions from: AGENTS.md"));
        assert!(prompt.contains("Skills provide specialized instructions"));
    }

    #[test]
    fn dynamic_prompt_explains_task_background_modes() {
        let prompt = compose(DynamicPromptContext {
            configured_prompt: None,
            model: &model("gpt-5.4-mini"),
            instruction_prompt: None,
            skill_tool_enabled: false,
        });

        assert!(prompt.contains("run_in_background=false` is synchronous"));
        assert!(prompt.contains("no `[BACKGROUND TASK ...]` reminder is emitted"));
        assert!(prompt.contains("Use `run_in_background=true` when testing background subagents"));
        assert!(prompt.contains("completion notification"));
        assert!(prompt.contains("wait for the coordinator"));
        assert!(prompt.contains("interim status checks"));
        assert!(prompt.contains("`cancel=true` anytime"));
        assert!(prompt.contains("final background result"));
    }

    #[test]
    fn dynamic_prompt_preserves_v1_section_precedence() {
        // arrange
        let context = DynamicPromptContext {
            configured_prompt: Some("Runtime agent prompt."),
            model: &model("gpt-5.4-mini"),
            instruction_prompt: Some(
                "Instructions from: configured instruction\nConfig rules.\n\nInstructions from: AGENTS.md\nProject rules.",
            ),
            skill_tool_enabled: true,
        };

        // act
        let prompt = compose(context);

        // assert
        assert_section_order(&prompt, "Runtime agent prompt.", "Task delegation reminder");
        assert_section_order(
            &prompt,
            "Task delegation reminder",
            "Instructions from: configured instruction",
        );
        assert_section_order(
            &prompt,
            "Instructions from: configured instruction",
            "Instructions from: AGENTS.md",
        );
        assert_section_order(
            &prompt,
            "Instructions from: AGENTS.md",
            "Skills provide specialized instructions",
        );
        assert_section_order(
            &prompt,
            "Skills provide specialized instructions",
            "The exact model ID",
        );
    }

    #[test]
    fn dynamic_prompt_keeps_volatile_environment_at_stable_prefix_tail() {
        let workspace = WorkspaceEnvironment {
            working_directory: "/workspace/current".into(),
            workspace_root: "/workspace".into(),
            is_git_repository: true,
            git_branch: Some("feature/cache".to_string()),
        };
        let prompt = compose_with_environment(
            DynamicPromptContext {
                configured_prompt: Some("Stable base prompt."),
                model: &model("gpt-5.4-mini"),
                instruction_prompt: Some("Stable project instructions."),
                skill_tool_enabled: true,
            },
            DynamicPromptEnvironment {
                workspace: &workspace,
                platform: "linux",
                today: "Sat May 30 2026",
            },
        );

        assert_section_order(
            &prompt,
            "Stable base prompt.",
            "Stable project instructions.",
        );
        assert_section_order(
            &prompt,
            "Stable project instructions.",
            "Skills provide specialized instructions",
        );
        assert_section_order(
            &prompt,
            "Skills provide specialized instructions",
            "Git branch: feature/cache",
        );
        assert_section_order(
            &prompt,
            "Git branch: feature/cache",
            "Today's date: Sat May 30 2026",
        );
    }

    #[test]
    fn dynamic_prompt_uses_harness_tool_names() {
        for model_name in [
            "gpt-5.4-mini",
            "gpt-5.3-codex",
            "claude-sonnet-4.5",
            "gemini-2.5-pro",
            "kimi-k2",
            "trinity",
            "unknown-model",
        ] {
            let prompt = compose(DynamicPromptContext {
                configured_prompt: None,
                model: &model(model_name),
                instruction_prompt: None,
                skill_tool_enabled: true,
            });
            for stale_name in [
                "apply_patch",
                "multi_tool_use.parallel",
                "Use Read",
                "Use `write`",
                "`write`/`edit`",
                "TodoWrite",
                "TodoRead",
                "WebFetch",
                "Task tool",
            ] {
                assert!(
                    !prompt.contains(stale_name),
                    "prompt for {model_name} contains stale tool name {stale_name:?}"
                );
            }
        }
    }

    fn assert_section_order(prompt: &str, before: &str, after: &str) {
        let before_index = prompt
            .find(before)
            .unwrap_or_else(|| panic!("prompt missing {before:?}"));
        let after_index = prompt
            .find(after)
            .unwrap_or_else(|| panic!("prompt missing {after:?}"));
        assert!(
            before_index < after_index,
            "expected {before:?} before {after:?} in prompt"
        );
    }
}

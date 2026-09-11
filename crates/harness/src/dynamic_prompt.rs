// allow: SIZE_OK — dynamic prompt context (variable interpolation + asset resolution + template assembly)
use crate::UnwrapOrAbort;
use std::path::Path;
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

pub fn compose_template(
    template: &harness_core::model_resolution::ModelPromptTemplate,
    model: &ResolvedModelTarget,
    skill_tool_enabled: bool,
) -> String {
    let workspace = WorkspaceEnvironment::current();
    let today = today_date_string();
    let base = template.configured_prompt.clone().unwrap_or_else(|| {
        let mut base = model
            .resolution
            .prompt_family
            .prompt(&workspace.workspace_root);
        if let Some(role) = &template.role_prompt {
            base.push_str("\n\n");
            base.push_str(role);
        }
        base
    });
    compose_with_environment(
        DynamicPromptContext {
            configured_prompt: Some(&base),
            model,
            instruction_prompt: template.instruction_prompt.as_deref(),
            skill_tool_enabled,
        },
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

pub(crate) fn prompt_family_asset_status(
    prompt_family: PromptFamily,
    workspace_root: &Path,
) -> harness_core::model_resolution::PromptFamilyAssetStatus {
    harness_core::model_resolution::effective_prompt_status(prompt_family, None, workspace_root)
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
    prompt_family.prompt(workspace_root)
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
    "Task delegation reminder: if the `task` tool is available, select the named subagent whose documented scope matches the bounded work and use a structured delegation body with context, goal, downstream use, request, required tools, must-do, and must-not-do. `run_in_background=false` is synchronous; the child result returns directly in the current tool response and no `[BACKGROUND TASK ...]` reminder is emitted. Use `run_in_background=true` when testing background tasks, wakeups, completion reminders, or `background_output`. For `run_in_background=true`, use `background_output` for interim status checks, user-requested interim updates, or `cancel=true` anytime; for the final background result, wait for the coordinator/system completion notification before retrieving with `background_output`."
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
            limits: Default::default(),
            resolution,
            catalog_entry: None,
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
            model: &model("gpt-6-astra"),
            instruction_prompt: None,
            skill_tool_enabled: false,
        });
        assert!(prompt.starts_with(PromptFamily::Gpt.bundled_prompt()));
        assert!(prompt.contains("The exact model ID is default/gpt-6-astra"));
    }

    #[test]
    fn provider_prompt_uses_resolved_metadata_family_not_model_substrings() {
        let prompt = compose(DynamicPromptContext {
            configured_prompt: None,
            model: &model_with_metadata_family("enterprise-alpha", "gemini-pro"),
            instruction_prompt: None,
            skill_tool_enabled: false,
        });

        assert!(prompt.starts_with(PromptFamily::Gemini.bundled_prompt()));
        assert!(prompt.contains("The exact model ID is github-copilot/enterprise-alpha"));
    }

    #[test]
    fn family_prompt_missing_asset_uses_its_bundled_family() {
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        for family in family_prompt_asset_families() {
            let prompt = render_family_prompt_for_test(*family, temp_dir.path());
            let status = prompt_family_asset_status(*family, temp_dir.path());
            assert_eq!(prompt, family.bundled_prompt());
            assert_eq!(status.family, family.id());
            assert_eq!(status.status, "builtin");
            assert_eq!(status.source, "bundled_prompt");
            assert!(status.warning.is_none());
        }
    }

    #[test]
    fn family_prompt_workspace_override_and_empty_asset_status_match_dispatch() {
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let assets = temp_dir.path().join(".agent-harness/prompt-families");
        std::fs::create_dir_all(&assets).unwrap_or_abort();
        std::fs::write(assets.join("gemini.md"), "WORKSPACE_BASE_SENTINEL").unwrap_or_abort();
        assert_eq!(
            render_family_prompt_for_test(PromptFamily::Gemini, temp_dir.path()),
            "WORKSPACE_BASE_SENTINEL"
        );
        assert_eq!(
            prompt_family_asset_status(PromptFamily::Gemini, temp_dir.path()).source,
            "data_asset"
        );
        std::fs::write(assets.join("gemini.md"), "  ").unwrap_or_abort();
        assert_eq!(
            render_family_prompt_for_test(PromptFamily::Gemini, temp_dir.path()),
            PromptFamily::Gemini.bundled_prompt()
        );
        assert!(
            prompt_family_asset_status(PromptFamily::Gemini, temp_dir.path())
                .warning
                .is_some()
        );
    }

    #[test]
    fn dynamic_prompt_does_not_include_source_branding() {
        let prompt = compose(DynamicPromptContext {
            configured_prompt: None,
            model: &model("gpt-5.3-codex"),
            instruction_prompt: Some("Instructions from: AGENTS.md\nProject rules."),
            skill_tool_enabled: true,
        });
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
        assert!(prompt.contains("Use `run_in_background=true` when testing background tasks"));
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
            "gpt-6-astra",
            "gpt-5.3-codex",
            "claude-sonnet-4.5",
            "gemini-2.5-pro",
            "kimi-k2",
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

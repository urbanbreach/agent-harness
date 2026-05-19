use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRegistry {
    commands: Vec<CommandSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub aliases: &'static [&'static str],
    pub dollar_aliases: &'static [&'static str],
    pub surface: CommandSurface,
    pub effect: CommandEffect,
    pub action: CommandAction,
    pub availability: WorkflowCommandAvailability,
    pub enabled_by_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSurface {
    WorkflowCommand,
    ContinuationCommand,
    StagedReferenceCommand,
    PromptTemplate,
    ProfileSwitch,
    NativeTool,
    TuiAction,
}

impl CommandSurface {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkflowCommand => "workflow_command",
            Self::ContinuationCommand => "continuation_command",
            Self::StagedReferenceCommand => "staged_reference_command",
            Self::PromptTemplate => "prompt_template",
            Self::ProfileSwitch => "profile_switch",
            Self::NativeTool => "native_tool",
            Self::TuiAction => "tui_action",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandEffect {
    ReadProjection,
    MutateCoordinatorState,
    ControlContinuation,
    BlockedNoSideEffect,
    SubmitPrompt,
    SwitchProfile,
    InvokeNativeTool,
    UpdateTuiState,
}

impl CommandEffect {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadProjection => "read_projection",
            Self::MutateCoordinatorState => "mutate_coordinator_state",
            Self::ControlContinuation => "control_continuation",
            Self::BlockedNoSideEffect => "blocked_no_side_effect",
            Self::SubmitPrompt => "submit_prompt",
            Self::SwitchProfile => "switch_profile",
            Self::InvokeNativeTool => "invoke_native_tool",
            Self::UpdateTuiState => "update_tui_state",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowCommandAvailability {
    Present,
    Staged,
    Rejected,
}

impl WorkflowCommandAvailability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Staged => "staged",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAction {
    PromptTemplate {
        prompt: &'static str,
    },
    LoadSkills {
        skills: &'static [&'static str],
    },
    ProfileSwitch {
        profile: &'static str,
    },
    NativeTool {
        tool_id: &'static str,
    },
    WorkflowIntent {
        intent: WorkflowIntent,
    },
    StartContinuation {
        mode: ContinuationMode,
    },
    StopContinuation,
    BlockedWorkflow {
        reason: &'static str,
        inventory_ref: &'static str,
    },
    PlanArtifact {
        artifact: &'static str,
    },
    HandoffArtifact {
        artifact: &'static str,
    },
    TuiAction {
        action: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowIntent {
    Run,
    Status,
    Signoff,
    Cancel,
    DossierExport,
    Snapshot,
    PlanConsensus,
    GoalLedger,
    ResearchMission,
    Wiki,
}

impl WorkflowIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Run => "workflow.run",
            Self::Status => "workflow.status",
            Self::Signoff => "workflow.signoff",
            Self::Cancel => "workflow.cancel",
            Self::DossierExport => "workflow.dossier_export",
            Self::Snapshot => "workflow.snapshot",
            Self::PlanConsensus => "workflow.plan_consensus",
            Self::GoalLedger => "workflow.goal_ledger",
            Self::ResearchMission => "workflow.research_mission",
            Self::Wiki => "workflow.wiki",
        }
    }

    pub fn effect(self) -> CommandEffect {
        match self {
            Self::Status | Self::DossierExport => CommandEffect::ReadProjection,
            Self::Run
            | Self::Signoff
            | Self::Cancel
            | Self::Snapshot
            | Self::PlanConsensus
            | Self::GoalLedger
            | Self::ResearchMission
            | Self::Wiki => CommandEffect::MutateCoordinatorState,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationMode {
    Ralph,
    Ultrawork,
}

impl ContinuationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ralph => "ralph",
            Self::Ultrawork => "ultrawork",
        }
    }
}

impl CommandRegistry {
    pub fn new(commands: Vec<CommandSpec>) -> Self {
        Self { commands }
    }

    pub fn builtins() -> Self {
        Self::new(vec![
            spec(
                "workflow-run",
                "Start a coordinator-owned workflow run",
                &["workflow"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Run,
                },
            ),
            spec(
                "workflow-status",
                "Inspect projected workflow status",
                &["status-workflow"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Status,
                },
            ),
            spec(
                "workflow-signoff",
                "Record or inspect workflow signoff decisions",
                &["signoff"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Signoff,
                },
            ),
            spec(
                "workflow-cancel",
                "Cancel a coordinator-owned workflow run",
                &["cancel-workflow"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Cancel,
                },
            ),
            spec(
                "workflow-dossier",
                "Export a replay-derived workflow Run Dossier",
                &["dossier"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::DossierExport,
                },
            ),
            spec(
                "workflow-snapshot",
                "List, read, or export workflow context snapshots",
                &["snapshot"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Snapshot,
                },
            ),
            spec(
                "plan-consensus",
                "Create a reviewed consensus plan artifact",
                &["plan", "ralplan", "consensus-plan", "workflow-plan-consensus"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::PlanConsensus,
                },
            )
            .with_dollar_aliases(&["plan", "ralplan"]),
            spec(
                "goal-ledger",
                "Inspect or checkpoint workflow goal ledger state",
                &["goal", "ultragoal", "workflow-goal"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::GoalLedger,
                },
            )
            .with_dollar_aliases(&["goal", "ultragoal"]),
            spec(
                "research-mission",
                "Create or inspect validator-gated research mission state",
                &[
                    "mission",
                    "research-loop",
                    "autoresearch",
                    "workflow-mission",
                ],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::ResearchMission,
                },
            )
            .with_dollar_aliases(&["autoresearch"]),
            spec(
                "wiki",
                "Read, query, or update the markdown workflow wiki",
                &["workflow-wiki"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Wiki,
                },
            )
            .with_dollar_aliases(&["wiki"]),
            disabled_spec(
                "init-deep",
                "Blocked until deep interview writes coordinator-owned intake evidence",
                &["deep-interview"],
                CommandAction::BlockedWorkflow {
                    reason: "deep interview is staged until it records coordinator-owned context/interview evidence; use /workflow-snapshot for explicit intake evidence",
                    inventory_ref: "init-deep",
                },
            )
            .with_dollar_aliases(&["deep-interview"]),
            spec(
                "ralph-loop",
                "Start bounded Ralph continuation",
                &["ralph"],
                CommandAction::StartContinuation {
                    mode: ContinuationMode::Ralph,
                },
            )
            .with_dollar_aliases(&["ralph"]),
            spec(
                "ulw-loop",
                "Start bounded ultrawork continuation",
                &["ultrawork", "ulw"],
                CommandAction::StartContinuation {
                    mode: ContinuationMode::Ultrawork,
                },
            )
            .with_dollar_aliases(&["ultrawork"]),
            spec(
                "cancel-ralph",
                "Stop Ralph continuation",
                &["stop-ralph"],
                CommandAction::StopContinuation,
            ),
            spec(
                "stop-continuation",
                "Stop active continuation",
                &[],
                CommandAction::StopContinuation,
            )
            .with_dollar_aliases(&["cancel"]),
            disabled_spec(
                "refactor",
                "Blocked until cleanup guidance is workflow-evidence mapped",
                &[],
                CommandAction::BlockedWorkflow {
                    reason: "refactor guidance is staged until it maps behavior locks and verification evidence through workflow state",
                    inventory_ref: "refactor",
                },
            )
            .with_dollar_aliases(&["refactor"]),
            disabled_spec(
                "start-work",
                "Blocked until work-start creates workflow-owned handoff evidence",
                &[],
                CommandAction::BlockedWorkflow {
                    reason: "work-start is staged until it creates coordinator-owned workflow handoff evidence",
                    inventory_ref: "start-work",
                },
            ),
            disabled_spec(
                "remove-ai-slops",
                "Blocked until cleanup workflow maps verification evidence",
                &["deslop", "ai-slop-cleaner"],
                CommandAction::BlockedWorkflow {
                    reason: "AI-slop cleanup is staged until behavior locks and verification evidence are workflow-mapped",
                    inventory_ref: "remove-ai-slops",
                },
            )
            .with_dollar_aliases(&["ai-slop-cleaner", "deslop"]),
            disabled_spec(
                "handoff",
                "Blocked until handoff writes workflow-owned evidence",
                &[],
                CommandAction::BlockedWorkflow {
                    reason: "handoff artifacts are staged until they are tied to workflow evidence and closeout readiness",
                    inventory_ref: "handoff",
                },
            ),
            disabled_spec(
                "hyperplan",
                "Blocked until team planning handoff is workflow-evidence mapped",
                &[],
                CommandAction::BlockedWorkflow {
                    reason: "hyperplan is staged until team/parallel handoff evidence is workflow-owned",
                    inventory_ref: "hyperplan",
                },
            ),
            disabled_spec(
                "omx-skill:team",
                "Blocked until team orchestration writes coordinator-owned evidence",
                &["team"],
                CommandAction::BlockedWorkflow {
                    reason: "team workflow is staged until team task handoff, evidence, and closeout state are coordinator-owned",
                    inventory_ref: "omx-skill:team",
                },
            )
            .with_dollar_aliases(&["team"]),
            disabled_spec(
                "omx-skill:swarm",
                "Blocked team-style compatibility workflow",
                &["swarm"],
                CommandAction::BlockedWorkflow {
                    reason: "swarm compatibility is staged behind the team orchestration contract",
                    inventory_ref: "omx-skill:swarm",
                },
            )
            .with_dollar_aliases(&["swarm"]),
            disabled_spec(
                "omx-skill:ultraqa",
                "Blocked until QA loops emit deterministic coordinator evidence",
                &["ultraqa"],
                CommandAction::BlockedWorkflow {
                    reason: "ultraqa is staged until scenario generation, repair loops, and verification evidence are coordinator-owned",
                    inventory_ref: "omx-skill:ultraqa",
                },
            )
            .with_dollar_aliases(&["ultraqa"]),
            disabled_spec(
                "omx-skill:analyze",
                "Blocked until analysis reports are workflow-evidence mapped",
                &["analyze"],
                CommandAction::BlockedWorkflow {
                    reason: "analysis workflow is staged until read-only findings are recorded as coordinator-owned evidence",
                    inventory_ref: "omx-skill:analyze",
                },
            )
            .with_dollar_aliases(&["analyze"]),
            disabled_spec(
                "omx-skill:code-review",
                "Blocked until review findings and signoff blockers are modeled",
                &["code-review"],
                CommandAction::BlockedWorkflow {
                    reason: "code review is staged until findings, blocker severity, and signoff readiness are coordinator-owned",
                    inventory_ref: "omx-skill:code-review",
                },
            )
            .with_dollar_aliases(&["code-review"]),
            disabled_spec(
                "omx-skill:review",
                "Blocked generic review compatibility workflow",
                &["review"],
                CommandAction::BlockedWorkflow {
                    reason: "generic review compatibility is staged behind the code-review evidence contract",
                    inventory_ref: "omx-skill:review",
                },
            )
            .with_dollar_aliases(&["review"]),
            disabled_spec(
                "omx-skill:security-review",
                "Blocked until security findings are workflow-evidence mapped",
                &["security-review"],
                CommandAction::BlockedWorkflow {
                    reason: "security review is staged until findings and trust-boundary evidence are coordinator-owned",
                    inventory_ref: "omx-skill:security-review",
                },
            )
            .with_dollar_aliases(&["security-review"]),
            disabled_spec(
                "omx-skill:ask",
                "Blocked external advisor compatibility workflow",
                &["ask"],
                CommandAction::BlockedWorkflow {
                    reason: "advisor workflow is staged until external process execution is permission-gated and artifact-backed",
                    inventory_ref: "omx-skill:ask",
                },
            )
            .with_dollar_aliases(&["ask"]),
            disabled_spec(
                "omx-skill:ask-claude",
                "Blocked Claude advisor compatibility workflow",
                &["ask-claude"],
                CommandAction::BlockedWorkflow {
                    reason: "Claude advisor workflow is staged until external CLI execution is permission-gated and artifact-backed",
                    inventory_ref: "omx-skill:ask-claude",
                },
            )
            .with_dollar_aliases(&["ask-claude"]),
            disabled_spec(
                "omx-skill:ask-gemini",
                "Blocked Gemini advisor compatibility workflow",
                &["ask-gemini"],
                CommandAction::BlockedWorkflow {
                    reason: "Gemini advisor workflow is staged until external CLI execution is permission-gated and artifact-backed",
                    inventory_ref: "omx-skill:ask-gemini",
                },
            )
            .with_dollar_aliases(&["ask-gemini"]),
            disabled_spec(
                "omx-skill:doctor",
                "Blocked until doctor checks are workflow-evidence mapped",
                &["doctor"],
                CommandAction::BlockedWorkflow {
                    reason: "doctor workflow is staged until config/runtime checks produce coordinator-owned evidence",
                    inventory_ref: "omx-skill:doctor",
                },
            )
            .with_dollar_aliases(&["doctor"]),
            disabled_spec(
                "omx-skill:help",
                "Blocked until help derives from the command registry",
                &["help"],
                CommandAction::BlockedWorkflow {
                    reason: "help workflow is staged until operator help output derives fully from registry and inventory state",
                    inventory_ref: "omx-skill:help",
                },
            )
            .with_dollar_aliases(&["help"]),
            disabled_spec(
                "omx-skill:hud",
                "Blocked until HUD status is modeled as a harness projection",
                &["hud"],
                CommandAction::BlockedWorkflow {
                    reason: "HUD workflow is staged until statusline state is exposed through harness projections",
                    inventory_ref: "omx-skill:hud",
                },
            )
            .with_dollar_aliases(&["hud"]),
            disabled_spec(
                "omx-skill:note",
                "Blocked until note/memory writes are coordinator-owned",
                &["note"],
                CommandAction::BlockedWorkflow {
                    reason: "note workflow is staged until notepad and memory writes are modeled as coordinator-owned artifacts",
                    inventory_ref: "omx-skill:note",
                },
            )
            .with_dollar_aliases(&["note"]),
            disabled_spec(
                "omx-skill:skill",
                "Blocked until skill management is permission-gated",
                &["skill"],
                CommandAction::BlockedWorkflow {
                    reason: "skill management is staged until install/remove/edit operations are permission-gated and artifact-backed",
                    inventory_ref: "omx-skill:skill",
                },
            )
            .with_dollar_aliases(&["skill"]),
            disabled_spec(
                "omx-skill:trace",
                "Blocked until trace output is workflow-evidence mapped",
                &["trace"],
                CommandAction::BlockedWorkflow {
                    reason: "trace workflow is staged until event timelines are exported as replay-safe evidence",
                    inventory_ref: "omx-skill:trace",
                },
            )
            .with_dollar_aliases(&["trace"]),
            disabled_spec(
                "omx-skill:configure-notifications",
                "Blocked notification configuration compatibility workflow",
                &["configure-notifications"],
                CommandAction::BlockedWorkflow {
                    reason: "notification configuration is staged because host side effects must be explicitly permission-gated",
                    inventory_ref: "omx-skill:configure-notifications",
                },
            )
            .with_dollar_aliases(&["configure-notifications"]),
            disabled_spec(
                "omx-skill:omx-setup",
                "Blocked setup compatibility workflow",
                &["omx-setup"],
                CommandAction::BlockedWorkflow {
                    reason: "setup workflow is staged because installation side effects must be explicitly permission-gated",
                    inventory_ref: "omx-skill:omx-setup",
                },
            )
            .with_dollar_aliases(&["omx-setup"]),
            disabled_spec(
                "omx-skill:design",
                "Blocked until design source-of-truth artifacts are modeled",
                &["design"],
                CommandAction::BlockedWorkflow {
                    reason: "design workflow is staged until design documents and review state are coordinator-owned",
                    inventory_ref: "omx-skill:design",
                },
            )
            .with_dollar_aliases(&["design"]),
            disabled_spec(
                "omx-skill:frontend-ui-ux",
                "Deprecated frontend workflow compatibility command",
                &["frontend-ui-ux"],
                CommandAction::BlockedWorkflow {
                    reason: "frontend-ui-ux is deprecated; use design or visual workflow parity once implemented",
                    inventory_ref: "omx-skill:frontend-ui-ux",
                },
            )
            .with_dollar_aliases(&["frontend-ui-ux"]),
            disabled_spec(
                "omx-skill:autopilot",
                "Blocked autonomous execution compatibility workflow",
                &["autopilot"],
                CommandAction::BlockedWorkflow {
                    reason: "autopilot is staged until plan, execution, cleanup, and review gates are coordinator-owned",
                    inventory_ref: "omx-skill:autopilot",
                },
            )
            .with_dollar_aliases(&["autopilot"]),
            disabled_spec(
                "omx-skill:autoresearch-goal",
                "Blocked research-goal compatibility workflow",
                &["autoresearch-goal"],
                CommandAction::BlockedWorkflow {
                    reason: "autoresearch-goal is staged behind the research mission validator contract",
                    inventory_ref: "omx-skill:autoresearch-goal",
                },
            )
            .with_dollar_aliases(&["autoresearch-goal"]),
            disabled_spec(
                "omx-skill:deepsearch",
                "Blocked deep search compatibility workflow",
                &["deepsearch"],
                CommandAction::BlockedWorkflow {
                    reason: "deepsearch is staged behind the research mission and web/search permission contract",
                    inventory_ref: "omx-skill:deepsearch",
                },
            )
            .with_dollar_aliases(&["deepsearch"]),
            disabled_spec(
                "omx-skill:performance-goal",
                "Blocked performance-goal compatibility workflow",
                &["performance-goal"],
                CommandAction::BlockedWorkflow {
                    reason: "performance-goal is staged until benchmark/evaluator evidence is coordinator-owned",
                    inventory_ref: "omx-skill:performance-goal",
                },
            )
            .with_dollar_aliases(&["performance-goal"]),
            disabled_spec(
                "omx-skill:pipeline",
                "Blocked pipeline compatibility workflow",
                &["pipeline"],
                CommandAction::BlockedWorkflow {
                    reason: "pipeline is staged until stage transitions and artifacts are coordinator-owned",
                    inventory_ref: "omx-skill:pipeline",
                },
            )
            .with_dollar_aliases(&["pipeline"]),
            disabled_spec(
                "omx-skill:ecomode",
                "Blocked ecomode runtime workflow",
                &["ecomode"],
                CommandAction::BlockedWorkflow {
                    reason: "ecomode is staged because runtime mode changes are not modeled in Harness",
                    inventory_ref: "omx-skill:ecomode",
                },
            )
            .with_dollar_aliases(&["ecomode"]),
            disabled_spec(
                "omx-skill:tdd",
                "Blocked TDD compatibility workflow",
                &["tdd"],
                CommandAction::BlockedWorkflow {
                    reason: "TDD workflow is staged until test-first state and verification evidence are coordinator-owned",
                    inventory_ref: "omx-skill:tdd",
                },
            )
            .with_dollar_aliases(&["tdd"]),
            disabled_spec(
                "omx-skill:visual-ralph",
                "Blocked visual Ralph compatibility workflow",
                &["visual-ralph"],
                CommandAction::BlockedWorkflow {
                    reason: "visual Ralph is staged until visual verdict evidence and screenshots are modeled",
                    inventory_ref: "omx-skill:visual-ralph",
                },
            )
            .with_dollar_aliases(&["visual-ralph"]),
            disabled_spec(
                "omx-skill:visual-verdict",
                "Blocked visual verdict compatibility workflow",
                &["visual-verdict"],
                CommandAction::BlockedWorkflow {
                    reason: "visual verdict is staged until screenshot comparison evidence is modeled",
                    inventory_ref: "omx-skill:visual-verdict",
                },
            )
            .with_dollar_aliases(&["visual-verdict"]),
            disabled_spec(
                "omx-skill:web-clone",
                "Blocked web clone compatibility workflow",
                &["web-clone"],
                CommandAction::BlockedWorkflow {
                    reason: "web clone is staged because browser/live side effects must be explicitly permission-gated",
                    inventory_ref: "omx-skill:web-clone",
                },
            )
            .with_dollar_aliases(&["web-clone"]),
            disabled_spec(
                "omx-skill:ralph-init",
                "Blocked legacy Ralph init compatibility workflow",
                &["ralph-init"],
                CommandAction::BlockedWorkflow {
                    reason: "ralph-init is staged behind the bounded Ralph continuation contract",
                    inventory_ref: "omx-skill:ralph-init",
                },
            )
            .with_dollar_aliases(&["ralph-init"]),
        ])
    }

    pub fn commands(&self) -> &[CommandSpec] {
        &self.commands
    }

    pub fn get(&self, name_or_alias: &str) -> Option<&CommandSpec> {
        self.commands.iter().find(|command| {
            command.name == name_or_alias || command.aliases.contains(&name_or_alias)
        })
    }

    pub fn roots() -> Vec<PathBuf> {
        vec![
            PathBuf::from(".agent-harness/commands"),
            PathBuf::from(".opencode/command"),
            PathBuf::from(".opencode/commands"),
            PathBuf::from(".claude/commands"),
            PathBuf::from(".agents/commands"),
            PathBuf::from(".harness/commands"),
            PathBuf::from("~/.config/agent-harness/commands"),
            PathBuf::from("~/.config/opencode/command"),
            PathBuf::from("~/.claude/commands"),
            PathBuf::from("~/.agents/commands"),
        ]
    }
}

impl CommandSpec {
    fn with_dollar_aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.dollar_aliases = aliases;
        self
    }
}

fn spec(
    name: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
    action: CommandAction,
) -> CommandSpec {
    let surface = action.surface(WorkflowCommandAvailability::Present);
    let effect = action.effect();
    CommandSpec {
        name,
        description,
        aliases,
        dollar_aliases: &[],
        surface,
        effect,
        action,
        availability: WorkflowCommandAvailability::Present,
        enabled_by_default: true,
    }
}

fn disabled_spec(
    name: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
    action: CommandAction,
) -> CommandSpec {
    let surface = action.surface(WorkflowCommandAvailability::Staged);
    let effect = action.effect();
    CommandSpec {
        name,
        description,
        aliases,
        dollar_aliases: &[],
        surface,
        effect,
        action,
        availability: WorkflowCommandAvailability::Staged,
        enabled_by_default: false,
    }
}

impl CommandAction {
    pub fn surface(&self, availability: WorkflowCommandAvailability) -> CommandSurface {
        match self {
            Self::WorkflowIntent { .. } => CommandSurface::WorkflowCommand,
            Self::StartContinuation { .. } | Self::StopContinuation => {
                CommandSurface::ContinuationCommand
            }
            Self::BlockedWorkflow { .. } => match availability {
                WorkflowCommandAvailability::Present => CommandSurface::WorkflowCommand,
                WorkflowCommandAvailability::Staged | WorkflowCommandAvailability::Rejected => {
                    CommandSurface::StagedReferenceCommand
                }
            },
            Self::PromptTemplate { .. } => CommandSurface::PromptTemplate,
            Self::LoadSkills { .. } => CommandSurface::PromptTemplate,
            Self::ProfileSwitch { .. } => CommandSurface::ProfileSwitch,
            Self::NativeTool { .. } => CommandSurface::NativeTool,
            Self::PlanArtifact { .. } | Self::HandoffArtifact { .. } => {
                CommandSurface::StagedReferenceCommand
            }
            Self::TuiAction { .. } => CommandSurface::TuiAction,
        }
    }

    pub fn effect(&self) -> CommandEffect {
        match self {
            Self::WorkflowIntent { intent } => intent.effect(),
            Self::StartContinuation { .. } | Self::StopContinuation => {
                CommandEffect::ControlContinuation
            }
            Self::BlockedWorkflow { .. }
            | Self::PlanArtifact { .. }
            | Self::HandoffArtifact { .. } => CommandEffect::BlockedNoSideEffect,
            Self::PromptTemplate { .. } | Self::LoadSkills { .. } => CommandEffect::SubmitPrompt,
            Self::ProfileSwitch { .. } => CommandEffect::SwitchProfile,
            Self::NativeTool { .. } => CommandEffect::InvokeNativeTool,
            Self::TuiAction { .. } => CommandEffect::UpdateTuiState,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        CommandAction, CommandEffect, CommandRegistry, CommandSurface, WorkflowCommandAvailability,
        WorkflowIntent,
    };

    #[test]
    fn builtin_registry_exposes_omo_commands_without_shell_actions() {
        let registry = CommandRegistry::builtins();
        for name in [
            "workflow-run",
            "workflow-status",
            "workflow-signoff",
            "workflow-cancel",
            "workflow-dossier",
            "workflow-snapshot",
            "plan-consensus",
            "goal-ledger",
            "research-mission",
            "wiki",
            "init-deep",
            "ralph-loop",
            "ulw-loop",
            "cancel-ralph",
            "refactor",
            "start-work",
            "stop-continuation",
            "remove-ai-slops",
            "handoff",
            "hyperplan",
        ] {
            assert!(registry.get(name).is_some(), "missing {name}");
        }

        assert!(registry.commands().iter().all(|command| !matches!(
            command.action,
            CommandAction::NativeTool {
                tool_id: "bash" | "shell.run"
            }
        )));
        assert!(CommandRegistry::roots()
            .iter()
            .any(|root| root == &std::path::PathBuf::from(".opencode/command")));
    }

    #[test]
    fn workflow_commands_have_stable_intents_and_unique_aliases() {
        let registry = CommandRegistry::builtins();
        for (name, intent) in [
            ("workflow-run", WorkflowIntent::Run),
            ("workflow-status", WorkflowIntent::Status),
            ("workflow-signoff", WorkflowIntent::Signoff),
            ("workflow-cancel", WorkflowIntent::Cancel),
            ("workflow-dossier", WorkflowIntent::DossierExport),
            ("workflow-snapshot", WorkflowIntent::Snapshot),
            ("plan-consensus", WorkflowIntent::PlanConsensus),
            ("goal-ledger", WorkflowIntent::GoalLedger),
            ("research-mission", WorkflowIntent::ResearchMission),
            ("wiki", WorkflowIntent::Wiki),
        ] {
            let command = registry
                .get(name)
                .unwrap_or_else(|| panic!("missing {name}"));
            assert_eq!(
                command.action,
                CommandAction::WorkflowIntent { intent },
                "{name} drifted from workflow intent {}",
                intent.as_str()
            );
            assert_eq!(command.surface, CommandSurface::WorkflowCommand);
            assert_eq!(command.effect, intent.effect());
        }

        let mut names_and_aliases = BTreeSet::new();
        let mut dollar_aliases = BTreeSet::new();
        for command in registry.commands() {
            assert!(
                names_and_aliases.insert(command.name),
                "duplicate command name {}",
                command.name
            );
            for alias in command.aliases {
                assert!(
                    names_and_aliases.insert(alias),
                    "duplicate command alias {alias}"
                );
            }
            for alias in command.dollar_aliases {
                assert!(
                    dollar_aliases.insert(alias),
                    "duplicate dollar command alias {alias}"
                );
            }
        }
    }

    #[test]
    fn enabled_registry_commands_do_not_use_prompt_or_artifact_placeholders() {
        let registry = CommandRegistry::builtins();

        for command in registry
            .commands()
            .iter()
            .filter(|command| command.enabled_by_default)
        {
            assert!(
                !matches!(
                    command.action,
                    CommandAction::PromptTemplate { .. }
                        | CommandAction::LoadSkills { .. }
                        | CommandAction::PlanArtifact { .. }
                        | CommandAction::HandoffArtifact { .. }
                        | CommandAction::NativeTool { .. }
                ),
                "{} is enabled by default through a placeholder action",
                command.name
            );
        }

        for command in registry
            .commands()
            .iter()
            .filter(|command| matches!(&command.action, CommandAction::BlockedWorkflow { .. }))
        {
            assert!(
                !command.enabled_by_default,
                "{} must remain hidden until its blocked workflow action is implemented",
                command.name
            );
            assert_eq!(
                command.availability,
                WorkflowCommandAvailability::Staged,
                "{} blocked workflow must be explicitly classified as staged",
                command.name
            );
            let CommandAction::BlockedWorkflow {
                reason,
                inventory_ref,
            } = &command.action
            else {
                unreachable!("filtered blocked commands");
            };
            assert!(
                !reason.trim().is_empty(),
                "{} blocked workflow reason is empty",
                command.name
            );
            assert_eq!(
                *inventory_ref, command.name,
                "{} blocked workflow inventory ref should point at the command entry",
                command.name
            );
        }
    }

    #[test]
    fn command_availability_is_truthful_for_visible_and_staged_workflows() {
        let registry = CommandRegistry::builtins();
        let mut has_present = false;
        let mut has_staged = false;

        for command in registry.commands() {
            assert!(
                !command.description.trim().is_empty(),
                "{} should have an operator-facing purpose",
                command.name
            );
            match command.availability {
                WorkflowCommandAvailability::Present => {
                    has_present = true;
                    assert!(
                        command.enabled_by_default,
                        "{} present command should be visible by default",
                        command.name
                    );
                    assert!(
                        !matches!(&command.action, CommandAction::BlockedWorkflow { .. }),
                        "{} present command cannot be a blocked workflow",
                        command.name
                    );
                    assert_ne!(
                        command.effect,
                        CommandEffect::BlockedNoSideEffect,
                        "{} present command cannot have blocked/no-side-effect semantics",
                        command.name
                    );
                }
                WorkflowCommandAvailability::Staged => {
                    has_staged = true;
                    assert!(
                        !command.enabled_by_default,
                        "{} staged command must not be visible by default",
                        command.name
                    );
                    assert!(
                        matches!(&command.action, CommandAction::BlockedWorkflow { .. }),
                        "{} staged command must fail closed instead of faking completion",
                        command.name
                    );
                    assert_eq!(
                        command.surface,
                        CommandSurface::StagedReferenceCommand,
                        "{} staged command must remain on the hidden reference surface",
                        command.name
                    );
                    assert_eq!(
                        command.effect,
                        CommandEffect::BlockedNoSideEffect,
                        "{} staged command must have no runtime side effect",
                        command.name
                    );
                }
                WorkflowCommandAvailability::Rejected => {
                    assert!(
                        !command.enabled_by_default,
                        "{} rejected command must never be visible by default",
                        command.name
                    );
                    assert!(
                        matches!(&command.action, CommandAction::BlockedWorkflow { .. }),
                        "{} rejected command must fail closed instead of dispatching",
                        command.name
                    );
                }
            }
        }

        assert!(
            has_present,
            "registry should classify implemented workflows"
        );
        assert!(has_staged, "registry should classify staged workflows");
    }

    #[test]
    fn omx_compatibility_aliases_resolve_without_prompt_only_dispatch() {
        let registry = CommandRegistry::builtins();
        for (alias, expected_name) in [
            ("deep-interview", "init-deep"),
            ("plan", "plan-consensus"),
            ("ralplan", "plan-consensus"),
            ("ralph", "ralph-loop"),
            ("ultrawork", "ulw-loop"),
            ("ultragoal", "goal-ledger"),
            ("autoresearch", "research-mission"),
            ("ai-slop-cleaner", "remove-ai-slops"),
        ] {
            let command = registry
                .get(alias)
                .unwrap_or_else(|| panic!("missing compatibility alias {alias}"));
            assert_eq!(
                command.name, expected_name,
                "{alias} resolved to unexpected command"
            );
            assert!(
                !matches!(command.action, CommandAction::PromptTemplate { .. }),
                "{alias} must not resolve to prompt-only dispatch"
            );
        }
    }

    #[test]
    fn dollar_workflow_family_aliases_resolve_to_native_or_fail_closed_actions() {
        let registry = CommandRegistry::builtins();

        for alias in [
            "ai-slop-cleaner",
            "analyze",
            "ask",
            "ask-claude",
            "ask-gemini",
            "autopilot",
            "autoresearch",
            "autoresearch-goal",
            "code-review",
            "configure-notifications",
            "deep-interview",
            "deepsearch",
            "design",
            "deslop",
            "doctor",
            "ecomode",
            "frontend-ui-ux",
            "goal",
            "help",
            "hud",
            "note",
            "omx-setup",
            "performance-goal",
            "pipeline",
            "plan",
            "ralph",
            "ralph-init",
            "ralplan",
            "refactor",
            "review",
            "security-review",
            "skill",
            "swarm",
            "tdd",
            "team",
            "trace",
            "ultragoal",
            "ultraqa",
            "ultrawork",
            "visual-ralph",
            "visual-verdict",
            "web-clone",
            "wiki",
        ] {
            let command = registry
                .commands()
                .iter()
                .find(|command| command.dollar_aliases.contains(&alias))
                .unwrap_or_else(|| panic!("missing dollar workflow alias ${alias}"));
            assert!(
                !matches!(
                    command.action,
                    CommandAction::PromptTemplate { .. }
                        | CommandAction::LoadSkills { .. }
                        | CommandAction::PlanArtifact { .. }
                        | CommandAction::HandoffArtifact { .. }
                ),
                "${alias} must not dispatch through a prompt/artifact placeholder"
            );
            if command.enabled_by_default {
                assert_eq!(
                    command.availability,
                    WorkflowCommandAvailability::Present,
                    "${alias} enabled command should be present"
                );
            } else {
                assert!(
                    matches!(command.action, CommandAction::BlockedWorkflow { .. }),
                    "${alias} staged command must fail closed"
                );
                assert_eq!(
                    command.effect,
                    CommandEffect::BlockedNoSideEffect,
                    "${alias} staged command must not mutate state"
                );
            }
        }
    }
}

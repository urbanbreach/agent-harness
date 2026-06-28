//! Command palette parity matrix derived from Opencode source.
//!
//! Each entry maps a stable Opencode command ID to its parity status, category,
//! title rule, and Harness dispatch path. Tests consume this matrix to assert
//! exact included/excluded/hidden command IDs.

/// Parity status for a command relative to the Opencode palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParityStatus {
    /// Visible/reachable Opencode palette command that Harness must include.
    Included,
    /// Explicitly excluded by user request; must not appear in the palette.
    Excluded,
    /// Hidden Opencode command; not a parity target.
    HiddenNonTarget,
    /// Harness-only command with no Opencode equivalent; excluded from parity accounting.
    HarnessOnly,
}

/// Title rule: either a static title or a dynamic toggle condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleRule {
    /// Static title string.
    Static(&'static str),
    /// Dynamic title: "Enable X" when off, "Disable X" when on.
    Toggle {
        enable: &'static str,
        disable: &'static str,
    },
    /// Dynamic title: "Show X" when hidden, "Hide X" when shown.
    ShowHide {
        show: &'static str,
        hide: &'static str,
    },
}

/// Suggested rule for the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestedRule {
    /// Never suggested.
    Never,
    /// Always suggested.
    Always,
    /// Suggested when sessions exist.
    WhenSessionsExist,
    /// Suggested when on session route.
    WhenSessionRoute,
    /// Suggested when provider is disconnected.
    WhenDisconnected,
    /// Suggested when org name is active.
    WhenOrgActive,
}

/// Availability rule for the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityRule {
    /// Always available.
    Always,
    /// Available only when not in startup shell.
    NotStartup,
    /// Available only when a live session is active.
    LiveSession,
    /// Available when editor context exists.
    EditorContextExists,
    /// Available when prompt input is non-empty.
    PromptInputExists,
    /// Available when stash is non-empty.
    StashNonEmpty,
    /// Available when share URL exists.
    ShareUrlExists,
    /// Available when revert message exists.
    RevertExists,
    /// Available when workspace feature is enabled.
    WorkspaceFeature,
    /// Available when worktree workspace has directory.
    WorktreeWorkspace,
    /// Available when variants are present.
    VariantsPresent,
    /// Available when multiple orgs are switchable.
    MultipleOrgs,
    /// Available when not in replay mode.
    NotReplay,
    /// Available when review surface is absent.
    NoReviewSurface,
}

/// Dispatch path for the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchPath {
    /// Opens a dialog/overlay.
    Dialog,
    /// Mutates local AppState only.
    LocalToggle,
    /// Emits a UiIntent.
    Intent,
    /// Executes an Action.
    Action,
    /// Not yet implemented; opens placeholder.
    Placeholder,
    /// Harness-only command with no Opencode equivalent.
    HarnessOnly,
}

/// A single parity matrix entry.
#[derive(Debug, Clone, Copy)]
pub struct ParityEntry {
    /// Stable Opencode command ID.
    pub id: &'static str,
    /// Opencode source file and line reference.
    pub origin: &'static str,
    /// Parity status.
    pub status: ParityStatus,
    /// Opencode category.
    pub category: &'static str,
    /// Title rule.
    pub title: TitleRule,
    /// Suggested rule.
    pub suggested: SuggestedRule,
    /// Availability rule.
    pub availability: AvailabilityRule,
    /// Dispatch path.
    pub dispatch: DispatchPath,
    /// Harness equivalent command ID or action, if any.
    pub harness_equivalent: &'static str,
}

/// The complete parity matrix.
///
/// Derived from:
/// - `inspirations/opencode/packages/tui/src/app.tsx:549` (app/global commands)
/// - `inspirations/opencode/packages/tui/src/component/prompt/index.tsx:330` (prompt/stash commands)
/// - `inspirations/opencode/packages/tui/src/routes/session/index.tsx:458` (session commands)
/// - `inspirations/opencode/packages/tui/src/feature-plugins/system/plugins.tsx:238` (plugin commands)
/// - `inspirations/opencode/packages/tui/src/feature-plugins/home/tips.tsx:10` (tips command)
pub const PARITY_MATRIX: &[ParityEntry] = &[
    // === App / Global Origin (app.tsx:549) ===
    ParityEntry {
        id: "command.palette.show",
        origin: "app.tsx:552",
        status: ParityStatus::HiddenNonTarget,
        category: "System",
        title: TitleRule::Static("Show command palette"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Action,
        harness_equivalent: "palette",
    },
    ParityEntry {
        id: "session.list",
        origin: "app.tsx:561",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("Switch session"),
        suggested: SuggestedRule::WhenSessionsExist,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Dialog,
        harness_equivalent: "session.list",
    },
    ParityEntry {
        id: "session.new",
        origin: "app.tsx:572",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("New session"),
        suggested: SuggestedRule::WhenSessionRoute,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Intent,
        harness_equivalent: "session.new",
    },
    ParityEntry {
        id: "workspace.copy_path",
        origin: "app.tsx:586",
        status: ParityStatus::Included,
        category: "Workspace",
        title: TitleRule::Static("Copy worktree path"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::WorktreeWorkspace,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "workspace.list",
        origin: "app.tsx:601",
        status: ParityStatus::Included,
        category: "Workspace",
        title: TitleRule::Static("Manage workspaces"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::WorkspaceFeature,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "model.list",
        origin: "app.tsx:620",
        status: ParityStatus::Included,
        category: "Agent",
        title: TitleRule::Static("Switch model"),
        suggested: SuggestedRule::Always,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Dialog,
        harness_equivalent: "model.list",
    },
    ParityEntry {
        id: "agent.list",
        origin: "app.tsx:668",
        status: ParityStatus::Included,
        category: "Agent",
        title: TitleRule::Static("Switch agent"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Dialog,
        harness_equivalent: "model_switcher",
    },
    ParityEntry {
        id: "mcp.list",
        origin: "app.tsx:677",
        status: ParityStatus::Included,
        category: "Agent",
        title: TitleRule::Static("Toggle MCPs"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Dialog,
        harness_equivalent: "toggles_menu",
    },
    ParityEntry {
        id: "variant.cycle",
        origin: "app.tsx:695",
        status: ParityStatus::Included,
        category: "Agent",
        title: TitleRule::Static("Variant cycle"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NotReplay,
        dispatch: DispatchPath::Action,
        harness_equivalent: "variant.cycle",
    },
    ParityEntry {
        id: "variant.list",
        origin: "app.tsx:703",
        status: ParityStatus::Included,
        category: "Agent",
        title: TitleRule::Static("Switch model variant"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::VariantsPresent,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "provider.connect",
        origin: "app.tsx:729",
        status: ParityStatus::Included,
        category: "Provider",
        title: TitleRule::Static("Connect provider"),
        suggested: SuggestedRule::WhenDisconnected,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Dialog,
        harness_equivalent: "provider.connect",
    },
    ParityEntry {
        id: "console.org.switch",
        origin: "app.tsx:741",
        status: ParityStatus::Included,
        category: "Provider",
        title: TitleRule::Static("Switch org"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::MultipleOrgs,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "opencode.status",
        origin: "app.tsx:754",
        status: ParityStatus::Excluded,
        category: "System",
        title: TitleRule::Static("View status"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Dialog,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "theme.switch",
        origin: "app.tsx:763",
        status: ParityStatus::Excluded,
        category: "System",
        title: TitleRule::Static("Switch theme"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Action,
        harness_equivalent: "open_theme_dialog",
    },
    ParityEntry {
        id: "theme.switch_mode",
        origin: "app.tsx:772",
        status: ParityStatus::Excluded,
        category: "System",
        title: TitleRule::Static("Switch to light mode"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "theme.mode.lock",
        origin: "app.tsx:781",
        status: ParityStatus::Excluded,
        category: "System",
        title: TitleRule::Static("Lock theme mode"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "help.show",
        origin: "app.tsx:791",
        status: ParityStatus::Excluded,
        category: "System",
        title: TitleRule::Static("Help"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Action,
        harness_equivalent: "help.show",
    },
    ParityEntry {
        id: "docs.open",
        origin: "app.tsx:800",
        status: ParityStatus::Excluded,
        category: "System",
        title: TitleRule::Static("Open docs"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "app.exit",
        origin: "app.tsx:809",
        status: ParityStatus::Included,
        category: "System",
        title: TitleRule::Static("Exit the app"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Action,
        harness_equivalent: "app.exit",
    },
    ParityEntry {
        id: "app.debug",
        origin: "app.tsx:817",
        status: ParityStatus::Included,
        category: "System",
        title: TitleRule::Static("Toggle debug panel"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "app.console",
        origin: "app.tsx:826",
        status: ParityStatus::Included,
        category: "System",
        title: TitleRule::Static("Toggle console"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "app.heap_snapshot",
        origin: "app.tsx:835",
        status: ParityStatus::Included,
        category: "System",
        title: TitleRule::Static("Write heap snapshot"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "terminal.suspend",
        origin: "app.tsx:849",
        status: ParityStatus::HiddenNonTarget,
        category: "System",
        title: TitleRule::Static("Suspend terminal"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "terminal.title.toggle",
        origin: "app.tsx:861",
        status: ParityStatus::Included,
        category: "System",
        title: TitleRule::Toggle {
            enable: "Enable terminal title",
            disable: "Disable terminal title",
        },
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "app.toggle.animations",
        origin: "app.tsx:875",
        status: ParityStatus::Included,
        category: "System",
        title: TitleRule::Toggle {
            enable: "Enable animations",
            disable: "Disable animations",
        },
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "app.toggle.file_context",
        origin: "app.tsx:884",
        status: ParityStatus::Included,
        category: "System",
        title: TitleRule::Toggle {
            enable: "Enable file context",
            disable: "Disable file context",
        },
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "app.toggle.diffwrap",
        origin: "app.tsx:893",
        status: ParityStatus::Included,
        category: "System",
        title: TitleRule::Toggle {
            enable: "Enable diff wrapping",
            disable: "Disable diff wrapping",
        },
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "app.toggle.paste_summary",
        origin: "app.tsx:903",
        status: ParityStatus::Included,
        category: "System",
        title: TitleRule::Toggle {
            enable: "Enable paste summary",
            disable: "Disable paste summary",
        },
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "app.toggle.session_directory_filter",
        origin: "app.tsx:916",
        status: ParityStatus::Included,
        category: "System",
        title: TitleRule::Toggle {
            enable: "Enable session directory filtering",
            disable: "Disable session directory filtering",
        },
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    // Hidden non-targets from app.tsx
    ParityEntry {
        id: "session.quick_switch.1",
        origin: "app.tsx:611",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Switch to session in quick slot 1"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.quick_switch.2",
        origin: "app.tsx:611",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Switch to session in quick slot 2"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.quick_switch.3",
        origin: "app.tsx:611",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Switch to session in quick slot 3"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.quick_switch.4",
        origin: "app.tsx:611",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Switch to session in quick slot 4"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.quick_switch.5",
        origin: "app.tsx:611",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Switch to session in quick slot 5"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.quick_switch.6",
        origin: "app.tsx:611",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Switch to session in quick slot 6"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.quick_switch.7",
        origin: "app.tsx:611",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Switch to session in quick slot 7"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.quick_switch.8",
        origin: "app.tsx:611",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Switch to session in quick slot 8"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.quick_switch.9",
        origin: "app.tsx:611",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Switch to session in quick slot 9"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "model.cycle_recent",
        origin: "app.tsx:632",
        status: ParityStatus::HiddenNonTarget,
        category: "Agent",
        title: TitleRule::Static("Model cycle"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "model.cycle_recent_reverse",
        origin: "app.tsx:641",
        status: ParityStatus::HiddenNonTarget,
        category: "Agent",
        title: TitleRule::Static("Model cycle reverse"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "model.cycle_favorite",
        origin: "app.tsx:650",
        status: ParityStatus::HiddenNonTarget,
        category: "Agent",
        title: TitleRule::Static("Favorite cycle"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "model.cycle_favorite_reverse",
        origin: "app.tsx:659",
        status: ParityStatus::HiddenNonTarget,
        category: "Agent",
        title: TitleRule::Static("Favorite cycle reverse"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "agent.cycle",
        origin: "app.tsx:686",
        status: ParityStatus::HiddenNonTarget,
        category: "Agent",
        title: TitleRule::Static("Agent cycle"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Action,
        harness_equivalent: "agent.list",
    },
    ParityEntry {
        id: "agent.cycle.reverse",
        origin: "app.tsx:720",
        status: ParityStatus::HiddenNonTarget,
        category: "Agent",
        title: TitleRule::Static("Agent cycle reverse"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Action,
        harness_equivalent: "agent.list",
    },
    // === Prompt / Stash Origin (prompt/index.tsx:330) ===
    ParityEntry {
        id: "prompt.clear",
        origin: "prompt/index.tsx:334",
        status: ParityStatus::HiddenNonTarget,
        category: "Prompt",
        title: TitleRule::Static("Clear prompt"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Action,
        harness_equivalent: "clear_prompt",
    },
    ParityEntry {
        id: "prompt.submit",
        origin: "prompt/index.tsx:344",
        status: ParityStatus::HiddenNonTarget,
        category: "Prompt",
        title: TitleRule::Static("Submit prompt"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Action,
        harness_equivalent: "submit_prompt",
    },
    ParityEntry {
        id: "prompt.editor_context.clear",
        origin: "prompt/index.tsx:357",
        status: ParityStatus::Included,
        category: "Prompt",
        title: TitleRule::Static("Remove editor context"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::EditorContextExists,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "prompt.paste",
        origin: "prompt/index.tsx:366",
        status: ParityStatus::HiddenNonTarget,
        category: "Prompt",
        title: TitleRule::Static("Paste"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.interrupt",
        origin: "prompt/index.tsx:389",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Interrupt session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "prompt.editor",
        origin: "prompt/index.tsx:421",
        status: ParityStatus::Excluded,
        category: "Session",
        title: TitleRule::Static("Open editor"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "prompt.skills",
        origin: "prompt/index.tsx:511",
        status: ParityStatus::Included,
        category: "Prompt",
        title: TitleRule::Static("Skills"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "workspace.set",
        origin: "prompt/index.tsx:533",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("Warp"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::WorkspaceFeature,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.move",
        origin: "prompt/index.tsx:542",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("Move session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::LiveSession,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "prompt.stash",
        origin: "prompt/index.tsx:734",
        status: ParityStatus::Included,
        category: "Prompt",
        title: TitleRule::Static("Stash prompt"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::PromptInputExists,
        dispatch: DispatchPath::Action,
        harness_equivalent: "prompt.stash",
    },
    ParityEntry {
        id: "prompt.stash.pop",
        origin: "prompt/index.tsx:752",
        status: ParityStatus::Included,
        category: "Prompt",
        title: TitleRule::Static("Stash pop"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::StashNonEmpty,
        dispatch: DispatchPath::Action,
        harness_equivalent: "prompt.stash.pop",
    },
    ParityEntry {
        id: "prompt.stash.list",
        origin: "prompt/index.tsx:768",
        status: ParityStatus::Included,
        category: "Prompt",
        title: TitleRule::Static("Stash list"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::StashNonEmpty,
        dispatch: DispatchPath::Action,
        harness_equivalent: "prompt.stash.list",
    },
    // === Session Origin (session/index.tsx:458) ===
    ParityEntry {
        id: "session.share",
        origin: "session/index.tsx:460",
        status: ParityStatus::Excluded,
        category: "Session",
        title: TitleRule::Static("Share session"),
        suggested: SuggestedRule::WhenSessionRoute,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.rename",
        origin: "session/index.tsx:500",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("Rename session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::LiveSession,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "slash_rename",
    },
    ParityEntry {
        id: "session.timeline",
        origin: "session/index.tsx:511",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("Jump to message"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::LiveSession,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.fork",
        origin: "session/index.tsx:533",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("Fork session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::LiveSession,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "slash_fork",
    },
    ParityEntry {
        id: "session.compact",
        origin: "session/index.tsx:555",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("Compact session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::LiveSession,
        dispatch: DispatchPath::Intent,
        harness_equivalent: "slash_compact",
    },
    ParityEntry {
        id: "session.unshare",
        origin: "session/index.tsx:581",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("Unshare session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::ShareUrlExists,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.undo",
        origin: "session/index.tsx:604",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("Undo previous message"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::LiveSession,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.redo",
        origin: "session/index.tsx:641",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("Redo"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::RevertExists,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.sidebar.toggle",
        origin: "session/index.tsx:667",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::ShowHide {
            show: "Show sidebar",
            hide: "Hide sidebar",
        },
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NoReviewSurface,
        dispatch: DispatchPath::Action,
        harness_equivalent: "toggle_operator_sidebar",
    },
    ParityEntry {
        id: "session.toggle.conceal",
        origin: "session/index.tsx:680",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Toggle {
            enable: "Enable code concealment",
            disable: "Disable code concealment",
        },
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NoReviewSurface,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.toggle.timestamps",
        origin: "session/index.tsx:689",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::ShowHide {
            show: "Show timestamps",
            hide: "Hide timestamps",
        },
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NoReviewSurface,
        dispatch: DispatchPath::LocalToggle,
        harness_equivalent: "show_timestamps/hide_timestamps",
    },
    ParityEntry {
        id: "session.toggle.thinking",
        origin: "session/index.tsx:702",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::ShowHide {
            show: "Expand thinking",
            hide: "Collapse thinking",
        },
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NoReviewSurface,
        dispatch: DispatchPath::LocalToggle,
        harness_equivalent: "show_thinking/hide_thinking",
    },
    ParityEntry {
        id: "session.toggle.actions",
        origin: "session/index.tsx:719",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::ShowHide {
            show: "Show tool details",
            hide: "Hide tool details",
        },
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NoReviewSurface,
        dispatch: DispatchPath::LocalToggle,
        harness_equivalent: "show_tool_details/hide_tool_details",
    },
    ParityEntry {
        id: "session.toggle.scrollbar",
        origin: "session/index.tsx:728",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("Toggle session scrollbar"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NoReviewSurface,
        dispatch: DispatchPath::Action,
        harness_equivalent: "toggle_scrollbar",
    },
    ParityEntry {
        id: "session.toggle.generic_tool_output",
        origin: "session/index.tsx:737",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::ShowHide {
            show: "Show generic tool output",
            hide: "Hide generic tool output",
        },
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NoReviewSurface,
        dispatch: DispatchPath::LocalToggle,
        harness_equivalent: "show_generic_tool_output/hide_generic_tool_output",
    },
    // Hidden non-target session navigation commands
    ParityEntry {
        id: "session.page.up",
        origin: "session/index.tsx:746",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Page up"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.page.down",
        origin: "session/index.tsx:756",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Page down"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.line.up",
        origin: "session/index.tsx:766",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Line up"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.line.down",
        origin: "session/index.tsx:776",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Line down"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.half.page.up",
        origin: "session/index.tsx:786",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Half page up"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.half.page.down",
        origin: "session/index.tsx:796",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Half page down"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.first",
        origin: "session/index.tsx:806",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("First message"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Action,
        harness_equivalent: "first_message",
    },
    ParityEntry {
        id: "session.last",
        origin: "session/index.tsx:816",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Last message"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Action,
        harness_equivalent: "last_message",
    },
    ParityEntry {
        id: "session.messages_last_user",
        origin: "session/index.tsx:826",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Jump to last user message"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.message.next",
        origin: "session/index.tsx:857",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Next message"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Action,
        harness_equivalent: "next_message",
    },
    ParityEntry {
        id: "session.message.previous",
        origin: "session/index.tsx:864",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Previous message"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Action,
        harness_equivalent: "previous_message",
    },
    ParityEntry {
        id: "messages.copy",
        origin: "session/index.tsx:872",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("Copy last assistant message"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::LiveSession,
        dispatch: DispatchPath::Action,
        harness_equivalent: "copy_message",
    },
    ParityEntry {
        id: "session.copy",
        origin: "session/index.tsx:914",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("Copy session transcript"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::LiveSession,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.export",
        origin: "session/index.tsx:944",
        status: ParityStatus::Included,
        category: "Session",
        title: TitleRule::Static("Export session transcript"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::LiveSession,
        dispatch: DispatchPath::Action,
        harness_equivalent: "export_session",
    },
    ParityEntry {
        id: "plugins.list",
        origin: "plugins.tsx:238",
        status: ParityStatus::Excluded,
        category: "System",
        title: TitleRule::Static("Plugins"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "plugins.install",
        origin: "plugins.tsx:238",
        status: ParityStatus::Excluded,
        category: "System",
        title: TitleRule::Static("Install plugin"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "tips.toggle",
        origin: "tips.tsx:10",
        status: ParityStatus::Included,
        category: "System",
        title: TitleRule::ShowHide {
            show: "Show tips",
            hide: "Hide tips",
        },
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "diff.open",
        origin: "diff-viewer.tsx:1053",
        status: ParityStatus::Excluded,
        category: "System",
        title: TitleRule::Static("Open diff viewer"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.background",
        origin: "session/index.tsx:1019",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Background session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "missing",
    },
    ParityEntry {
        id: "session.child.first",
        origin: "session/index.tsx:1033",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("First child session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "harness.session_child_first",
    },
    ParityEntry {
        id: "session.parent",
        origin: "session/index.tsx:1043",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Parent session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "harness.session_parent",
    },
    ParityEntry {
        id: "session.child.next",
        origin: "session/index.tsx:1060",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Next child session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "harness.session_child_cycle",
    },
    ParityEntry {
        id: "session.child.previous",
        origin: "session/index.tsx:1071",
        status: ParityStatus::HiddenNonTarget,
        category: "Session",
        title: TitleRule::Static("Previous child session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::Always,
        dispatch: DispatchPath::Placeholder,
        harness_equivalent: "harness.session_child_cycle_reverse",
    },
    // === Harness-only commands (no Opencode equivalent) ===
    ParityEntry {
        id: "harness.close_review_surface",
        origin: "harness",
        status: ParityStatus::HarnessOnly,
        category: "Session",
        title: TitleRule::Static("Session shell"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NotStartup,
        dispatch: DispatchPath::Action,
        harness_equivalent: "harness.close_review_surface",
    },
    ParityEntry {
        id: "harness.toggle_terminal_panel",
        origin: "harness",
        status: ParityStatus::HarnessOnly,
        category: "Session",
        title: TitleRule::Static("Toggle terminal panel"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NotStartup,
        dispatch: DispatchPath::Action,
        harness_equivalent: "harness.toggle_terminal_panel",
    },
    ParityEntry {
        id: "harness.toggle_follow",
        origin: "harness",
        status: ParityStatus::HarnessOnly,
        category: "Agent",
        title: TitleRule::Static("Toggle follow"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NotReplay,
        dispatch: DispatchPath::Action,
        harness_equivalent: "harness.toggle_follow",
    },
    ParityEntry {
        id: "harness.revert_workspace",
        origin: "harness",
        status: ParityStatus::HarnessOnly,
        category: "Session",
        title: TitleRule::Static("Revert workspace"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::LiveSession,
        dispatch: DispatchPath::Action,
        harness_equivalent: "harness.revert_workspace",
    },
    ParityEntry {
        id: "harness.open_lineage_browser",
        origin: "harness",
        status: ParityStatus::HarnessOnly,
        category: "Session",
        title: TitleRule::Static("Session tree"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NotStartup,
        dispatch: DispatchPath::Action,
        harness_equivalent: "harness.open_lineage_browser",
    },
    ParityEntry {
        id: "harness.session_child_first",
        origin: "harness",
        status: ParityStatus::HarnessOnly,
        category: "Session",
        title: TitleRule::Static("First child session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NotStartup,
        dispatch: DispatchPath::Action,
        harness_equivalent: "harness.session_child_first",
    },
    ParityEntry {
        id: "harness.session_child_cycle",
        origin: "harness",
        status: ParityStatus::HarnessOnly,
        category: "Session",
        title: TitleRule::Static("Next child session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NotStartup,
        dispatch: DispatchPath::Action,
        harness_equivalent: "harness.session_child_cycle",
    },
    ParityEntry {
        id: "harness.session_child_cycle_reverse",
        origin: "harness",
        status: ParityStatus::HarnessOnly,
        category: "Session",
        title: TitleRule::Static("Previous child session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NotStartup,
        dispatch: DispatchPath::Action,
        harness_equivalent: "harness.session_child_cycle_reverse",
    },
    ParityEntry {
        id: "harness.session_parent",
        origin: "harness",
        status: ParityStatus::HarnessOnly,
        category: "Session",
        title: TitleRule::Static("Parent session"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NotStartup,
        dispatch: DispatchPath::Action,
        harness_equivalent: "harness.session_parent",
    },
    ParityEntry {
        id: "harness.stack_transcript_diffs",
        origin: "harness",
        status: ParityStatus::HarnessOnly,
        category: "Agent",
        title: TitleRule::Static("Use stacked diffs"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NoReviewSurface,
        dispatch: DispatchPath::LocalToggle,
        harness_equivalent: "harness.stack_transcript_diffs",
    },
    ParityEntry {
        id: "harness.split_transcript_diffs",
        origin: "harness",
        status: ParityStatus::HarnessOnly,
        category: "Agent",
        title: TitleRule::Static("Use split diffs"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NoReviewSurface,
        dispatch: DispatchPath::LocalToggle,
        harness_equivalent: "harness.split_transcript_diffs",
    },
    ParityEntry {
        id: "harness.expand_turn_results",
        origin: "harness",
        status: ParityStatus::HarnessOnly,
        category: "Agent",
        title: TitleRule::Static("Expand turn results"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NoReviewSurface,
        dispatch: DispatchPath::LocalToggle,
        harness_equivalent: "harness.expand_turn_results",
    },
    ParityEntry {
        id: "harness.collapse_turn_results",
        origin: "harness",
        status: ParityStatus::HarnessOnly,
        category: "Agent",
        title: TitleRule::Static("Collapse turn results"),
        suggested: SuggestedRule::Never,
        availability: AvailabilityRule::NoReviewSurface,
        dispatch: DispatchPath::LocalToggle,
        harness_equivalent: "harness.collapse_turn_results",
    },
];

/// Get all included command IDs from the parity matrix.
pub fn included_ids() -> Vec<&'static str> {
    PARITY_MATRIX
        .iter()
        .filter(|entry| entry.status == ParityStatus::Included)
        .map(|entry| entry.id)
        .collect()
}

/// Get all excluded command IDs from the parity matrix.
pub fn excluded_ids() -> Vec<&'static str> {
    PARITY_MATRIX
        .iter()
        .filter(|entry| entry.status == ParityStatus::Excluded)
        .map(|entry| entry.id)
        .collect()
}

/// Get all hidden non-target command IDs from the parity matrix.
pub fn hidden_non_target_ids() -> Vec<&'static str> {
    PARITY_MATRIX
        .iter()
        .filter(|entry| entry.status == ParityStatus::HiddenNonTarget)
        .map(|entry| entry.id)
        .collect()
}

/// Get all Harness-only command IDs from the parity matrix.
pub fn harness_only_ids() -> Vec<&'static str> {
    PARITY_MATRIX
        .iter()
        .filter(|entry| entry.status == ParityStatus::HarnessOnly)
        .map(|entry| entry.id)
        .collect()
}

/// Find a parity entry by command ID.
pub fn find_entry(id: &str) -> Option<&'static ParityEntry> {
    PARITY_MATRIX.iter().find(|entry| entry.id == id)
}

/// Get the static title for a command, if the title rule is static.
pub fn static_title(entry: &ParityEntry) -> Option<&'static str> {
    match entry.title {
        TitleRule::Static(title) => Some(title),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn matrix_has_no_duplicate_ids() {
        let ids: HashSet<&str> = PARITY_MATRIX.iter().map(|e| e.id).collect();
        assert_eq!(
            ids.len(),
            PARITY_MATRIX.len(),
            "parity matrix has duplicate command IDs"
        );
    }

    #[test]
    fn matrix_includes_all_seed_included_commands() {
        let included: HashSet<&str> = included_ids().into_iter().collect();
        let required = [
            "session.list",
            "session.new",
            "workspace.copy_path",
            "workspace.list",
            "model.list",
            "agent.list",
            "mcp.list",
            "variant.cycle",
            "variant.list",
            "provider.connect",
            "console.org.switch",
            "app.exit",
            "app.debug",
            "app.console",
            "app.heap_snapshot",
            "terminal.title.toggle",
            "app.toggle.animations",
            "app.toggle.file_context",
            "app.toggle.diffwrap",
            "app.toggle.paste_summary",
            "app.toggle.session_directory_filter",
            "prompt.editor_context.clear",
            "prompt.skills",
            "workspace.set",
            "session.move",
            "prompt.stash",
            "prompt.stash.pop",
            "prompt.stash.list",
            "session.rename",
            "session.timeline",
            "session.fork",
            "session.compact",
            "session.unshare",
            "session.undo",
            "session.redo",
            "session.sidebar.toggle",
            "session.toggle.conceal",
            "session.toggle.timestamps",
            "session.toggle.thinking",
            "session.toggle.actions",
            "session.toggle.scrollbar",
            "session.toggle.generic_tool_output",
            "messages.copy",
            "session.copy",
            "session.export",
            "tips.toggle",
        ];
        for id in required {
            assert!(
                included.contains(id),
                "matrix missing included command ID: {id}"
            );
        }
    }

    #[test]
    fn matrix_includes_all_excluded_commands() {
        let excluded: HashSet<&str> = excluded_ids().into_iter().collect();
        let required = [
            "session.share",
            "prompt.editor",
            "theme.switch",
            "theme.switch_mode",
            "theme.mode.lock",
            "help.show",
            "docs.open",
            "diff.open",
            "opencode.status",
            "plugins.list",
            "plugins.install",
        ];
        for id in required {
            assert!(
                excluded.contains(id),
                "matrix missing excluded command ID: {id}"
            );
        }
    }

    #[test]
    fn matrix_includes_hidden_non_targets() {
        let hidden: HashSet<&str> = hidden_non_target_ids().into_iter().collect();
        let required = [
            "command.palette.show",
            "prompt.clear",
            "prompt.submit",
            "prompt.paste",
            "session.interrupt",
            "terminal.suspend",
            "agent.cycle",
            "agent.cycle.reverse",
            "model.cycle_recent",
            "session.page.up",
            "session.page.down",
            "session.first",
            "session.last",
            "session.message.next",
            "session.message.previous",
            "session.background",
            "session.child.first",
            "session.parent",
            "session.child.next",
            "session.child.previous",
            "session.quick_switch.1",
            "session.quick_switch.2",
            "session.quick_switch.3",
            "session.quick_switch.4",
            "session.quick_switch.5",
            "session.quick_switch.6",
            "session.quick_switch.7",
            "session.quick_switch.8",
            "session.quick_switch.9",
        ];
        for id in required {
            assert!(
                hidden.contains(id),
                "matrix missing hidden non-target command ID: {id}"
            );
        }
    }

    #[test]
    fn excluded_and_included_are_disjoint() {
        let included: HashSet<&str> = included_ids().into_iter().collect();
        let excluded: HashSet<&str> = excluded_ids().into_iter().collect();
        let overlap: Vec<&str> = included.intersection(&excluded).copied().collect();
        assert!(
            overlap.is_empty(),
            "included and excluded sets overlap: {overlap:?}"
        );
    }
}

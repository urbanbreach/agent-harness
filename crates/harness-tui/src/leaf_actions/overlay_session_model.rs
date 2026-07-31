//! Overlay/session/model action leaf for TUI interaction shards.
//!
//! Deterministic action leaf that maps overlay/session/model actions to their
//! real backend owners. No app-state or registry dependency — all types are
//! plain `Copy` value objects.

/// A real backend owner module and function for an action.
///
/// Each overlay/session/model action that opens or closes a surface routes to
/// a real backend function in the `app` module tree. This struct names that
/// owner so the integrator (Todo 28) can wire the action to the real handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendOwner {
    /// Module path relative to `crates/harness-tui/src/`.
    pub module: &'static str,
    /// Function name within the module.
    pub function: &'static str,
}

/// Overlay kind for routing actions to the correct overlay surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayKind {
    /// No overlay (non-open actions).
    #[default]
    None,
    /// Command palette overlay (OVL-PALETTE).
    Palette,
    /// Session navigation overlay (OVL-SESSION).
    Session,
    /// Model switcher overlay.
    Model,
    /// Toggles menu overlay.
    Toggles,
    /// Connect/auth dialog overlay.
    Connect,
}

/// Focus owner after an overlay action.
///
/// Determines which surface receives keyboard focus after the action is
/// dispatched. `CloseOverlay` and `RestoreFocus` always restore focus to
/// the composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LeafFocusOwner {
    /// Composer area (default focus target).
    #[default]
    Composer,
    /// Command palette overlay.
    Palette,
    /// Session navigation overlay.
    Session,
    /// Model switcher overlay.
    Model,
}

/// Deterministic action leaf for overlay/session/model interaction shards.
///
/// Each variant maps to a real backend owner via [`backend_owner`](Self::backend_owner).
/// Navigation actions (`NavigateUp`, `NavigateDown`, `SelectCurrent`) are
/// handled by the overlay's own key handler and do not have a single backend
/// owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlaySessionModelAction {
    /// No action (default).
    #[default]
    None,
    /// Open the command palette (OVL-PALETTE).
    OpenPalette,
    /// Open the session history picker (OVL-SESSION).
    OpenSessionHistory,
    /// Open the fork selector (OVL-SESSION).
    OpenForkSelector,
    /// Open the lineage browser / session tree (OVL-SESSION).
    OpenLineageBrowser,
    /// Open the model switcher overlay.
    OpenModelSwitcher,
    /// Open the toggles menu overlay.
    OpenTogglesMenu,
    /// Open the connect/auth dialog overlay.
    OpenConnectDialog,
    /// Open the session rename dialog (OVL-SESSION).
    OpenSessionRename,
    /// Close the current overlay (Escape).
    CloseOverlay,
    /// Navigate selection up within the overlay.
    NavigateUp,
    /// Navigate selection down within the overlay.
    NavigateDown,
    /// Select the current entry (Enter).
    SelectCurrent,
    /// Restore focus to the previous owner.
    RestoreFocus,
    /// Start a new session.
    NewSession,
    /// Start a new worktree session.
    NewWorktreeSession,
    /// Compact the current session.
    CompactSession,
    /// Copy the session transcript.
    CopyTranscript,
}

impl OverlaySessionModelAction {
    /// Returns the real backend owner for this action, if any.
    ///
    /// Open actions name the real `app::*` module and function that handles
    /// the overlay. Close/restore actions name `session_navigation::close_palette`.
    /// Navigation actions return `None` (handled by the overlay key handler).
    pub fn backend_owner(&self) -> Option<BackendOwner> {
        match self {
            Self::None => None,
            Self::OpenPalette => Some(BackendOwner {
                module: "app::palette_controller",
                function: "dispatch_palette_command",
            }),
            Self::OpenSessionHistory => Some(BackendOwner {
                module: "app::session_history",
                function: "begin_session_history_picker",
            }),
            Self::OpenForkSelector => Some(BackendOwner {
                module: "app::lineage",
                function: "open_fork_selector",
            }),
            Self::OpenLineageBrowser => Some(BackendOwner {
                module: "app::lineage",
                function: "open_lineage_browser",
            }),
            Self::OpenModelSwitcher => Some(BackendOwner {
                module: "app::model_switcher",
                function: "open_model_switcher",
            }),
            Self::OpenTogglesMenu => Some(BackendOwner {
                module: "app::toggles",
                function: "open_toggles_menu",
            }),
            Self::OpenConnectDialog => Some(BackendOwner {
                module: "app::auth_dialog::lifecycle",
                function: "open_connect_dialog",
            }),
            Self::OpenSessionRename => Some(BackendOwner {
                module: "app::session_history",
                function: "open_session_rename_dialog",
            }),
            Self::CloseOverlay | Self::RestoreFocus => Some(BackendOwner {
                module: "app::session_navigation",
                function: "close_palette",
            }),
            Self::NewSession => Some(BackendOwner {
                module: "app::session_navigation",
                function: "apply_new_session_launcher_selection",
            }),
            Self::NewWorktreeSession => Some(BackendOwner {
                module: "app::session_navigation",
                function: "request_new_worktree_session",
            }),
            Self::CompactSession => Some(BackendOwner {
                module: "app::session_navigation",
                function: "emit_ui_intent",
            }),
            Self::CopyTranscript => Some(BackendOwner {
                module: "app::palette_controller",
                function: "dispatch_palette_command",
            }),
            Self::NavigateUp | Self::NavigateDown | Self::SelectCurrent => None,
        }
    }

    /// Returns `true` if this action routes to a real backend owner.
    pub fn has_real_owner(&self) -> bool {
        self.backend_owner().is_some()
    }

    /// Returns the overlay kind this action targets.
    pub fn overlay_kind(&self) -> OverlayKind {
        match self {
            Self::OpenPalette => OverlayKind::Palette,
            Self::OpenSessionHistory
            | Self::OpenForkSelector
            | Self::OpenLineageBrowser
            | Self::OpenSessionRename => OverlayKind::Session,
            Self::OpenModelSwitcher => OverlayKind::Model,
            Self::OpenTogglesMenu => OverlayKind::Toggles,
            Self::OpenConnectDialog => OverlayKind::Connect,
            _ => OverlayKind::None,
        }
    }

    /// Returns the focus owner that should be active after this action.
    ///
    /// Open actions set focus to their overlay. Close/restore actions
    /// return focus to the composer.
    pub fn restored_focus(&self) -> LeafFocusOwner {
        match self {
            Self::OpenPalette => LeafFocusOwner::Palette,
            Self::OpenSessionHistory
            | Self::OpenForkSelector
            | Self::OpenLineageBrowser
            | Self::OpenSessionRename => LeafFocusOwner::Session,
            Self::OpenModelSwitcher => LeafFocusOwner::Model,
            _ => LeafFocusOwner::Composer,
        }
    }
}

/// A palette entry leaf for validation.
///
/// Mirrors the essential fields of a palette command entry for duplicate
/// and stale detection without pulling in the full `palette_model` registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteEntryLeaf {
    /// Command ID (must be non-empty and unique).
    pub id: &'static str,
    /// Display title (must be non-empty; empty indicates stale).
    pub title: &'static str,
}

impl PaletteEntryLeaf {
    /// Create a new palette entry leaf.
    pub const fn new(id: &'static str, title: &'static str) -> Self {
        Self { id, title }
    }

    /// Returns `true` if this entry is stale (empty title).
    pub fn is_stale(&self) -> bool {
        self.title.is_empty()
    }
}

/// Validation error for palette entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteValidationError {
    /// An entry has an empty ID at the given index.
    EmptyId {
        /// Index of the offending entry.
        index: usize,
    },
    /// Two entries share the same ID.
    DuplicateId {
        /// The duplicated ID.
        id: &'static str,
        /// Index of the first occurrence.
        first: usize,
        /// Index of the second occurrence.
        second: usize,
    },
    /// An entry has an empty title (stale entry).
    StaleEntry {
        /// ID of the stale entry.
        id: &'static str,
    },
}

/// Validate palette entries for empty IDs, duplicates, and stale entries.
///
/// Returns `Ok(())` if all entries are valid, or the first error found.
pub fn validate_palette_entries(
    entries: &[PaletteEntryLeaf],
) -> Result<(), PaletteValidationError> {
    for (i, entry) in entries.iter().enumerate() {
        if entry.id.is_empty() {
            return Err(PaletteValidationError::EmptyId { index: i });
        }
        if entry.is_stale() {
            return Err(PaletteValidationError::StaleEntry { id: entry.id });
        }
        for (j, other) in entries.iter().enumerate() {
            if i < j && entry.id == other.id {
                return Err(PaletteValidationError::DuplicateId {
                    id: entry.id,
                    first: i,
                    second: j,
                });
            }
        }
    }
    Ok(())
}

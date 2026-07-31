//! Permission leaf view.

/// Deterministic view state for a permission prompt row.
///
/// No app-state or registry dependency — a plain `Copy` value type.
/// Captures the permission kind, decision state, and ordering relative
/// to the tool call it gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PermissionLeafView {
    pub kind: PermissionKindLeaf,
    pub state: PermissionStateLeaf,
    pub tool_call_id: &'static str,
    pub summary: &'static str,
}

/// Lightweight permission kind label mirroring the canonical permission
/// names without the full permission-system dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionKindLeaf {
    #[default]
    Edit,
    Bash,
    Task,
    Webfetch,
    Websearch,
    Codesearch,
    Lsp,
}

/// Lightweight permission lifecycle state mirroring
/// `app::permissions::PermissionState` without the full modal dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionStateLeaf {
    #[default]
    Pending,
    Granted,
    Denied,
    TimedOut,
    Cancelled,
}

impl PermissionLeafView {
    pub const fn new(
        kind: PermissionKindLeaf,
        state: PermissionStateLeaf,
        tool_call_id: &'static str,
    ) -> Self {
        Self {
            kind,
            state,
            tool_call_id,
            summary: "",
        }
    }

    /// Mark the permission as granted.
    pub const fn granted(mut self) -> Self {
        self.state = PermissionStateLeaf::Granted;
        self
    }

    /// Mark the permission as denied.
    pub const fn denied(mut self) -> Self {
        self.state = PermissionStateLeaf::Denied;
        self
    }

    /// Mark the permission as cancelled (e.g. user pressed Esc).
    pub const fn cancelled(mut self) -> Self {
        self.state = PermissionStateLeaf::Cancelled;
        self
    }

    /// Returns true when the permission was resolved (granted or denied)
    /// before the tool executed — the canonical ordering invariant.
    pub fn resolved_before_tool(&self) -> bool {
        matches!(
            self.state,
            PermissionStateLeaf::Granted | PermissionStateLeaf::Denied
        )
    }

    /// Returns true when the permission is still pending (tool must not
    /// execute yet).
    pub fn is_pending(&self) -> bool {
        matches!(self.state, PermissionStateLeaf::Pending)
    }
}

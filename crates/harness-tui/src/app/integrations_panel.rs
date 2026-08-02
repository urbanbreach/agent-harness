//! Integrations panel state for inspecting MCP servers and extensions.
//!
//! This module backs the integrations panel overlay surface, which allows
//! operators to inspect configured MCP servers, extension manifests, and
//! tool plugin registrations. The panel is read-only and replay-safe.

/// A single MCP server entry displayed in the integrations panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerEntry {
    /// Server identifier from config.
    pub id: String,
    /// Whether the server is enabled.
    pub enabled: bool,
    /// Number of discovered tools (0 if not yet probed).
    pub tool_count: usize,
}

/// A single extension entry displayed in the integrations panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionEntry {
    /// Extension manifest identifier.
    pub id: String,
    /// Extension version string.
    pub version: String,
    /// Whether the extension is active.
    pub active: bool,
}

/// State for the integrations panel overlay.
///
/// The panel is read-only: it displays MCP server and extension metadata
/// derived from config and does not mutate runtime state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IntegrationsPanelState {
    /// Whether the panel overlay is currently visible.
    pub visible: bool,
    /// MCP server entries to display (may be empty when none configured).
    pub mcp_servers: Vec<McpServerEntry>,
    /// Extension entries to display (may be empty when none installed).
    pub extensions: Vec<ExtensionEntry>,
    /// Index of the currently selected entry (across both lists).
    pub selected: usize,
    /// Filter input for narrowing entries.
    pub filter_input: String,
}

impl IntegrationsPanelState {
    /// Move the selection by `delta` positions, clamping at boundaries.
    pub fn move_selection(&mut self, delta: isize) {
        let total = self.mcp_servers.len() + self.extensions.len();
        if total == 0 {
            self.selected = 0;
            return;
        }
        let amount = delta.unsigned_abs() % total;
        self.selected = if delta < 0 {
            if amount <= self.selected {
                self.selected - amount
            } else {
                total - (amount - self.selected)
            }
        } else {
            (self.selected + amount) % total
        };
    }

    /// Total number of entries across MCP servers and extensions.
    pub fn total_entries(&self) -> usize {
        self.mcp_servers.len() + self.extensions.len()
    }
}

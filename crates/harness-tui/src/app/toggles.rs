use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::AppState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TogglesConfig {
    pub entries: Vec<ToggleEntryConfig>,
}

impl Default for TogglesConfig {
    fn default() -> Self {
        Self {
            entries: default_toggle_entries(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToggleEntryConfig {
    pub kind: ToggleEntryKind,
    pub label: String,
    pub description: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToggleEntryKind {
    Agent { name: String },
    Subagent { name: String },
    DynamicPrompt { name: String },
    Hook { id: String },
    YoloMode,
    McpServer { name: String },
    AgentTool { agent: String, tool: String },
    AgentSkill { agent: String, skill: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToggleMenuRow {
    pub index: usize,
    pub section: &'static str,
    pub label: String,
    pub description: String,
    pub enabled: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeTogglesState {
    entries: Vec<ToggleEntryState>,
}

impl Default for RuntimeTogglesState {
    fn default() -> Self {
        Self::from_config(TogglesConfig::default())
    }
}

impl RuntimeTogglesState {
    fn from_config(config: TogglesConfig) -> Self {
        let mut entries: Vec<ToggleEntryState> = Vec::new();
        for entry in config.entries {
            let state = ToggleEntryState::from_config(entry);
            if !entries
                .iter()
                .any(|existing| existing.same_identity(&state))
            {
                entries.push(state);
            }
        }
        Self { entries }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToggleEntryState {
    kind: ToggleEntryKind,
    label: String,
    description: String,
    enabled: bool,
}

impl ToggleEntryState {
    fn from_config(config: ToggleEntryConfig) -> Self {
        Self {
            kind: config.kind,
            label: config.label,
            description: config.description,
            enabled: config.enabled,
        }
    }

    fn same_identity(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl ToggleEntryKind {
    fn section(&self) -> &'static str {
        match self {
            Self::Agent { .. } => "Agents",
            Self::Subagent { .. } => "Subagents",
            Self::DynamicPrompt { .. } => "Dynamic prompts",
            Self::Hook { .. } => "Hooks",
            Self::YoloMode => "Safety",
            Self::McpServer { .. } => "MCP servers",
            Self::AgentTool { .. } => "Agent tools",
            Self::AgentSkill { .. } => "Agent skills",
        }
    }
}

impl AppState {
    pub fn set_toggles_config(&mut self, config: TogglesConfig) {
        let previous_entries = std::mem::take(&mut self.runtime_toggles.entries);
        let mut next = RuntimeTogglesState::from_config(config);
        for entry in previous_entries {
            if !next
                .entries
                .iter()
                .any(|existing| existing.same_identity(&entry))
            {
                next.entries.push(entry);
            }
        }
        self.runtime_toggles = next;
        self.toggles_selected = self
            .toggles_selected
            .min(self.filtered_toggle_entry_indices().len().saturating_sub(1));
    }

    pub(in crate::app) fn open_toggles_menu(&mut self) {
        if !self.overlay_state.palette_visible {
            self.palette_focus_return = Some(self.focus);
        }
        self.overlay_state.palette_visible = true;
        self.overlay_state.session_history_visible = false;
        self.overlay_state.model_switcher_visible = false;
        self.overlay_state.lineage_browser_visible = false;
        self.overlay_state.fork_selector_visible = false;
        self.overlay_state.toggles_menu_visible = true;
        self.toggles_yolo_confirm_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.toggles_selected = 0;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn handle_toggles_key(&mut self, key: &KeyEvent) -> bool {
        if self.toggles_yolo_confirm_visible {
            return self.handle_yolo_confirmation_key(key);
        }

        let ctrl_only = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Esc => {
                self.close_palette();
                true
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.toggle_selected_toggle_entry();
                true
            }
            KeyCode::PageUp => {
                self.move_toggles_selection(-10);
                true
            }
            KeyCode::PageDown => {
                self.move_toggles_selection(10);
                true
            }
            KeyCode::Home => {
                self.toggles_selected = 0;
                true
            }
            KeyCode::End => {
                self.toggles_selected =
                    self.filtered_toggle_entry_indices().len().saturating_sub(1);
                true
            }
            KeyCode::Up => {
                self.move_toggles_selection(-1);
                true
            }
            KeyCode::Down => {
                self.move_toggles_selection(1);
                true
            }
            KeyCode::Backspace => {
                self.overlay_backspace(Self::update_toggles_filter);
                true
            }
            KeyCode::Delete => {
                self.overlay_delete(Self::update_toggles_filter);
                true
            }
            KeyCode::Char('p') if ctrl_only => {
                self.move_toggles_selection(-1);
                true
            }
            KeyCode::Char('n') if ctrl_only => {
                self.move_toggles_selection(1);
                true
            }
            KeyCode::Char(c) => {
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    return false;
                }
                self.overlay_insert_char(c, Self::update_toggles_filter);
                true
            }
            _ => false,
        }
    }

    pub fn toggle_menu_rows(&self) -> Vec<ToggleMenuRow> {
        self.filtered_toggle_entry_indices()
            .into_iter()
            .enumerate()
            .filter_map(|(selected_index, index)| {
                let entry = self.runtime_toggles.entries.get(index)?;
                Some(ToggleMenuRow {
                    index,
                    section: entry.kind.section(),
                    label: entry.label.clone(),
                    description: entry.description.clone(),
                    enabled: entry.enabled,
                    selected: selected_index == self.toggles_selected,
                })
            })
            .collect()
    }

    pub fn toggles_yolo_confirmation_visible(&self) -> bool {
        self.toggles_yolo_confirm_visible
    }

    pub(in crate::app) fn primary_agent_enabled(&self, profile: &str) -> bool {
        self.runtime_toggles
            .entries
            .iter()
            .find(|entry| matches!(&entry.kind, ToggleEntryKind::Agent { name } if name == profile))
            .map(|entry| entry.enabled)
            .unwrap_or(true)
    }

    pub(in crate::app) fn seed_toggles_from_launch_metadata(&mut self) {
        let primary_profiles = self
            .launch_metadata
            .switchable_profiles()
            .iter()
            .filter_map(|profile| {
                let trimmed = profile.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            })
            .collect::<Vec<_>>();
        for profile in &primary_profiles {
            self.add_toggle_entry_if_missing(ToggleEntryState {
                kind: ToggleEntryKind::Agent {
                    name: profile.clone(),
                },
                label: profile.clone(),
                description: "Primary agent".to_string(),
                enabled: true,
            });
        }

        let subagent_profiles = self
            .launch_metadata
            .available_models()
            .iter()
            .map(|option| option.profile.clone())
            .filter(|profile| !primary_profiles.contains(profile))
            .collect::<Vec<_>>();
        for profile in subagent_profiles {
            if profile.trim().is_empty() {
                continue;
            }
            self.add_toggle_entry_if_missing(ToggleEntryState {
                kind: ToggleEntryKind::Subagent {
                    name: profile.clone(),
                },
                label: profile,
                description: "Subagent profile".to_string(),
                enabled: true,
            });
        }
    }

    fn handle_yolo_confirmation_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.toggles_yolo_confirm_visible = false;
                true
            }
            KeyCode::Enter => {
                self.enable_yolo_mode();
                self.toggles_yolo_confirm_visible = false;
                true
            }
            _ => true,
        }
    }

    fn move_toggles_selection(&mut self, delta: isize) {
        let len = self.filtered_toggle_entry_indices().len();
        if len == 0 {
            self.toggles_selected = 0;
            return;
        }
        if delta == -1 {
            self.toggles_selected = if self.toggles_selected == 0 {
                len - 1
            } else {
                self.toggles_selected - 1
            };
            return;
        }
        if delta == 1 {
            self.toggles_selected = (self.toggles_selected + 1) % len;
            return;
        }
        let current = self.toggles_selected.min(len.saturating_sub(1)) as isize;
        let next = (current + delta).clamp(0, len.saturating_sub(1) as isize);
        self.toggles_selected = usize::try_from(next).unwrap_or(0);
    }

    fn update_toggles_filter(&mut self) {
        self.toggles_selected = self
            .toggles_selected
            .min(self.filtered_toggle_entry_indices().len().saturating_sub(1));
    }

    fn toggle_selected_toggle_entry(&mut self) {
        let Some(entry_index) = self
            .filtered_toggle_entry_indices()
            .get(self.toggles_selected)
            .copied()
        else {
            return;
        };
        let Some(entry) = self.runtime_toggles.entries.get_mut(entry_index) else {
            return;
        };
        if matches!(entry.kind, ToggleEntryKind::YoloMode) && !entry.enabled {
            self.toggles_yolo_confirm_visible = true;
            return;
        }
        entry.enabled = !entry.enabled;
    }

    fn enable_yolo_mode(&mut self) {
        for entry in &mut self.runtime_toggles.entries {
            entry.enabled = true;
        }
    }

    fn add_toggle_entry_if_missing(&mut self, entry: ToggleEntryState) {
        if self
            .runtime_toggles
            .entries
            .iter()
            .any(|existing| existing.same_identity(&entry))
        {
            return;
        }
        self.runtime_toggles.entries.push(entry);
    }

    fn filtered_toggle_entry_indices(&self) -> Vec<usize> {
        let input = self.palette_input.to_lowercase();
        self.runtime_toggles
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                let matches = input.is_empty()
                    || entry.label.to_lowercase().contains(&input)
                    || entry.description.to_lowercase().contains(&input)
                    || entry.kind.section().to_lowercase().contains(&input);
                matches.then_some(index)
            })
            .collect()
    }
}

fn default_toggle_entries() -> Vec<ToggleEntryConfig> {
    vec![
        ToggleEntryConfig {
            kind: ToggleEntryKind::DynamicPrompt {
                name: "builtins".to_string(),
            },
            label: "Built-in dynamic prompts".to_string(),
            description: "Runtime-synthesized prompts for shipped agents".to_string(),
            enabled: true,
        },
        ToggleEntryConfig {
            kind: ToggleEntryKind::DynamicPrompt {
                name: "project_instructions".to_string(),
            },
            label: "Project instructions".to_string(),
            description: "Include discovered AGENTS.md instructions".to_string(),
            enabled: true,
        },
        ToggleEntryConfig {
            kind: ToggleEntryKind::YoloMode,
            label: "YOLO mode".to_string(),
            description: "Mark all menu entries on after confirmation".to_string(),
            enabled: false,
        },
    ]
}

// allow: SIZE_OK — TUI app state (session projection + interaction)
use crate::UnwrapOrAbort;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use harness_core::file_tag::{
    FileTagSource, SelectedAgentTag, SelectedFileTag, SelectedResourceTag,
};
use ratatui::layout::Rect;

use super::{rect_contains, AppState, Focus};

mod entries;

use entries::{
    file_mention_agent_entries, file_mention_resource_entries, ripgrep_workspace_files,
    scan_workspace_entries, search_file_mentions, selected_file_mention_tag,
};

const FILE_MENTION_RESULT_LIMIT: usize = 10;
const ALWAYS_SKIPPED_DIRS: &[&str] = &[
    ".git",
    ".agent-harness/sessions",
    "target",
    "node_modules",
    "dist",
    "build",
    "vendor",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileMentionEntry {
    pub display: String,
    pub value: String,
    pub path: String,
    pub is_directory: bool,
    kind: FileMentionEntryKind,
    search_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FileMentionEntryKind {
    File,
    Agent {
        name: String,
    },
    Resource {
        name: String,
        uri: String,
        mime: String,
        description: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileMentionIndex {
    root: PathBuf,
    entries: Vec<FileMentionEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileMentionTag {
    pub start: usize,
    pub end: usize,
    pub part: FileMentionSelectedTag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileMentionSelectedTag {
    File(SelectedFileTag),
    Agent(SelectedAgentTag),
    Resource(SelectedResourceTag),
}

impl FileMentionSelectedTag {
    fn source_mut(&mut self) -> &mut FileTagSource {
        match self {
            Self::File(tag) => &mut tag.source,
            Self::Agent(tag) => &mut tag.source,
            Self::Resource(tag) => &mut tag.source,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct FileMentionFrecency {
    frequency: u32,
    last_used_unix: u64,
}

impl FileMentionFrecency {
    fn score(self) -> u64 {
        u64::from(self.frequency).saturating_mul(1_000_000_000) + self.last_used_unix
    }
}

pub(crate) trait FileMentionWorkspaceScanner: Send + Sync {
    fn workspace_files(&self, workspace_root: &Path) -> Option<Vec<String>>;
}

#[derive(Debug, Default)]
pub(crate) struct SystemFileMentionWorkspaceScanner;

impl FileMentionWorkspaceScanner for SystemFileMentionWorkspaceScanner {
    fn workspace_files(&self, workspace_root: &Path) -> Option<Vec<String>> {
        ripgrep_workspace_files(workspace_root)
    }
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct FixedFileMentionWorkspaceScanner {
    files: Vec<String>,
}

#[cfg(test)]
impl FixedFileMentionWorkspaceScanner {
    pub(crate) fn new(files: Vec<String>) -> Self {
        Self { files }
    }
}

#[cfg(test)]
impl FileMentionWorkspaceScanner for FixedFileMentionWorkspaceScanner {
    fn workspace_files(&self, _workspace_root: &Path) -> Option<Vec<String>> {
        Some(self.files.clone())
    }
}

pub(crate) fn system_file_mention_workspace_root() -> Option<PathBuf> {
    std::env::current_dir().ok()
}

pub(crate) fn system_file_mention_now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

impl FileMentionIndex {
    fn build(root: PathBuf, scanner: &dyn FileMentionWorkspaceScanner) -> Self {
        Self {
            entries: scan_workspace_entries(&root, scanner),
            root,
        }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn entries(&self) -> &[FileMentionEntry] {
        &self.entries
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActiveFileMention {
    trigger: usize,
    cursor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineRangeQuery<'a> {
    base_query: &'a str,
    suffix: Option<&'a str>,
}

impl AppState {
    pub(in crate::app) fn clear_file_mention_menu(&mut self) {
        self.file_mention_visible = false;
        self.file_mention_entries.clear();
        self.file_mention_selected = 0;
        self.file_mention_trigger = None;
    }

    pub(in crate::app) fn file_mention_overlay_should_render(&self) -> bool {
        self.file_mention_visible
    }

    pub(in crate::app) fn sync_file_mention_overlay(&mut self) {
        let Some(active) = self.active_file_mention() else {
            self.clear_file_mention_menu();
            return;
        };

        if self.focus != Focus::Prompt
            || self.composer_disabled()
            || self.palette_visible
            || self.session_history_visible
            || self.model_switcher_visible
            || self.active_permission().is_some()
        {
            self.clear_file_mention_menu();
            return;
        }

        let query = self
            .prompt_slice(active.trigger + 1, active.cursor)
            .to_string();
        let workspace_root = self.file_mention_workspace_root();
        let file_mention_entries = {
            let frecency = self.file_mention_frecency.clone();
            let mut entries = file_mention_agent_entries(&self.launch_metadata);
            entries.extend_from_slice(self.file_mention_index_entries(&workspace_root));
            entries.extend(file_mention_resource_entries(
                self.launch_metadata.mcp_resources(),
            ));
            search_file_mentions(&entries, &query, &frecency)
        };
        self.file_mention_visible = true;
        self.file_mention_trigger = Some(active.trigger);
        self.file_mention_entries = file_mention_entries;
        self.file_mention_selected = self
            .file_mention_selected
            .min(self.file_mention_entries.len().saturating_sub(1));
    }

    pub(in crate::app) fn handle_file_mention_key(&mut self, key: &KeyEvent) -> bool {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.clear_file_mention_menu();
                true
            }
            (KeyCode::Enter, _) => {
                self.apply_selected_file_mention(false);
                true
            }
            (KeyCode::Tab, _) => {
                let expand_directory = self
                    .file_mention_entries
                    .get(self.file_mention_selected)
                    .is_some_and(|entry| entry.is_directory);
                self.apply_selected_file_mention(expand_directory);
                true
            }
            (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                self.move_file_mention_selection(-1);
                true
            }
            (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                self.move_file_mention_selection(1);
                true
            }
            _ => false,
        }
    }

    pub(in crate::app) fn handle_file_mention_mouse(&mut self, mouse: MouseEvent, overlay: Rect) {
        let list_area = crate::layout::completion_overlay_content_area(overlay);
        if self.file_mention_entries.is_empty()
            || list_area.height == 0
            || !rect_contains(list_area, mouse.column, mouse.row)
        {
            return;
        }

        let visible_rows = usize::from(list_area.height);
        let selected = self
            .file_mention_selected
            .min(self.file_mention_entries.len().saturating_sub(1));
        let scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
        let row = usize::from(mouse.row.saturating_sub(list_area.y));
        let Some(next) = scroll
            .checked_add(row)
            .filter(|index| *index < self.file_mention_entries.len())
        else {
            return;
        };

        match mouse.kind {
            MouseEventKind::Moved
            | MouseEventKind::Drag(MouseButton::Left)
            | MouseEventKind::Down(MouseButton::Left) => {
                self.file_mention_selected = next;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.file_mention_selected = next;
                self.apply_selected_file_mention(false);
            }
            _ => {}
        }
    }

    fn active_file_mention(&self) -> Option<ActiveFileMention> {
        if self.composer.prompt_cursor == 0 {
            return None;
        }

        let text_before_cursor = self.prompt_slice(0, self.composer.prompt_cursor);
        let mut trigger = None;
        for (index, ch) in text_before_cursor.chars().enumerate() {
            if ch == '@' {
                trigger = Some(index);
            }
        }
        let trigger = trigger?;
        let before = (trigger > 0).then(|| self.prompt_slice(trigger - 1, trigger));
        if before.is_some_and(|value| !value.chars().all(char::is_whitespace)) {
            return None;
        }

        let between = self.prompt_slice(trigger, self.composer.prompt_cursor);
        if between.chars().any(char::is_whitespace) {
            return None;
        }

        Some(ActiveFileMention {
            trigger,
            cursor: self.composer.prompt_cursor,
        })
    }

    fn apply_selected_file_mention(&mut self, expand_directory: bool) {
        let Some(entry) = self
            .file_mention_entries
            .get(self.file_mention_selected)
            .cloned()
        else {
            return;
        };
        let Some(trigger) = self.file_mention_trigger else {
            self.clear_file_mention_menu();
            return;
        };

        let token = if expand_directory {
            format!("@{}", entry.path)
        } else {
            let suffix = if self.char_after_prompt_cursor() == Some(' ') {
                ""
            } else {
                " "
            };
            format!("@{}{}", entry.value, suffix)
        };
        let tag_end = trigger + 1 + entry.value.chars().count();
        self.replace_prompt_range(trigger, self.composer.prompt_cursor, &token);
        self.composer.prompt_cursor = trigger + token.chars().count();

        if expand_directory {
            self.file_mention_selected = 0;
            self.sync_file_mention_overlay();
        } else {
            self.record_file_mention_frecency(&entry.path);
            self.add_file_mention_tag(
                trigger,
                tag_end,
                selected_file_mention_tag(
                    &entry,
                    trigger,
                    tag_end,
                    &self.file_mention_workspace_root(),
                ),
            );
            self.clear_file_mention_menu();
        }
    }

    pub(in crate::app) fn clear_file_mention_tags(&mut self) {
        self.file_mention_tags.clear();
    }

    pub(in crate::app) fn add_file_mention_tag(
        &mut self,
        start: usize,
        end: usize,
        part: FileMentionSelectedTag,
    ) {
        if start >= end {
            return;
        }
        self.file_mention_tags
            .retain(|tag| tag.end <= start || tag.start >= end);
        self.file_mention_tags
            .push(FileMentionTag { start, end, part });
        self.file_mention_tags.sort_by_key(|tag| tag.start);
    }

    pub(in crate::app) fn adjust_file_mention_tags_for_insert(&mut self, at: usize, len: usize) {
        if len == 0 || self.file_mention_tags.is_empty() {
            return;
        }
        let mut adjusted = Vec::with_capacity(self.file_mention_tags.len());
        for mut tag in self.file_mention_tags.drain(..) {
            if at <= tag.start {
                tag.start += len;
                tag.end += len;
                let source = tag.part.source_mut();
                source.start += len;
                source.end += len;
                adjusted.push(tag);
            } else if at < tag.end {
                // Harness extmarks represent selected parts. If the user edits inside a
                // selected part, stop treating that stale range as a selected tag.
            } else {
                adjusted.push(tag);
            }
        }
        self.file_mention_tags = adjusted;
    }

    pub(in crate::app) fn adjust_file_mention_tags_for_delete(&mut self, start: usize, end: usize) {
        if start >= end || self.file_mention_tags.is_empty() {
            return;
        }
        let len = end - start;
        let mut adjusted = Vec::with_capacity(self.file_mention_tags.len());
        for mut tag in self.file_mention_tags.drain(..) {
            if tag.end <= start {
                adjusted.push(tag);
            } else if tag.start >= end {
                tag.start = tag.start.saturating_sub(len);
                tag.end = tag.end.saturating_sub(len);
                let source = tag.part.source_mut();
                source.start = source.start.saturating_sub(len);
                source.end = source.end.saturating_sub(len);
                adjusted.push(tag);
            }
        }
        self.file_mention_tags = adjusted;
    }

    fn move_file_mention_selection(&mut self, delta: isize) {
        let len = self.file_mention_entries.len();
        if len == 0 {
            self.file_mention_selected = 0;
            return;
        }
        if delta == -1 {
            self.file_mention_selected = if self.file_mention_selected == 0 {
                len - 1
            } else {
                self.file_mention_selected - 1
            };
        } else if delta == 1 {
            self.file_mention_selected = (self.file_mention_selected + 1) % len;
        }
    }

    fn file_mention_workspace_root(&self) -> PathBuf {
        self.file_mention_workspace_root
            .clone()
            .or_else(|| (self.file_mention_workspace_root_provider)())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn file_mention_index_entries(&mut self, workspace_root: &Path) -> &[FileMentionEntry] {
        let rebuild = self
            .file_mention_index
            .as_ref()
            .is_none_or(|index| index.root() != workspace_root);
        if rebuild {
            let scanner = std::sync::Arc::clone(&self.file_mention_scanner);
            self.file_mention_index = Some(FileMentionIndex::build(
                workspace_root.to_path_buf(),
                scanner.as_ref(),
            ));
        }
        self.file_mention_index
            .as_ref()
            .map(FileMentionIndex::entries)
            .unwrap_or(&[])
    }

    fn prompt_slice(&self, start: usize, end: usize) -> &str {
        let start_byte = prompt_char_to_byte(&self.composer.prompt_buffer, start);
        let end_byte = prompt_char_to_byte(&self.composer.prompt_buffer, end);
        &self.composer.prompt_buffer[start_byte..end_byte]
    }

    fn replace_prompt_range(&mut self, start: usize, end: usize, replacement: &str) {
        let start_byte = prompt_char_to_byte(&self.composer.prompt_buffer, start);
        let end_byte = prompt_char_to_byte(&self.composer.prompt_buffer, end);
        self.adjust_file_mention_tags_for_delete(start, end);
        self.adjust_file_mention_tags_for_insert(start, replacement.chars().count());
        self.composer
            .prompt_buffer
            .replace_range(start_byte..end_byte, replacement);
    }

    fn char_after_prompt_cursor(&self) -> Option<char> {
        self.composer
            .prompt_buffer
            .chars()
            .nth(self.composer.prompt_cursor)
    }

    #[cfg(test)]
    pub(crate) fn set_file_mention_workspace_root_for_test(&mut self, root: PathBuf) {
        self.file_mention_workspace_root = Some(root);
        self.file_mention_index = None;
    }

    #[cfg(test)]
    pub(crate) fn set_file_mention_collaborators_for_test(
        &mut self,
        root: PathBuf,
        files: Vec<String>,
        now_unix: u64,
    ) {
        self.file_mention_workspace_root = Some(root);
        self.file_mention_scanner =
            std::sync::Arc::new(FixedFileMentionWorkspaceScanner::new(files));
        self.file_mention_now_unix = std::sync::Arc::new(move || now_unix);
        self.file_mention_index = None;
    }

    #[cfg(test)]
    pub(crate) fn file_mention_frecency_for_test(&self, path: &str) -> Option<(u32, u64)> {
        self.file_mention_frecency
            .get(path)
            .map(|entry| (entry.frequency, entry.last_used_unix))
    }

    pub(crate) fn selected_file_tags(&self) -> Vec<SelectedFileTag> {
        self.file_mention_tags
            .iter()
            .filter_map(|tag| match &tag.part {
                FileMentionSelectedTag::File(part) => Some(part.clone()),
                FileMentionSelectedTag::Agent(_) | FileMentionSelectedTag::Resource(_) => None,
            })
            .collect()
    }

    pub(crate) fn selected_agent_tags(&self) -> Vec<SelectedAgentTag> {
        self.file_mention_tags
            .iter()
            .filter_map(|tag| match &tag.part {
                FileMentionSelectedTag::Agent(part) => Some(part.clone()),
                FileMentionSelectedTag::File(_) | FileMentionSelectedTag::Resource(_) => None,
            })
            .collect()
    }

    pub(crate) fn selected_resource_tags(&self) -> Vec<SelectedResourceTag> {
        self.file_mention_tags
            .iter()
            .filter_map(|tag| match &tag.part {
                FileMentionSelectedTag::Resource(part) => Some(part.clone()),
                FileMentionSelectedTag::File(_) | FileMentionSelectedTag::Agent(_) => None,
            })
            .collect()
    }

    fn record_file_mention_frecency(&mut self, path: &str) {
        let now = (self.file_mention_now_unix)();
        let entry = self
            .file_mention_frecency
            .entry(path.to_string())
            .or_default();
        entry.frequency = entry.frequency.saturating_add(1);
        entry.last_used_unix = now;
    }
}

fn prompt_char_to_byte(value: &str, char_index: usize) -> usize {
    value
        .char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(value.len())
}

#[cfg(test)]
mod tests {
    use crate::UnwrapOrAbort;
    use std::collections::BTreeMap;

    use super::entries::{extract_line_range, search_file_mentions};
    use super::{FileMentionFrecency, FileMentionIndex, SystemFileMentionWorkspaceScanner};

    #[test]
    fn line_range_parser_matches_harness_suffix_behavior() {
        let parsed = extract_line_range("src/main.rs#12-20");
        assert_eq!(parsed.base_query, "src/main.rs");
        assert_eq!(parsed.suffix, Some("#12-20"));

        let invalid = extract_line_range("src/main.rs#abc");
        assert_eq!(invalid.base_query, "src/main.rs");
        assert_eq!(invalid.suffix, None);
    }

    #[test]
    fn search_file_mentions_returns_directories_for_empty_query() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        std::fs::create_dir(tempdir.path().join("src")).unwrap_or_abort();
        std::fs::write(tempdir.path().join("src/lib.rs"), "lib").unwrap_or_abort();
        std::fs::write(tempdir.path().join("README.md"), "readme").unwrap_or_abort();

        let index = FileMentionIndex::build(
            tempdir.path().to_path_buf(),
            &SystemFileMentionWorkspaceScanner,
        );
        let entries = search_file_mentions(index.entries(), "", &BTreeMap::new());

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].display, "src/");
        assert!(entries[0].is_directory);
    }

    #[test]
    fn search_file_mentions_preserves_valid_line_range_on_file_value() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        std::fs::create_dir(tempdir.path().join("src")).unwrap_or_abort();
        std::fs::write(tempdir.path().join("src/main.rs"), "fn main() {}").unwrap_or_abort();

        let index = FileMentionIndex::build(
            tempdir.path().to_path_buf(),
            &SystemFileMentionWorkspaceScanner,
        );
        let entries = search_file_mentions(index.entries(), "main#3", &BTreeMap::new());

        assert_eq!(entries[0].value, "src/main.rs#3");
        assert_eq!(entries[0].path, "src/main.rs");
    }

    #[test]
    fn search_file_mentions_prefers_shallow_workspace_directory_over_hidden_nested_match() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        std::fs::create_dir_all(tempdir.path().join(".harness/skills/.system/openai-docs"))
            .unwrap_or_abort();
        std::fs::write(
            tempdir
                .path()
                .join(".harness/skills/.system/openai-docs/readme.md"),
            "hidden docs",
        )
        .unwrap_or_abort();
        std::fs::create_dir(tempdir.path().join("docs")).unwrap_or_abort();
        std::fs::write(tempdir.path().join("docs/guide.md"), "workspace docs").unwrap_or_abort();

        let index = FileMentionIndex::build(
            tempdir.path().to_path_buf(),
            &SystemFileMentionWorkspaceScanner,
        );
        let entries = search_file_mentions(index.entries(), "docs", &BTreeMap::new());

        assert_eq!(entries[0].value, "docs/");
    }

    #[test]
    fn search_file_mentions_uses_frecency_before_depth_and_path() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        std::fs::create_dir_all(tempdir.path().join("src/deep")).unwrap_or_abort();
        std::fs::write(tempdir.path().join("alpha.rs"), "root").unwrap_or_abort();
        std::fs::write(tempdir.path().join("src/deep/alpha.rs"), "nested").unwrap_or_abort();
        let index = FileMentionIndex::build(
            tempdir.path().to_path_buf(),
            &SystemFileMentionWorkspaceScanner,
        );
        let mut frecency = BTreeMap::new();
        frecency.insert(
            "src/deep/alpha.rs".to_string(),
            FileMentionFrecency {
                frequency: 3,
                last_used_unix: 10,
            },
        );

        let entries = search_file_mentions(index.entries(), "alpha", &frecency);

        assert_eq!(entries[0].value, "src/deep/alpha.rs");
    }
}

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use harness_core::file_tag::{
    split_line_range, FileTagSource, SelectedAgentTag, SelectedFileTag, SelectedResourceTag,
};
use ratatui::layout::Rect;

use super::{rect_contains, session_navigation::McpResourceOption, AppState, Focus};

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

impl FileMentionIndex {
    fn build(root: PathBuf) -> Self {
        Self {
            entries: scan_workspace_entries(&root),
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
        if self.prompt_cursor == 0 {
            return None;
        }

        let text_before_cursor = self.prompt_slice(0, self.prompt_cursor);
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

        let between = self.prompt_slice(trigger, self.prompt_cursor);
        if between.chars().any(char::is_whitespace) {
            return None;
        }

        Some(ActiveFileMention {
            trigger,
            cursor: self.prompt_cursor,
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
        self.replace_prompt_range(trigger, self.prompt_cursor, &token);
        self.prompt_cursor = trigger + token.chars().count();

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
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn file_mention_index_entries(&mut self, workspace_root: &Path) -> &[FileMentionEntry] {
        let rebuild = self
            .file_mention_index
            .as_ref()
            .is_none_or(|index| index.root() != workspace_root);
        if rebuild {
            self.file_mention_index = Some(FileMentionIndex::build(workspace_root.to_path_buf()));
        }
        self.file_mention_index
            .as_ref()
            .map(FileMentionIndex::entries)
            .unwrap_or(&[])
    }

    fn prompt_slice(&self, start: usize, end: usize) -> &str {
        let start_byte = prompt_char_to_byte(&self.prompt_buffer, start);
        let end_byte = prompt_char_to_byte(&self.prompt_buffer, end);
        &self.prompt_buffer[start_byte..end_byte]
    }

    fn replace_prompt_range(&mut self, start: usize, end: usize, replacement: &str) {
        let start_byte = prompt_char_to_byte(&self.prompt_buffer, start);
        let end_byte = prompt_char_to_byte(&self.prompt_buffer, end);
        self.adjust_file_mention_tags_for_delete(start, end);
        self.adjust_file_mention_tags_for_insert(start, replacement.chars().count());
        self.prompt_buffer
            .replace_range(start_byte..end_byte, replacement);
    }

    fn char_after_prompt_cursor(&self) -> Option<char> {
        self.prompt_buffer.chars().nth(self.prompt_cursor)
    }

    #[cfg(test)]
    pub(crate) fn set_file_mention_workspace_root_for_test(&mut self, root: PathBuf) {
        self.file_mention_workspace_root = Some(root);
        self.file_mention_index = None;
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
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        let entry = self
            .file_mention_frecency
            .entry(path.to_string())
            .or_default();
        entry.frequency = entry.frequency.saturating_add(1);
        entry.last_used_unix = now;
    }
}

fn selected_file_mention_tag(
    entry: &FileMentionEntry,
    start: usize,
    end: usize,
    workspace_root: &Path,
) -> FileMentionSelectedTag {
    match &entry.kind {
        FileMentionEntryKind::File => {
            FileMentionSelectedTag::File(selected_file_tag(entry, start, end, workspace_root))
        }
        FileMentionEntryKind::Agent { name } => FileMentionSelectedTag::Agent(SelectedAgentTag {
            name: name.clone(),
            source: FileTagSource {
                start,
                end,
                value: format!("@{}", entry.value),
            },
        }),
        FileMentionEntryKind::Resource {
            name,
            uri,
            mime,
            description,
        } => FileMentionSelectedTag::Resource(SelectedResourceTag {
            name: name.clone(),
            uri: uri.clone(),
            mime: mime.clone(),
            description: description.clone(),
            source: FileTagSource {
                start,
                end,
                value: format!("@{}", entry.value),
            },
        }),
    }
}

fn selected_file_tag(
    entry: &FileMentionEntry,
    start: usize,
    end: usize,
    workspace_root: &Path,
) -> SelectedFileTag {
    let (path, line_range) = split_line_range(&entry.value);
    let path = if entry.is_directory {
        entry.path.clone()
    } else {
        path.to_string()
    };
    let url = file_tag_url(workspace_root, &path, line_range);
    let mime = if entry.is_directory {
        "application/x-directory".to_string()
    } else {
        attachment_mime(&path).to_string()
    };
    SelectedFileTag {
        path,
        filename: entry.value.clone(),
        url,
        mime,
        source: FileTagSource {
            start,
            end,
            value: format!("@{}", entry.value),
        },
        line_range,
    }
}

fn file_mention_agent_entries(launch_metadata: &super::LaunchMetadata) -> Vec<FileMentionEntry> {
    let current = launch_metadata.profile();
    let mut names = BTreeSet::new();
    for option in launch_metadata.available_models() {
        let profile = option.profile.trim();
        if profile.is_empty()
            || profile == current
            || profile == harness_core::session_title::TITLE_AGENT_NAME
        {
            continue;
        }
        names.insert(profile.to_string());
    }
    names.into_iter().map(FileMentionEntry::agent).collect()
}

fn file_mention_resource_entries(resources: &[McpResourceOption]) -> Vec<FileMentionEntry> {
    resources
        .iter()
        .filter(|resource| !resource.uri.trim().is_empty())
        .map(FileMentionEntry::resource)
        .collect()
}

fn file_tag_url(
    workspace_root: &Path,
    path: &str,
    line_range: Option<harness_core::file_tag::FileTagLineRange>,
) -> String {
    let absolute = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        workspace_root.join(path)
    };
    let mut url = format!(
        "file://{}",
        absolute.display().to_string().replace(' ', "%20")
    );
    if let Some(range) = line_range {
        url.push_str(&format!("?start={}", range.start));
        if let Some(end) = range.end {
            url.push_str(&format!("&end={end}"));
        }
    }
    url
}

fn attachment_mime(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("pdf") => "application/pdf",
        _ => "text/plain",
    }
}

fn search_file_mentions(
    entries: &[FileMentionEntry],
    query: &str,
    frecency: &BTreeMap<String, FileMentionFrecency>,
) -> Vec<FileMentionEntry> {
    let range_query = extract_line_range(query);
    let prefer_hidden =
        range_query.base_query.starts_with('.') || range_query.base_query.contains("/.");

    let mut matches = if range_query.base_query.is_empty() {
        let mut matches = entries
            .iter()
            .filter(|entry| !matches!(entry.kind, FileMentionEntryKind::File) || entry.is_directory)
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| file_mention_order(left, right, prefer_hidden, frecency));
        matches
    } else {
        let mut ranked = entries
            .iter()
            .filter_map(|entry| {
                fuzzy_rank(range_query.base_query, &entry.search_key).map(|rank| (rank, entry))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(left_rank, left), (right_rank, right)| {
            frecency_order(left, right, frecency)
                .then_with(|| left_rank.cmp(right_rank))
                .then_with(|| path_order(left, right, prefer_hidden))
        });
        ranked.into_iter().map(|(_, entry)| entry).collect()
    };

    matches
        .drain(..)
        .take(FILE_MENTION_RESULT_LIMIT)
        .map(|entry| {
            let mut entry = entry.clone();
            if matches!(entry.kind, FileMentionEntryKind::File) && !entry.is_directory {
                if let Some(suffix) = range_query.suffix {
                    entry.value.push_str(suffix);
                    entry.display.push_str(suffix);
                }
            }
            entry
        })
        .collect()
}

fn extract_line_range(query: &str) -> LineRangeQuery<'_> {
    let Some(hash_index) = query.rfind('#') else {
        return LineRangeQuery {
            base_query: query,
            suffix: None,
        };
    };
    let suffix = &query[hash_index..];
    let line_part = &suffix[1..];
    if valid_line_range(line_part) {
        LineRangeQuery {
            base_query: &query[..hash_index],
            suffix: Some(suffix),
        }
    } else {
        LineRangeQuery {
            base_query: &query[..hash_index],
            suffix: None,
        }
    }
}

fn valid_line_range(value: &str) -> bool {
    let Some((start, end)) = value.split_once('-') else {
        return !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit());
    };
    !start.is_empty()
        && start.chars().all(|ch| ch.is_ascii_digit())
        && (end.is_empty() || end.chars().all(|ch| ch.is_ascii_digit()))
}

fn scan_workspace_entries(workspace_root: &Path) -> Vec<FileMentionEntry> {
    let files = ripgrep_workspace_files(workspace_root).unwrap_or_else(|| {
        let mut files = Vec::new();
        scan_directory(workspace_root, workspace_root, &mut files);
        files
    });

    entries_from_file_paths(files)
}

fn entries_from_file_paths(files: Vec<String>) -> Vec<FileMentionEntry> {
    let mut files = files
        .into_iter()
        .filter(|file| !file.is_empty() && !should_skip_path(file))
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();

    let mut dirs = BTreeSet::new();
    for file in &files {
        let mut current = Path::new(file.as_str());
        while let Some(parent) = current.parent() {
            let dir = normalize_relative_path(parent);
            if dir.is_empty() {
                break;
            }
            dirs.insert(format!("{dir}/"));
            current = parent;
        }
    }

    let mut entries = files
        .into_iter()
        .map(|file| FileMentionEntry::new(file, false))
        .collect::<Vec<_>>();
    entries.extend(
        dirs.into_iter()
            .map(|directory| FileMentionEntry::new(directory, true)),
    );
    entries
}

fn ripgrep_workspace_files(workspace_root: &Path) -> Option<Vec<String>> {
    let output = Command::new("rg")
        .current_dir(workspace_root)
        .args(["--no-config", "--files", "--hidden", "--glob=!.git/*", "."])
        .output()
        .ok()?;
    if !output.status.success() && output.status.code() != Some(1) {
        return None;
    }

    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(clean_ripgrep_path)
            .filter(|file| !file.is_empty())
            .collect(),
    )
}

fn clean_ripgrep_path(file: &str) -> String {
    let normalized = file.trim().replace('\\', "/");
    normalized
        .strip_prefix("./")
        .unwrap_or(&normalized)
        .to_string()
}

fn scan_directory(workspace_root: &Path, directory: &Path, files: &mut Vec<String>) {
    let Ok(read_dir) = std::fs::read_dir(directory) else {
        return;
    };

    let mut children = read_dir.filter_map(Result::ok).collect::<Vec<_>>();
    children.sort_by_key(|entry| entry.path());

    for child in children {
        let path = child.path();
        let Ok(file_type) = child.file_type() else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(workspace_root) else {
            continue;
        };
        let normalized = normalize_relative_path(relative);
        if should_skip_path(&normalized) {
            continue;
        }

        if file_type.is_dir() {
            scan_directory(workspace_root, &path, files);
        } else if file_type.is_file() {
            files.push(normalized);
        }
    }
}

impl FileMentionEntry {
    fn new(path: String, is_directory: bool) -> Self {
        Self {
            display: path.clone(),
            value: path.clone(),
            search_key: path.to_lowercase(),
            path,
            is_directory,
            kind: FileMentionEntryKind::File,
        }
    }

    fn agent(name: String) -> Self {
        let display = format!("{name} agent");
        Self {
            display: display.clone(),
            value: name.clone(),
            path: format!("agent:{name}"),
            is_directory: false,
            kind: FileMentionEntryKind::Agent { name: name.clone() },
            search_key: format!("{name} {display}").to_lowercase(),
        }
    }

    fn resource(resource: &McpResourceOption) -> Self {
        let name = if resource.name.trim().is_empty() {
            resource.uri.clone()
        } else {
            resource.name.clone()
        };
        let display = format!("{name} resource");
        let search_key = format!(
            "{} {} {} {}",
            name,
            resource.uri,
            resource.mime,
            resource.description.as_deref().unwrap_or_default()
        )
        .to_lowercase();
        Self {
            display,
            value: resource.uri.clone(),
            path: format!("resource:{}", resource.uri),
            is_directory: false,
            kind: FileMentionEntryKind::Resource {
                name,
                uri: resource.uri.clone(),
                mime: resource.mime.clone(),
                description: resource.description.clone(),
            },
            search_key,
        }
    }
}

fn normalize_relative_path(path: &Path) -> String {
    path.iter()
        .map(|segment| segment.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn should_skip_path(path: &str) -> bool {
    ALWAYS_SKIPPED_DIRS
        .iter()
        .any(|skipped| path == *skipped || path.starts_with(&format!("{skipped}/")))
}

fn file_mention_order(
    left: &FileMentionEntry,
    right: &FileMentionEntry,
    prefer_hidden: bool,
    frecency: &BTreeMap<String, FileMentionFrecency>,
) -> Ordering {
    frecency_order(left, right, frecency).then_with(|| path_order(left, right, prefer_hidden))
}

fn frecency_order(
    left: &FileMentionEntry,
    right: &FileMentionEntry,
    frecency: &BTreeMap<String, FileMentionFrecency>,
) -> Ordering {
    frecency
        .get(&right.path)
        .map(|entry| entry.score())
        .unwrap_or_default()
        .cmp(
            &frecency
                .get(&left.path)
                .map(|entry| entry.score())
                .unwrap_or_default(),
        )
}

fn path_order(left: &FileMentionEntry, right: &FileMentionEntry, prefer_hidden: bool) -> Ordering {
    hidden_rank(&left.path, prefer_hidden)
        .cmp(&hidden_rank(&right.path, prefer_hidden))
        .then_with(|| path_depth(&left.path).cmp(&path_depth(&right.path)))
        .then_with(|| left.path.cmp(&right.path))
}

fn hidden_rank(path: &str, prefer_hidden: bool) -> usize {
    usize::from(!prefer_hidden && hidden(path))
}

fn hidden(path: &str) -> bool {
    path.trim_end_matches('/')
        .split('/')
        .any(|part| part.starts_with('.') && part.len() > 1)
}

fn path_depth(path: &str) -> usize {
    path.trim_end_matches('/').split('/').count()
}

fn fuzzy_rank(query: &str, value: &str) -> Option<(usize, usize)> {
    let query = query.to_lowercase();
    if value.starts_with(&query) {
        return Some((0, value.len()));
    }
    if let Some(index) = value.find(&query) {
        return Some((1 + index, value.len()));
    }
    subsequence_rank(&query, value).map(|span| (10_000 + span, value.len()))
}

fn subsequence_rank(query: &str, value: &str) -> Option<usize> {
    let mut first = None;
    let mut last = 0usize;
    let mut value_chars = value.char_indices();
    for query_char in query.chars() {
        let (idx, _) = value_chars.find(|(_, value_char)| *value_char == query_char)?;
        first.get_or_insert(idx);
        last = idx;
    }
    Some(last.saturating_sub(first.unwrap_or(0)))
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
    use std::collections::BTreeMap;

    use super::{extract_line_range, search_file_mentions, FileMentionFrecency, FileMentionIndex};

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
        let tempdir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(tempdir.path().join("src")).expect("create src");
        std::fs::write(tempdir.path().join("src/lib.rs"), "lib").expect("write lib");
        std::fs::write(tempdir.path().join("README.md"), "readme").expect("write readme");

        let index = FileMentionIndex::build(tempdir.path().to_path_buf());
        let entries = search_file_mentions(index.entries(), "", &BTreeMap::new());

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].display, "src/");
        assert!(entries[0].is_directory);
    }

    #[test]
    fn search_file_mentions_preserves_valid_line_range_on_file_value() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(tempdir.path().join("src")).expect("create src");
        std::fs::write(tempdir.path().join("src/main.rs"), "fn main() {}").expect("write main");

        let index = FileMentionIndex::build(tempdir.path().to_path_buf());
        let entries = search_file_mentions(index.entries(), "main#3", &BTreeMap::new());

        assert_eq!(entries[0].value, "src/main.rs#3");
        assert_eq!(entries[0].path, "src/main.rs");
    }

    #[test]
    fn search_file_mentions_prefers_shallow_workspace_directory_over_hidden_nested_match() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tempdir.path().join(".harness/skills/.system/openai-docs"))
            .expect("create hidden docs dir");
        std::fs::write(
            tempdir
                .path()
                .join(".harness/skills/.system/openai-docs/readme.md"),
            "hidden docs",
        )
        .expect("write hidden docs file");
        std::fs::create_dir(tempdir.path().join("docs")).expect("create docs");
        std::fs::write(tempdir.path().join("docs/guide.md"), "workspace docs")
            .expect("write docs file");

        let index = FileMentionIndex::build(tempdir.path().to_path_buf());
        let entries = search_file_mentions(index.entries(), "docs", &BTreeMap::new());

        assert_eq!(entries[0].value, "docs/");
    }

    #[test]
    fn search_file_mentions_uses_frecency_before_depth_and_path() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tempdir.path().join("src/deep")).expect("create nested dir");
        std::fs::write(tempdir.path().join("alpha.rs"), "root").expect("write root");
        std::fs::write(tempdir.path().join("src/deep/alpha.rs"), "nested").expect("write nested");
        let index = FileMentionIndex::build(tempdir.path().to_path_buf());
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

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use harness_core::file_tag::{
    split_line_range, FileTagSource, SelectedAgentTag, SelectedFileTag, SelectedResourceTag,
};

use crate::app::{LaunchMetadata, McpResourceOption};

use super::{
    FileMentionEntry, FileMentionEntryKind, FileMentionFrecency, FileMentionSelectedTag,
    FileMentionWorkspaceScanner, LineRangeQuery, ALWAYS_SKIPPED_DIRS, FILE_MENTION_RESULT_LIMIT,
};

pub(super) fn selected_file_mention_tag(
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

pub(super) fn file_mention_agent_entries(
    launch_metadata: &LaunchMetadata,
) -> Vec<FileMentionEntry> {
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

pub(super) fn file_mention_resource_entries(
    resources: &[McpResourceOption],
) -> Vec<FileMentionEntry> {
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

pub(super) fn search_file_mentions(
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

pub(super) fn extract_line_range(query: &str) -> LineRangeQuery<'_> {
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

pub(super) fn scan_workspace_entries(
    workspace_root: &Path,
    scanner: &dyn FileMentionWorkspaceScanner,
) -> Vec<FileMentionEntry> {
    let files = scanner.workspace_files(workspace_root).unwrap_or_else(|| {
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

pub(super) fn ripgrep_workspace_files(workspace_root: &Path) -> Option<Vec<String>> {
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

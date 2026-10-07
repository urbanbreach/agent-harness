//! Claude Code transcript reading: the SDK's `getSessionMessages(sessionId, {dir})` main-chain
//! projection, used to verify a restored binding before it is resumed.
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
mod queued;
pub use queued::*;
mod fragments;
pub use fragments::*;

/// A transcript message as `getSessionMessages` returns it; `parent_tool_use_id` is always
/// null for the main transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionMessage {
    pub kind: String,
    pub uuid: String,
    pub session_id: String,
}

const MAX_SANITIZED_LENGTH: usize = 200;
const PRECOMPACT_SKIP_BYTES: u64 = 5 * 1024 * 1024;

fn js_hash(value: &str) -> i32 {
    value.encode_utf16().fold(0_i32, |hash, unit| {
        hash.wrapping_shl(5)
            .wrapping_sub(hash)
            .wrapping_add(i32::from(unit))
    })
}

fn base36(mut value: u64) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(digits[usize::try_from(value % 36).unwrap_or(0)]);
        value /= 36;
        if value == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// The project directory name Claude Code derives from a working directory.
pub fn sanitize_project_path(path: &str) -> String {
    let sanitized: String = path
        .encode_utf16()
        .map(|unit| match char::from_u32(u32::from(unit)) {
            Some(c) if c.is_ascii_alphanumeric() => c,
            _ => '-',
        })
        .collect();
    if sanitized.len() <= MAX_SANITIZED_LENGTH {
        return sanitized;
    }
    format!(
        "{}-{}",
        &sanitized[..MAX_SANITIZED_LENGTH],
        base36(u64::from(js_hash(path).unsigned_abs()))
    )
}

pub fn claude_config_dir(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    env("CLAUDE_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|home| Path::new(&home).join(".claude")))
}

fn non_empty_file(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .filter(|m| m.is_file() && m.len() > 0)
        .map(|m| m.len())
}

fn locate(session_id: &str, dir: Option<&Path>, projects: &Path) -> Option<(PathBuf, u64)> {
    let file = format!("{session_id}.jsonl");
    if let Some(dir) = dir {
        let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let sanitized = sanitize_project_path(&canonical.to_string_lossy());
        let candidate = projects.join(&sanitized).join(&file);
        if let Some(size) = non_empty_file(&candidate) {
            return Some((candidate, size));
        }
        if sanitized.len() <= MAX_SANITIZED_LENGTH {
            return None;
        }
        // Hashed long names may differ by runtime; any directory with the prefix is a candidate.
        let prefix = format!("{}-", &sanitized[..MAX_SANITIZED_LENGTH]);
        return std::fs::read_dir(projects)
            .ok()?
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .find_map(|entry| {
                let candidate = entry.path().join(&file);
                non_empty_file(&candidate).map(|size| (candidate, size))
            });
    }
    std::fs::read_dir(projects)
        .ok()?
        .flatten()
        .find_map(|entry| {
            let candidate = entry.path().join(&file);
            non_empty_file(&candidate).map(|size| (candidate, size))
        })
}

fn is_session_uuid(value: &str) -> bool {
    value.len() == 36
        && value.char_indices().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn compact_boundary(entry: &Value) -> bool {
    entry["type"] == "system" && entry["subtype"] == "compact_boundary"
}

fn parse_entries(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .filter(|entry| {
            matches!(
                entry["type"].as_str(),
                Some("user" | "assistant" | "progress" | "system" | "attachment")
            ) && entry["uuid"].is_string()
        })
        .collect()
}

fn read_entries(path: &Path, size: u64) -> Option<Vec<Value>> {
    let text = std::fs::read_to_string(path).ok()?;
    if size <= PRECOMPACT_SKIP_BYTES {
        return Some(parse_entries(&text));
    }
    // Large transcripts are read from their last plain compact boundary on.
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().rposition(|line| {
        line.contains("\"compact_boundary\"")
            && serde_json::from_str::<Value>(line).is_ok_and(|entry| {
                compact_boundary(&entry)
                    && entry["compactMetadata"]["preservedSegment"].is_null()
                    && entry["compactMetadata"]["preservedMessages"].is_null()
            })
    });
    Some(parse_entries(&lines[start.unwrap_or(0)..].join("\n")))
}

fn uuid(entry: &Value) -> &str {
    entry["uuid"].as_str().unwrap_or("")
}
fn parent(entry: &Value) -> Option<&str> {
    entry["parentUuid"].as_str().filter(|p| !p.is_empty())
}
fn message_id(entry: &Value) -> Option<&str> {
    (entry["type"] == "assistant")
        .then(|| entry["message"]["id"].as_str())
        .flatten()
}
fn block_ids<'a>(entry: &'a Value, kind: &str, key: &str) -> Vec<&'a str> {
    entry["message"]["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b["type"] == kind)
                .filter_map(|b| b[key].as_str())
                .collect()
        })
        .unwrap_or_default()
}
fn is_tool_result(entry: &Value) -> bool {
    entry["type"] == "user"
        && parent(entry).is_some()
        && entry["message"]["content"]
            .as_array()
            .is_some_and(|blocks| blocks.iter().any(|b| b["type"] == "tool_result"))
}

fn relink(
    by_uuid: &mut HashMap<String, Value>,
    order: &[String],
    head: &str,
    anchor: &str,
    tail: &str,
) {
    for id in order {
        if id != head
            && let Some(entry) = by_uuid.get_mut(id)
            && parent(entry) == Some(anchor)
        {
            entry["parentUuid"] = Value::String(tail.into());
        }
    }
}

/// Re-parents preserved compaction segments onto their anchors.
fn relink_compaction(by_uuid: &mut HashMap<String, Value>, order: &[String]) {
    let boundaries: Vec<Value> = order
        .iter()
        .filter_map(|id| by_uuid.get(id))
        .filter(|e| compact_boundary(e))
        .cloned()
        .collect();
    for boundary in boundaries {
        let metadata = &boundary["compactMetadata"];
        if let Some(preserved) = metadata["preservedMessages"].as_object() {
            let uuids: Vec<String> = preserved["uuids"]
                .as_array()
                .map(|u| {
                    u.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            if uuids.is_empty() || uuids.iter().any(|u| !by_uuid.contains_key(u)) {
                continue;
            }
            let anchor = preserved["anchorUuid"].as_str().unwrap_or("").to_owned();
            let mut previous = anchor.clone();
            for id in &uuids {
                if let Some(entry) = by_uuid.get_mut(id) {
                    entry["parentUuid"] = Value::String(previous.clone());
                }
                previous.clone_from(id);
            }
            relink(by_uuid, order, &uuids[0], &anchor, &uuids[uuids.len() - 1]);
        } else if let Some(segment) = metadata["preservedSegment"].as_object() {
            let text = |key: &str| segment[key].as_str().unwrap_or("").to_owned();
            let (head, anchor, tail) = (text("headUuid"), text("anchorUuid"), text("tailUuid"));
            if let Some(entry) = by_uuid.get_mut(&head) {
                entry["parentUuid"] = Value::String(anchor.clone());
            }
            relink(by_uuid, order, &head, &anchor, &tail);
        }
    }
}

fn is_main_lane(e: &Value) -> bool {
    e["isSidechain"] != true
        && e["teamName"].is_null()
        && e["type"] != "progress"
        && !(e["type"] == "attachment" && e["attachment"]["type"] == "fork_briefing")
}

/// The newest main-lane leaf whose walk reaches a user or assistant message.
fn select_leaf(
    by_uuid: &HashMap<String, Value>,
    order: &[String],
    entries: &[Value],
) -> Option<String> {
    let position: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (uuid(e), i))
        .collect();
    let parents_of_main: HashSet<&str> = order
        .iter()
        .filter_map(|id| by_uuid.get(id))
        .filter(|e| is_main_lane(e))
        .filter_map(parent)
        .collect();
    let mut leaves: Vec<&Value> = order
        .iter()
        .filter_map(|id| by_uuid.get(id))
        .filter(|e| is_main_lane(e) && !parents_of_main.contains(uuid(e)))
        .collect();
    leaves.sort_by_key(|e| {
        std::cmp::Reverse(
            position
                .get(uuid(e))
                .map_or(-1, |p| i64::try_from(*p).unwrap_or(i64::MAX)),
        )
    });
    let mut visited: HashSet<String> = HashSet::new();
    for leaf in leaves {
        let mut path = Vec::new();
        let mut current = Some(leaf);
        while let Some(entry) = current {
            let id = uuid(entry);
            if visited.contains(id) || path.iter().any(|p: &String| p == id) {
                break;
            }
            if matches!(entry["type"].as_str(), Some("user" | "assistant")) {
                return Some(id.to_owned());
            }
            path.push(id.to_owned());
            current = parent(entry).and_then(|p| by_uuid.get(p));
        }
        visited.extend(path);
    }
    None
}

/// Relinks preserved compaction segments, picks the newest main-chain leaf and walks it.
fn main_chain(entries: &[Value]) -> (Vec<Value>, Option<String>) {
    let mut by_uuid: HashMap<String, Value> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for entry in entries {
        if !by_uuid.contains_key(uuid(entry)) {
            order.push(uuid(entry).to_owned());
        }
        by_uuid.insert(uuid(entry).to_owned(), entry.clone());
    }
    relink_compaction(&mut by_uuid, &order);
    let Some(found) = select_leaf(&by_uuid, &order, entries) else {
        return (Vec::new(), None);
    };
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut current = by_uuid.get(&found);
    while let Some(entry) = current {
        if !seen.insert(uuid(entry).to_owned()) {
            break;
        }
        chain.push(entry.clone());
        current = parent(entry).and_then(|p| by_uuid.get(p));
    }
    chain.reverse();
    (
        merge_fragments(&by_uuid, &order, chain, &mut seen),
        Some(found),
    )
}

/// `getSessionMessages(sessionId, {dir})`: user/assistant main-chain messages.
pub fn get_session_messages(
    session_id: &str,
    dir: Option<&Path>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<SessionMessage>, String> {
    if !is_session_uuid(session_id) {
        return Ok(Vec::new());
    }
    let projects = claude_config_dir(env)
        .ok_or("no Claude config directory")?
        .join("projects");
    let Some((path, size)) = locate(session_id, dir, &projects) else {
        return Ok(Vec::new());
    };
    let entries = read_entries(&path, size).ok_or("unreadable transcript")?;
    let (mut chain, leaf) = main_chain(&entries);
    let trailing = leaf
        .map(|leaf| trailing_queued_commands(&entries, &chain, &leaf))
        .unwrap_or_default();
    let trailing_ids: HashSet<String> = trailing.iter().map(|e| uuid(e).to_owned()).collect();
    chain.extend(trailing);
    Ok(project_queued_commands(chain, &trailing_ids)
        .into_iter()
        .filter(|e| matches!(e["type"].as_str(), Some("user" | "assistant")))
        .filter(|e| {
            (e["isMeta"] != true || surfaced_origin(&e["origin"]))
                && e["isSidechain"] != true
                && e["teamName"].is_null()
        })
        .map(|e| SessionMessage {
            kind: e["type"].as_str().unwrap_or("").to_owned(),
            uuid: uuid(&e).to_owned(),
            session_id: e["sessionId"].as_str().unwrap_or("").to_owned(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitized_paths_match_claude_code() {
        assert_eq!(
            sanitize_project_path("/home/u/Projects/agent-harness"),
            "-home-u-Projects-agent-harness"
        );
        let long = format!("/{}", "a".repeat(250));
        let sanitized = sanitize_project_path(&long);
        assert!(sanitized.starts_with(&format!("-{}", "a".repeat(199))));
        assert_eq!(
            sanitized.len(),
            201 + base36(u64::from(js_hash(&long).unsigned_abs())).len()
        );
    }
}

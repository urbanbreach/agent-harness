//! Queued-command projection.
use super::*;

const INTERRUPTION_PREFIXES: [&str; 8] = [
    "[Request interrupted by user]",
    "[Request interrupted by user for tool use]",
    "[Tool call did not complete: the turn was ended to deliver the message that follows. Nothing refused it; re-run it if still needed.]",
    "[Tool call interrupted: the session ended before this call's result was recorded, so its outcome is unknown. Check whether it took effect before relying on it or running it again.]",
    "[Tool call result not in this copy: this session was copied from another session before that session recorded this call's result. The call may have finished there, may still be running there, or may never have run. Check whether it took effect before relying on it or running it again.]",
    "The user doesn't want to take this action right now. STOP what you are doing and wait for the user to tell you how to proceed.",
    "[Tool call skipped: the turn was stopped before this call ran, by the check whose denial is on another call in this batch. Nothing refused this call and it had no effects; re-run it if still needed.]",
    "[Tool call skipped: the turn ended to deliver the message that follows before this call ran. Nothing refused it; re-run it if still needed.]",
];

/// A user entry that only reports an interrupted or skipped tool call.
fn is_interruption(entry: &Value) -> bool {
    if entry["type"] != "user" {
        return false;
    }
    let starts = |text: &str| INTERRUPTION_PREFIXES.iter().any(|p| text.starts_with(p));
    match &entry["message"]["content"] {
        Value::String(text) => starts(text),
        Value::Array(blocks) => {
            !blocks.is_empty()
                && blocks.iter().all(|b| {
                    let text = if b["type"] == "text" {
                        b["text"].as_str()
                    } else if b["type"] == "tool_result" && b["is_error"] == true {
                        b["content"].as_str()
                    } else {
                        None
                    };
                    text.is_some_and(starts)
                })
        }
        _ => false,
    }
}

fn is_queued_command(entry: &Value) -> bool {
    entry["type"] == "attachment" && entry["attachment"]["type"] == "queued_command"
}

/// Origins whose meta messages still surface (channels, observers, peers).
pub(super) fn surfaced_origin(origin: &Value) -> bool {
    matches!(
        origin["kind"].as_str(),
        Some("channel" | "observer" | "observer-activity" | "slack-ping" | "peer")
    )
}

/// Queued commands hanging off the leaf that no other branch leads to.
pub(super) fn trailing_queued_commands(
    entries: &[Value],
    chain: &[Value],
    leaf: &str,
) -> Vec<Value> {
    let mut children: HashMap<&str, Vec<&Value>> = HashMap::new();
    for entry in entries
        .iter()
        .filter(|e| !matches!(e["type"].as_str(), Some("user" | "assistant")))
    {
        if let Some(parent) = parent(entry) {
            children.entry(parent).or_default().push(entry);
        }
    }
    let mut seen: HashSet<String> = chain.iter().map(|e| uuid(e).to_owned()).collect();
    let by_uuid: HashMap<&str, &Value> = entries.iter().map(|e| (uuid(e), e)).collect();
    let mut ancestors = HashSet::new();
    for entry in entries {
        if !matches!(entry["type"].as_str(), Some("user" | "assistant"))
            || entry["isSidechain"] == true
            || !entry["teamName"].is_null()
            || seen.contains(uuid(entry))
        {
            continue;
        }
        let mut current = parent(entry).and_then(|p| by_uuid.get(p));
        while let Some(node) = current {
            if seen.contains(uuid(node)) || !ancestors.insert(uuid(node).to_owned()) {
                break;
            }
            current = parent(node).and_then(|p| by_uuid.get(p));
        }
    }
    let Some(leaf_entry) = by_uuid.get(leaf) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut stack = vec![*leaf_entry];
    while let Some(node) = stack.pop() {
        if uuid(node) != leaf {
            if !seen.insert(uuid(node).to_owned()) {
                continue;
            }
            if is_queued_command(node) && !ancestors.contains(uuid(node)) {
                found.push(node.clone());
            }
        }
        let mut next = children.get(uuid(node)).cloned().unwrap_or_default();
        next.sort_by(|a, b| {
            a["timestamp"]
                .as_str()
                .unwrap_or("")
                .cmp(b["timestamp"].as_str().unwrap_or(""))
        });
        stack.extend(next.into_iter().rev().filter(|c| !seen.contains(uuid(c))));
    }
    found
}

/// Queued commands a reply followed (or that trail the leaf) surface as user messages.
pub(super) fn project_queued_commands(list: Vec<Value>, trailing: &HashSet<String>) -> Vec<Value> {
    let mut reply = vec![false; list.len()];
    let mut state: Option<bool> = None;
    for (index, entry) in list.iter().enumerate().rev() {
        reply[index] = state == Some(true);
        if entry["type"] == "assistant" || is_tool_result(entry) || is_interruption(entry) {
            state = Some(true);
        } else if entry["type"] == "user"
            && entry["isMeta"] != true
            && entry["isCompactSummary"] != true
        {
            state = Some(false);
        }
    }
    let mut known: HashSet<String> = list.iter().map(|e| uuid(e).to_owned()).collect();
    list.into_iter()
        .enumerate()
        .map(|(index, entry)| {
            if !(reply[index] || trailing.contains(uuid(&entry))) || !is_queued_command(&entry) {
                return entry;
            }
            let command = &entry["attachment"];
            let origin = command["origin"].clone();
            let prompt = &command["prompt"];
            if command["isMeta"] == true && !surfaced_origin(&origin)
                || !(prompt.is_string() || prompt.is_array())
                || command["forwardedIntent"]["lineage"].is_string()
            {
                return entry;
            }
            let id = command["source_uuid"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(uuid(&entry))
                .to_owned();
            if id != uuid(&entry) && known.contains(&id) {
                return entry;
            }
            known.insert(id.clone());
            let origin = if origin["kind"].is_string() {
                origin
            } else if command["commandMode"] == "task-notification" {
                serde_json::json!({"kind": "task-notification"})
            } else {
                Value::Null
            };
            serde_json::json!({
                "type": "user",
                "uuid": id,
                "parentUuid": entry["parentUuid"],
                "sessionId": entry["sessionId"],
                "timestamp": entry["timestamp"],
                "message": {"role": "user", "content": prompt},
                "isMeta": command["isMeta"] == true,
                "origin": origin,
                "isQueuedCommand": true,
                "isSidechain": entry["isSidechain"],
                "teamName": entry["teamName"],
            })
        })
        .collect()
}

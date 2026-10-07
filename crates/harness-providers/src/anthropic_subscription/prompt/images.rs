//! Cold-seed history image replay (senpi `cold-seed-images.ts`).
use super::*;

// ---- cold-seed image replay (#2490) ----

/// Most distinct historical images one cold-seed / flatten replay re-sends.
pub const MAX_REPLAYED_HISTORY_IMAGES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Origin {
    User,
    Tool { name: String, id: String },
}
fn subject(origin: &Origin) -> String {
    match origin {
        Origin::User => "[image attached by the user".into(),
        Origin::Tool { name, .. } => format!("[image returned by the {name} tool"),
    }
}
fn source(origin: &Origin) -> String {
    match origin {
        Origin::User => "attached by the user".into(),
        Origin::Tool { name, id } => format!("returned by the {name} tool, id={id}"),
    }
}

fn decode_image(data: &str) -> Option<(String, String)> {
    use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
    // Node's base64 decoder is lenient about trailing bits and padding.
    const LENIENT: GeneralPurpose = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_allow_trailing_bits(true)
            .with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
    let compact: String = data.chars().filter(|c| !c.is_whitespace()).collect();
    let trimmed = compact.trim_end_matches('=');
    if trimmed.is_empty()
        || trimmed.len() % 4 == 1
        || !compact.bytes().enumerate().all(|(i, b)| {
            b.is_ascii_alphanumeric() || b"+/_-".contains(&b) || (b == b'=' && i >= trimmed.len())
        })
        || compact.len() - trimmed.len() > 2
    {
        return None;
    }
    let normalized: String = trimmed
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            c => c,
        })
        .collect();
    let bytes = LENIENT.decode(normalized.as_bytes()).ok()?;
    let digest = hex(&Sha256::digest(&bytes));
    Some((
        digest,
        base64::engine::general_purpose::STANDARD.encode(bytes),
    ))
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// Rewrites the image entries of a cold-seed history: tool images are labeled, identical
/// bytes are sent once, the replay is capped, undecodable data becomes a note.
pub(super) fn replay_history_images(
    history: &[LaneMessage],
    sdk_tool_name: &dyn Fn(&str) -> String,
) -> HashMap<usize, Vec<Value>> {
    struct Occurrence {
        message: usize,
        entry: usize,
        origin: Origin,
        value: Value,
        decoded: Option<(String, String)>,
    }
    let mut occurrences = Vec::new();
    let mut contents = BTreeMap::new();
    for (index, message) in history.iter().enumerate() {
        let (origin, content) = match message {
            LaneMessage::User {
                content: Content::Blocks(blocks),
            } => (Origin::User, blocks),
            LaneMessage::ToolResult {
                tool_name,
                tool_call_id,
                content: Content::Blocks(blocks),
                ..
            } => (
                Origin::Tool {
                    name: sdk_tool_name(tool_name),
                    id: tool_call_id.clone(),
                },
                blocks,
            ),
            _ => continue,
        };
        contents.insert(index, content.clone());
        for (entry_index, entry) in content.iter().enumerate() {
            let (Some(data), Some(mime)) = (entry["data"].as_str(), entry["mimeType"].as_str())
            else {
                continue;
            };
            if entry["type"] != "image" || !is_sdk_image_media_type(mime) {
                continue;
            }
            occurrences.push(Occurrence {
                message: index,
                entry: entry_index,
                origin: origin.clone(),
                value: entry.clone(),
                decoded: decode_image(data),
            });
        }
    }
    if occurrences.is_empty() {
        return HashMap::new();
    }
    let mut ranked: Vec<(String, bool, usize)> = Vec::new();
    for (order, occurrence) in occurrences.iter().enumerate() {
        let Some((hash, _)) = &occurrence.decoded else {
            continue;
        };
        let user = occurrence.origin == Origin::User;
        match ranked.iter_mut().find(|(h, _, _)| h == hash) {
            Some(entry) => {
                entry.1 |= user;
                entry.2 = order;
            }
            None => ranked.push((hash.clone(), user, order)),
        }
    }
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
    let replayed: HashSet<String> = ranked
        .into_iter()
        .take(MAX_REPLAYED_HISTORY_IMAGES)
        .map(|(hash, _, _)| hash)
        .collect();
    let mut first_shown: HashMap<String, Origin> = HashMap::new();
    let mut replacements: BTreeMap<usize, BTreeMap<usize, Vec<Value>>> = BTreeMap::new();
    for occurrence in &occurrences {
        let replacement = match &occurrence.decoded {
            None => vec![text_block(format!(
                "{}: omitted because its image data is missing or unreadable]",
                subject(&occurrence.origin)
            ))],
            Some((hash, _)) if !replayed.contains(hash) => vec![text_block(format!(
                "{}: omitted from this replay, which re-sends at most {MAX_REPLAYED_HISTORY_IMAGES} earlier images; re-read the source if you need it]",
                subject(&occurrence.origin)
            ))],
            Some((hash, canonical)) => match first_shown.get(hash) {
                Some(first) => vec![text_block(format!(
                    "{}: identical to an image already shown above ({}); not attached again]",
                    subject(&occurrence.origin),
                    source(first)
                ))],
                None => {
                    first_shown.insert(hash.clone(), occurrence.origin.clone());
                    let mut value = occurrence.value.clone();
                    value["data"] = json!(canonical);
                    match &occurrence.origin {
                        Origin::Tool { name, .. } => vec![
                            text_block(format!(
                                "[image returned by the {name} tool (tool output, not a user attachment)]"
                            )),
                            value,
                        ],
                        Origin::User => vec![value],
                    }
                }
            },
        };
        replacements
            .entry(occurrence.message)
            .or_default()
            .insert(occurrence.entry, replacement);
    }
    replacements
        .into_iter()
        .map(|(index, per_message)| {
            let content = contents.get(&index).cloned().unwrap_or_default();
            let rewritten = content
                .into_iter()
                .enumerate()
                .flat_map(|(entry, value)| {
                    per_message
                        .get(&entry)
                        .cloned()
                        .unwrap_or_else(|| vec![value])
                })
                .collect();
            (index, rewritten)
        })
        .collect()
}

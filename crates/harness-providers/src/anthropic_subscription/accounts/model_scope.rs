//! Model-scoped rate-limit blocks (senpi `credential-pool/model-scope.ts`).
use super::*;

// ---- model scope (senpi#2555) ----

static FAMILY_KEY: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(?:opus|sonnet|haiku|fable|mythos)$").ok());
static MODEL_FAMILY_LIMIT: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(opus|sonnet|haiku|fable|mythos)(?:\s+\d+(?:\.\d+)?)?(?:\s+(?:weekly|daily|monthly|session|\d+-hour))?\s+limit\b",
    )
    .ok()
});

pub fn rate_limit_model_family(text: &str) -> Option<String> {
    MODEL_FAMILY_LIMIT
        .as_ref()?
        .captures(text)?
        .get(1)
        .map(|m| m.as_str().to_lowercase())
}

fn in_family(model_id: &str, family: &str) -> bool {
    Regex::new(&format!(
        "(?i)(?:^|[^a-z]){}(?:[^a-z]|$)",
        regex::escape(family)
    ))
    .is_ok_and(|re| re.is_match(model_id))
}

/// The family when the requested model belongs to it, otherwise the exact model id.
pub fn model_block_key(family: &str, model_id: &str) -> String {
    if in_family(model_id, family) {
        family.into()
    } else {
        model_id.into()
    }
}

fn applies(key: &str, model_id: &str) -> bool {
    key == model_id
        || FAMILY_KEY.as_ref().is_some_and(|re| re.is_match(key)) && in_family(model_id, key)
}

pub fn active_model_block_until(
    blocks: Option<&ModelBlocks>,
    model_id: Option<&str>,
    now: i64,
) -> Option<i64> {
    let (blocks, model_id) = (blocks?, model_id?);
    blocks
        .iter()
        .filter(|(key, block)| block.blocked_until > now && applies(key, model_id))
        .map(|(_, block)| block.blocked_until)
        .max()
}

/// Drops expired entries and, when `served_model_id` is given, the entries that bind it.
pub fn prune_model_blocks(
    blocks: Option<&ModelBlocks>,
    now: i64,
    served_model_id: Option<&str>,
) -> Option<ModelBlocks> {
    let kept: ModelBlocks = blocks?
        .iter()
        .filter(|(key, block)| {
            block.blocked_until > now && served_model_id.is_none_or(|served| !applies(key, served))
        })
        .map(|(key, block)| (key.clone(), *block))
        .collect();
    (!kept.is_empty()).then_some(kept)
}

pub fn with_model_block(
    blocks: Option<&ModelBlocks>,
    key: &str,
    blocked_until: i64,
    now: i64,
) -> ModelBlocks {
    let block = ModelBlocks::from([(key.to_owned(), ModelBlock { blocked_until })]);
    merge_model_blocks(
        prune_model_blocks(blocks, now, None).as_ref(),
        Some(&block),
        now,
    )
    .unwrap_or_default()
}

/// Union of two block maps, the later expiry winning per key.
pub fn merge_model_blocks(
    left: Option<&ModelBlocks>,
    right: Option<&ModelBlocks>,
    now: i64,
) -> Option<ModelBlocks> {
    let mut merged = prune_model_blocks(left, now, None).unwrap_or_default();
    for (key, block) in prune_model_blocks(right, now, None).unwrap_or_default() {
        let until = merged
            .get(&key)
            .map_or(0, |b| b.blocked_until)
            .max(block.blocked_until);
        merged.insert(
            key,
            ModelBlock {
                blocked_until: until,
            },
        );
    }
    (!merged.is_empty()).then_some(merged)
}

pub fn describe_model_blocks(blocks: Option<&ModelBlocks>, now: i64) -> Vec<String> {
    prune_model_blocks(blocks, now, None)
        .unwrap_or_default()
        .iter()
        .map(|(key, block)| format!("blocked for {key} until {}", iso(block.blocked_until)))
        .collect()
}

/// `new Date(ms).toISOString()`.
pub fn iso(ms: i64) -> String {
    let time = std::time::UNIX_EPOCH
        + std::time::Duration::from_millis(u64::try_from(ms.max(0)).unwrap_or(0));
    humantime::format_rfc3339_millis(time).to_string()
}

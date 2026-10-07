//! Split assistant fragments.
use super::*;

type Owners<'a> = HashMap<&'a str, Option<&'a Value>>;

/// Fragments by assistant message id, the unique owner of each tool use, and tool results.
fn index_fragments<'a>(
    all: &[&'a Value],
) -> (HashMap<&'a str, Vec<&'a Value>>, Owners<'a>, Vec<&'a Value>) {
    let mut fragments: HashMap<&str, Vec<&Value>> = HashMap::new();
    let mut owners: Owners<'_> = HashMap::new();
    let mut results = Vec::new();
    for entry in all {
        let Some(id) = message_id(entry) else {
            if is_tool_result(entry) {
                results.push(*entry);
            }
            continue;
        };
        fragments.entry(id).or_default().push(entry);
        for tool in block_ids(entry, "tool_use", "id") {
            let owner = match owners.get(tool) {
                None => Some(*entry),
                Some(Some(existing)) if message_id(existing) == Some(id) => Some(*entry),
                _ => None,
            };
            owners.insert(tool, owner);
        }
    }
    (fragments, owners, results)
}

fn same_lane(a: &Value, b: &Value) -> bool {
    a["isSidechain"].as_bool().unwrap_or(false) == b["isSidechain"].as_bool().unwrap_or(false)
        && a["agentId"] == b["agentId"]
}

/// Tool results keyed by every assistant fragment they answer.
fn link_results<'a>(
    by_uuid: &'a HashMap<String, Value>,
    owners: &Owners<'a>,
    results: &[&'a Value],
) -> HashMap<String, Vec<&'a Value>> {
    let mut children: HashMap<String, Vec<&Value>> = HashMap::new();
    let mut linked = HashSet::new();
    for result in results {
        let mut keys: Vec<&str> = parent(result).into_iter().collect();
        if let Some(source) = result["sourceToolAssistantUUID"].as_str()
            && Some(source) != parent(result)
            && let Some(owner) = by_uuid.get(source)
            && same_lane(result, owner)
        {
            keys.push(uuid(owner));
        }
        keys.extend(
            block_ids(result, "tool_result", "tool_use_id")
                .into_iter()
                .filter_map(|tool| owners.get(tool).copied().flatten())
                .filter(|owner| same_lane(result, owner))
                .map(uuid),
        );
        for key in keys {
            if linked.insert(format!("{key}\n{}", uuid(result))) {
                children.entry(key.to_owned()).or_default().push(*result);
            }
        }
    }
    children
}

struct FragmentIndex<'a> {
    fragments: HashMap<&'a str, Vec<&'a Value>>,
    owners: Owners<'a>,
    children: HashMap<String, Vec<&'a Value>>,
    answered: HashSet<&'a str>,
    rank: HashMap<&'a str, usize>,
}

/// The off-chain fragments of one assistant message and the results that answer them.
fn fragment_tail<'a>(
    index: &FragmentIndex<'a>,
    id: &str,
    primary: &'a Value,
    on_chain: &HashSet<String>,
) -> Vec<&'a Value> {
    let siblings = index
        .fragments
        .get(id)
        .cloned()
        .unwrap_or_else(|| vec![primary]);
    let sibling_ids: HashSet<&str> = siblings.iter().map(|s| uuid(s)).collect();
    let mut missing: Vec<&Value> = siblings
        .iter()
        .copied()
        .filter(|s| !on_chain.contains(uuid(s)))
        .collect();
    let (mut direct, mut indirect) = (Vec::new(), Vec::new());
    let mut seen = HashSet::new();
    let unseen = siblings
        .iter()
        .flat_map(|sibling| index.children.get(uuid(sibling)).into_iter().flatten())
        .filter(|child| !on_chain.contains(uuid(child)));
    for child in unseen {
        if !seen.insert(uuid(child)) {
            continue;
        }
        if parent(child).is_some_and(|p| sibling_ids.contains(p)) {
            direct.push(*child);
        } else {
            indirect.push(*child);
        }
    }
    let mut known = index.answered.clone();
    known.extend(
        direct
            .iter()
            .flat_map(|child| block_ids(child, "tool_result", "tool_use_id")),
    );
    indirect.sort_by_key(|c| index.rank.get(uuid(c)).copied().unwrap_or(usize::MAX));
    for child in indirect {
        let tools = block_ids(child, "tool_result", "tool_use_id");
        let relevant = tools.iter().any(|t| {
            !known.contains(t)
                && index
                    .owners
                    .get(t)
                    .copied()
                    .flatten()
                    .is_some_and(|owner| sibling_ids.contains(uuid(owner)))
        });
        if relevant {
            known.extend(tools);
            direct.push(child);
        }
    }
    let by_time = |a: &&Value, b: &&Value| {
        a["timestamp"]
            .as_str()
            .unwrap_or("")
            .cmp(b["timestamp"].as_str().unwrap_or(""))
    };
    missing.sort_by(by_time);
    direct.sort_by(by_time);
    missing.into_iter().chain(direct).collect()
}

/// Re-inserts sibling fragments of split assistant messages (and their tool results) after
/// the primary fragment on the chain.
pub(super) fn merge_fragments(
    by_uuid: &HashMap<String, Value>,
    order: &[String],
    chain: Vec<Value>,
    on_chain: &mut HashSet<String>,
) -> Vec<Value> {
    let assistants: Vec<&Value> = chain.iter().filter(|e| e["type"] == "assistant").collect();
    if assistants.is_empty() {
        return chain;
    }
    let primary: HashMap<&str, &Value> = assistants
        .iter()
        .filter_map(|e| message_id(e).map(|id| (id, *e)))
        .collect();
    let all: Vec<&Value> = order.iter().filter_map(|id| by_uuid.get(id)).collect();
    let (fragments, owners, results) = index_fragments(&all);
    let index = FragmentIndex {
        children: link_results(by_uuid, &owners, &results),
        fragments,
        owners,
        answered: chain
            .iter()
            .flat_map(|e| block_ids(e, "tool_result", "tool_use_id"))
            .collect(),
        rank: order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i))
            .collect(),
    };
    let mut done = HashSet::new();
    let mut inserts: HashMap<String, Vec<Value>> = HashMap::new();
    for entry in &assistants {
        let Some(id) = message_id(entry).filter(|id| done.insert(*id)) else {
            continue;
        };
        let primary_entry = primary.get(id).copied().unwrap_or(entry);
        let added: Vec<Value> = fragment_tail(&index, id, primary_entry, on_chain)
            .into_iter()
            .cloned()
            .collect();
        if added.is_empty() {
            continue;
        }
        on_chain.extend(added.iter().map(|value| uuid(value).to_owned()));
        inserts.insert(uuid(primary_entry).to_owned(), added);
    }
    if inserts.is_empty() {
        return chain;
    }
    let mut out = Vec::new();
    for entry in chain {
        let extra = inserts.remove(uuid(&entry));
        out.push(entry);
        out.extend(extra.into_iter().flatten());
    }
    out
}

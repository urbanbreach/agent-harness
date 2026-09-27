use super::*;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
#[derive(Default)]
pub(super) struct State {
    pub tracker: EditAttributionTracker,
    pub snapshots: BTreeMap<String, Vec<u8>>,
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Kind {
    AgentTool,
    External,
    Drift,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    v: u32,
    seq: u64,
    path: String,
    source: EditSource,
    kind: Kind,
    content_sha256: String,
    mtime_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_snapshot_hex: Option<String>,
    ts_unix_ms: u64,
}
pub(super) fn validate_key(key: &str) -> Result<(), ()> {
    if key.is_empty()
        || key.len() > 4096
        || key.chars().any(char::is_control)
        || !Path::new(key)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        || crate::redact::DefaultRedactor::default().secret_finding_count(key) > 0
    {
        return Err(());
    }
    Ok(())
}
pub(super) fn validate_content(path: &Path, bytes: &[u8]) -> Result<(), EditAttributionError> {
    if bytes.len() > 8 * 1024 * 1024
        || crate::redact::DefaultRedactor::default()
            .secret_finding_count(&String::from_utf8_lossy(bytes))
            > 0
    {
        return Err(invalid(
            path,
            "snapshot exceeds 8 MiB or contains a credential",
        ));
    }
    Ok(())
}
pub(super) fn load(path: &Path) -> Result<State, EditAttributionError> {
    let Some(bytes) = crate::store::read_private_bytes(path, MAX_BYTES).map_err(|source| {
        EditAttributionError::Read {
            path: path.display().to_string(),
            source,
        }
    })?
    else {
        return Ok(State::default());
    };
    let mut state = State::default();
    for (index, line) in bytes
        .split(|b| *b == b'\n')
        .filter(|s| !s.iter().all(u8::is_ascii_whitespace))
        .enumerate()
    {
        if index >= 65_536 {
            return Err(invalid(path, "too many attribution records"));
        }
        let record: Record =
            serde_json::from_slice(line).map_err(|_| invalid(path, "invalid JSON record"))?;
        if record.v != 1
            || record.seq != index as u64 + 1
            || validate_key(&record.path).is_err()
            || record.content_sha256.len() != 64
            || !record.content_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(invalid(
                path,
                "invalid record version, sequence, path, or digest",
            ));
        }
        if let Some(hex) = record.agent_snapshot_hex {
            let body = hex::decode(hex).map_err(|_| invalid(path, "invalid snapshot encoding"))?;
            validate_content(path, &body)?;
            if record.source != EditSource::AgentTool || sha256_hex(&body) != record.content_sha256
            {
                return Err(invalid(path, "snapshot digest or source mismatch"));
            }
            state.snapshots.insert(record.path.clone(), body);
        }
        match record.kind {
            Kind::AgentTool => {
                if record.source != EditSource::AgentTool
                    || state
                        .snapshots
                        .get(&record.path)
                        .is_none_or(|body| sha256_hex(body) != record.content_sha256)
                {
                    return Err(invalid(path, "agent record lacks its snapshot"));
                }
                state.tracker.drifted.remove(&record.path);
            }
            Kind::External => {
                if record.source != EditSource::External {
                    return Err(invalid(path, "external record has an agent source"));
                }
                state.tracker.drifted.remove(&record.path);
            }
            Kind::Drift => {
                if record.source != EditSource::External
                    || !state.snapshots.contains_key(&record.path)
                {
                    return Err(invalid(path, "drift lacks an agent baseline"));
                }
                state.tracker.drifted.insert(record.path.clone());
            }
        }
        state.tracker.entries.insert(
            record.path.clone(),
            AttributedEdit {
                path: record.path,
                source: record.source,
                content_sha256: record.content_sha256,
                mtime_unix_ms: record.mtime_unix_ms,
            },
        );
        if state.tracker.len() > 1024 {
            return Err(invalid(path, "attribution exceeds 1024 paths"));
        }
    }
    Ok(state)
}
pub(super) fn save(path: &Path, state: &State) -> Result<(), EditAttributionError> {
    if state.tracker.len() > 1024 {
        return Err(invalid(path, "attribution exceeds 1024 paths"));
    }
    let mut bytes = Vec::new();
    let mut seq = 0;
    // ponytail: keep current ownership and one baseline per path; run events retain edit history.
    for entry in state.tracker.list() {
        let baseline = state.snapshots.get(&entry.path);
        if let Some(body) = baseline {
            seq += 1;
            let record = Record {
                v: 1,
                seq,
                path: entry.path.clone(),
                source: EditSource::AgentTool,
                kind: Kind::AgentTool,
                content_sha256: sha256_hex(body),
                mtime_unix_ms: entry.mtime_unix_ms,
                agent_snapshot_hex: Some(hex::encode(body)),
                ts_unix_ms: 0,
            };
            serde_json::to_writer(&mut bytes, &record)
                .map_err(|_| invalid(path, "cannot encode snapshot"))?;
            bytes.push(b'\n');
        }
        if entry.source == EditSource::External || baseline.is_none() {
            seq += 1;
            let record = Record {
                v: 1,
                seq,
                path: entry.path.clone(),
                source: entry.source,
                kind: if state.tracker.is_drifted(&entry.path) {
                    Kind::Drift
                } else {
                    Kind::External
                },
                content_sha256: entry.content_sha256.clone(),
                mtime_unix_ms: entry.mtime_unix_ms,
                agent_snapshot_hex: None,
                ts_unix_ms: 0,
            };
            serde_json::to_writer(&mut bytes, &record)
                .map_err(|_| invalid(path, "cannot encode attribution"))?;
            bytes.push(b'\n');
        }
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid(path, "attribution journal exceeds 64 MiB"));
        }
    }
    crate::store::write_private_atomic(path, &bytes).map_err(|source| EditAttributionError::Write {
        path: path.display().to_string(),
        source,
    })
}
fn invalid(path: &Path, detail: &str) -> EditAttributionError {
    EditAttributionError::Parse {
        path: path.display().to_string(),
        detail: detail.into(),
    }
}

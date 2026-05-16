use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::digest::digest12_json;
use crate::redact::Redactor;
use crate::tool::{ArtifactRef, ArtifactStore, ArtifactStoreError};

pub const CONTEXT_SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const CONTEXT_SNAPSHOT_ARTIFACT_KIND: &str = "context_snapshot";
pub const CONTEXT_SNAPSHOT_ARTIFACT_DIR: &str = "context_snapshots";
pub const CONTEXT_SNAPSHOT_EVIDENCE_CATEGORY: &str = "evidence.context_snapshot";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContextSnapshot {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub slug: String,
    pub created_at: String,
    pub source_command: String,
    pub task_statement: String,
    pub desired_outcome: String,
    pub probable_intent: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub known_facts: Vec<ContextSnapshotFact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub non_goals: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decision_boundaries: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknowns: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub likely_touchpoints: Vec<String>,
    pub ambiguity: ContextSnapshotAmbiguity,
    pub handoff_ready: bool,
    #[serde(default, skip_serializing_if = "ContextSnapshotSafetyReport::is_empty")]
    pub safety: ContextSnapshotSafetyReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContextSnapshotFact {
    pub source: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContextSnapshotAmbiguity {
    pub score: f32,
    pub threshold: f32,
}

impl Default for ContextSnapshotAmbiguity {
    fn default() -> Self {
        Self {
            score: 0.0,
            threshold: 0.2,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContextSnapshotSafetyReport {
    #[serde(default)]
    pub redacted: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capped_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub original_lengths: BTreeMap<String, usize>,
}

impl ContextSnapshotSafetyReport {
    pub fn is_empty(&self) -> bool {
        !self.redacted && self.capped_fields.is_empty() && self.original_lengths.is_empty()
    }

    fn record_cap(&mut self, field: &str, original_chars: usize) {
        if !self.capped_fields.iter().any(|existing| existing == field) {
            self.capped_fields.push(field.to_string());
        }
        self.original_lengths
            .entry(field.to_string())
            .or_insert(original_chars);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContextSnapshotInput {
    pub source_command: String,
    pub task_statement: String,
    #[serde(default)]
    pub desired_outcome: String,
    #[serde(default)]
    pub probable_intent: String,
    #[serde(default)]
    pub known_facts: Vec<ContextSnapshotFact>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub non_goals: Vec<String>,
    #[serde(default)]
    pub decision_boundaries: Vec<String>,
    #[serde(default)]
    pub unknowns: Vec<String>,
    #[serde(default)]
    pub likely_touchpoints: Vec<String>,
    #[serde(default)]
    pub ambiguity: ContextSnapshotAmbiguity,
    #[serde(default)]
    pub handoff_ready: bool,
}

impl ContextSnapshotInput {
    pub fn new(
        source_command: impl Into<String>,
        task_statement: impl Into<String>,
        desired_outcome: impl Into<String>,
    ) -> Self {
        Self {
            source_command: source_command.into(),
            task_statement: task_statement.into(),
            desired_outcome: desired_outcome.into(),
            probable_intent: String::new(),
            known_facts: Vec::new(),
            constraints: Vec::new(),
            non_goals: Vec::new(),
            decision_boundaries: Vec::new(),
            unknowns: Vec::new(),
            likely_touchpoints: Vec::new(),
            ambiguity: ContextSnapshotAmbiguity::default(),
            handoff_ready: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContextSnapshotOptions {
    pub max_text_chars: usize,
    pub max_list_items: usize,
}

impl Default for ContextSnapshotOptions {
    fn default() -> Self {
        Self {
            max_text_chars: 4_000,
            max_list_items: 64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContextSnapshotWriteResult {
    pub snapshot_id: String,
    pub slug: String,
    pub artifact_path: String,
    pub artifact_digest: String,
    pub artifact_bytes: u64,
    pub ambiguity_score: f32,
    pub capped: bool,
    pub redacted: bool,
}

impl ContextSnapshotWriteResult {
    pub fn workflow_evidence_metadata(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("snapshot_id".to_string(), self.snapshot_id.clone()),
            ("snapshot_slug".to_string(), self.slug.clone()),
            (
                "ambiguity_score".to_string(),
                format!("{:.3}", self.ambiguity_score),
            ),
            (
                "artifact_kind".to_string(),
                CONTEXT_SNAPSHOT_ARTIFACT_KIND.to_string(),
            ),
        ])
    }
}

pub fn build_context_snapshot<R: Redactor + ?Sized>(
    input: ContextSnapshotInput,
    options: ContextSnapshotOptions,
    redactor: &R,
    created_at: impl Into<String>,
) -> ContextSnapshot {
    let mut safety = ContextSnapshotSafetyReport::default();
    let source_command = sanitize_text(
        "source_command",
        input.source_command,
        options,
        redactor,
        &mut safety,
    );
    let task_statement = sanitize_text(
        "task_statement",
        input.task_statement,
        options,
        redactor,
        &mut safety,
    );
    let desired_outcome = sanitize_text(
        "desired_outcome",
        input.desired_outcome,
        options,
        redactor,
        &mut safety,
    );
    let probable_intent = sanitize_text(
        "probable_intent",
        input.probable_intent,
        options,
        redactor,
        &mut safety,
    );

    let mut snapshot = ContextSnapshot {
        schema_version: CONTEXT_SNAPSHOT_SCHEMA_VERSION,
        snapshot_id: String::new(),
        slug: slug_from_text(&task_statement),
        created_at: created_at.into(),
        source_command,
        task_statement,
        desired_outcome,
        probable_intent,
        known_facts: sanitize_facts(input.known_facts, options, redactor, &mut safety),
        constraints: sanitize_list(
            "constraints",
            input.constraints,
            options,
            redactor,
            &mut safety,
        ),
        non_goals: sanitize_list("non_goals", input.non_goals, options, redactor, &mut safety),
        decision_boundaries: sanitize_list(
            "decision_boundaries",
            input.decision_boundaries,
            options,
            redactor,
            &mut safety,
        ),
        unknowns: sanitize_list("unknowns", input.unknowns, options, redactor, &mut safety),
        likely_touchpoints: sanitize_list(
            "likely_touchpoints",
            input.likely_touchpoints,
            options,
            redactor,
            &mut safety,
        ),
        ambiguity: input.ambiguity,
        handoff_ready: input.handoff_ready,
        safety,
    };
    snapshot.snapshot_id = format!("ctx_{}", digest12_json(&snapshot));
    snapshot
}

pub fn write_context_snapshot_artifact(
    artifact_store: &ArtifactStore,
    snapshot: &ContextSnapshot,
) -> Result<(ArtifactRef, u64), ArtifactStoreError> {
    let body = serde_json::to_string_pretty(snapshot)
        .expect("context snapshot serialization should be infallible");
    let artifact = artifact_store.write_text(
        &format!(
            "{CONTEXT_SNAPSHOT_ARTIFACT_DIR}/{}.json",
            snapshot.snapshot_id
        ),
        &body,
    )?;
    Ok((artifact, body.len() as u64))
}

pub fn snapshot_write_result(
    snapshot: &ContextSnapshot,
    artifact: &ArtifactRef,
    artifact_bytes: u64,
) -> ContextSnapshotWriteResult {
    ContextSnapshotWriteResult {
        snapshot_id: snapshot.snapshot_id.clone(),
        slug: snapshot.slug.clone(),
        artifact_path: artifact.path.clone(),
        artifact_digest: artifact.digest.clone().unwrap_or_default(),
        artifact_bytes,
        ambiguity_score: snapshot.ambiguity.score,
        capped: !snapshot.safety.capped_fields.is_empty(),
        redacted: snapshot.safety.redacted,
    }
}

fn sanitize_facts<R: Redactor + ?Sized>(
    facts: Vec<ContextSnapshotFact>,
    options: ContextSnapshotOptions,
    redactor: &R,
    safety: &mut ContextSnapshotSafetyReport,
) -> Vec<ContextSnapshotFact> {
    let original_len = facts.len();
    let facts = facts
        .into_iter()
        .take(options.max_list_items)
        .enumerate()
        .map(|(index, fact)| ContextSnapshotFact {
            source: sanitize_text(
                &format!("known_facts[{index}].source"),
                fact.source,
                options,
                redactor,
                safety,
            ),
            summary: sanitize_text(
                &format!("known_facts[{index}].summary"),
                fact.summary,
                options,
                redactor,
                safety,
            ),
            refs: sanitize_list(
                &format!("known_facts[{index}].refs"),
                fact.refs,
                options,
                redactor,
                safety,
            ),
        })
        .collect::<Vec<_>>();
    if original_len > options.max_list_items {
        safety.record_cap("known_facts", original_len);
    }
    facts
}

fn sanitize_list<R: Redactor + ?Sized>(
    field: &str,
    values: Vec<String>,
    options: ContextSnapshotOptions,
    redactor: &R,
    safety: &mut ContextSnapshotSafetyReport,
) -> Vec<String> {
    let original_len = values.len();
    let values = values
        .into_iter()
        .take(options.max_list_items)
        .enumerate()
        .map(|(index, value)| {
            sanitize_text(
                &format!("{field}[{index}]"),
                value,
                options,
                redactor,
                safety,
            )
        })
        .collect::<Vec<_>>();
    if original_len > options.max_list_items {
        safety.record_cap(field, original_len);
    }
    values
}

fn sanitize_text<R: Redactor + ?Sized>(
    field: &str,
    value: String,
    options: ContextSnapshotOptions,
    redactor: &R,
    safety: &mut ContextSnapshotSafetyReport,
) -> String {
    let redacted = redactor.redact_text(&value);
    if redacted != value {
        safety.redacted = true;
    }

    let char_count = redacted.chars().count();
    if char_count <= options.max_text_chars {
        return redacted;
    }

    safety.record_cap(field, char_count);
    let mut capped = redacted
        .chars()
        .take(options.max_text_chars.saturating_sub(32))
        .collect::<String>();
    capped.push_str(&format!("… [capped from {char_count} chars]"));
    capped
}

fn slug_from_text(text: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "workflow-context".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::redact::DefaultRedactor;
    use crate::tool::ArtifactStore;

    use super::{
        build_context_snapshot, snapshot_write_result, write_context_snapshot_artifact,
        ContextSnapshotAmbiguity, ContextSnapshotFact, ContextSnapshotInput,
        ContextSnapshotOptions,
    };

    #[test]
    fn context_snapshot_redacts_and_caps_oversized_input() {
        let mut input = ContextSnapshotInput::new(
            "/interview",
            format!("Investigate secret sk-ABCDE12345ABCDE {}", "x".repeat(120)),
            "Working code",
        );
        input.known_facts = vec![
            ContextSnapshotFact {
                source: "from-code".to_string(),
                summary: "Authorization: Bearer secret.token".to_string(),
                refs: vec!["src/lib.rs:1".to_string()],
            },
            ContextSnapshotFact {
                source: "from-user".to_string(),
                summary: "second fact".to_string(),
                refs: vec![],
            },
        ];
        input.ambiguity = ContextSnapshotAmbiguity {
            score: 0.42,
            threshold: 0.2,
        };

        let snapshot = build_context_snapshot(
            input,
            ContextSnapshotOptions {
                max_text_chars: 80,
                max_list_items: 1,
            },
            &DefaultRedactor::default(),
            "2026-05-16T00:00:00Z",
        );
        let json = serde_json::to_string(&snapshot).expect("snapshot json");

        assert!(!json.contains("sk-ABCDE12345ABCDE"));
        assert!(!json.contains("secret.token"));
        assert!(snapshot.safety.redacted);
        assert!(snapshot
            .safety
            .capped_fields
            .iter()
            .any(|field| field == "task_statement"));
        assert_eq!(snapshot.known_facts.len(), 1);
    }

    #[test]
    fn context_snapshot_artifact_ref_includes_digest_and_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = ArtifactStore::new(dir.path().join("artifacts")).expect("artifact store");
        let snapshot = build_context_snapshot(
            ContextSnapshotInput::new("/workflow", "Ship snapshot support", "Snapshot artifact"),
            ContextSnapshotOptions::default(),
            &DefaultRedactor::default(),
            "2026-05-16T00:00:00Z",
        );

        let (artifact, bytes) =
            write_context_snapshot_artifact(&store, &snapshot).expect("write snapshot");
        let result = snapshot_write_result(&snapshot, &artifact, bytes);

        assert!(result
            .artifact_path
            .starts_with("artifacts/context_snapshots/ctx_"));
        assert_eq!(result.artifact_digest.len(), 64);
        assert!(result.artifact_bytes > 0);
        assert!(dir.path().join(&result.artifact_path).exists());
    }

    #[test]
    fn context_snapshot_artifact_write_failure_is_reported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let artifacts = dir.path().join("artifacts");
        fs::create_dir_all(&artifacts).expect("create artifacts dir");
        fs::write(artifacts.join("context_snapshots"), "not a directory")
            .expect("block snapshot directory");
        let store = ArtifactStore::new(&artifacts).expect("artifact store");
        let snapshot = build_context_snapshot(
            ContextSnapshotInput::new("/workflow", "Cannot write", "Failure"),
            ContextSnapshotOptions::default(),
            &DefaultRedactor::default(),
            "2026-05-16T00:00:00Z",
        );

        let err = write_context_snapshot_artifact(&store, &snapshot)
            .expect_err("blocked context_snapshots path should fail");
        assert!(err
            .to_string()
            .contains("failed to create artifact directory"));
    }
}

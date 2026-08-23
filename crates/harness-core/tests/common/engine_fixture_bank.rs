use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunStartedEvent, SCHEMA_VERSION,
};
use harness_core::UnwrapOrAbort;
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootSessionId(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryId(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnId(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderRequestId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyLogVariant {
    BoundariesOmitted,
    MetadataAbsent,
    NoUsage,
    AttachmentMetadataMissing,
    LineageUnknown,
}

impl LegacyLogVariant {
    pub fn omitted_field(self) -> &'static str {
        match self {
            Self::BoundariesOmitted => "boundaries",
            Self::MetadataAbsent => "metadata",
            Self::NoUsage => "usage",
            Self::AttachmentMetadataMissing => "attachment_metadata",
            Self::LineageUnknown => "lineage",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticSession {
    root: RootSessionId,
    entry: EntryId,
    turn: TurnId,
    provider_request: ProviderRequestId,
    provider_history: Vec<ProviderRequestId>,
    events: Vec<EventEnvelopeV1>,
}

impl SemanticSession {
    pub fn events(&self) -> Vec<EventEnvelopeV1> {
        self.events.clone()
    }

    pub fn semantic_ids(&self) -> (&RootSessionId, &EntryId, &TurnId) {
        (&self.root, &self.entry, &self.turn)
    }

    pub fn provider_request_id(&self) -> &ProviderRequestId {
        &self.provider_request
    }

    pub fn provider_history(&self) -> &[ProviderRequestId] {
        &self.provider_history
    }

    pub fn retry(&self, attempt: u32) -> Self {
        let mut retry = self.clone();
        let request_id =
            ProviderRequestId(format!("{}-retry-{attempt}", self.provider_request.0));
        retry.provider_request = request_id.clone();
        retry.provider_history = vec![request_id];
        retry
    }
}

pub struct SemanticSessionBuilder {
    seed: u64,
}

impl SemanticSessionBuilder {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }

    pub fn build(self) -> SemanticSession {
        let root = RootSessionId(format!("root-{:016x}", self.seed));
        let entry = EntryId(format!("entry-{:016x}", self.seed));
        let turn = TurnId(format!("turn-{:016x}", self.seed));
        let provider_request =
            ProviderRequestId(format!("provider-request-{:016x}", self.seed));
        let run_id = format!("run-{:016x}", self.seed);
        let event = EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("event-{:016x}", self.seed),
            seq: 1,
            run_id: run_id.clone().into(),
            mono_ms: 1,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("engine-fixture".into())),
            correlation_id: Some(entry.0.clone()),
            causation_id: Some(turn.0.clone()),
            stream_key: Some(format!("run:{run_id}")),
            payload: EventV1::RunStarted(RunStartedEvent {
                run_name: format!("semantic-{:016x}", self.seed).into(),
                workspace_root: format!("/fixture/{}", root.0),
            }),
        };

        SemanticSession {
            root,
            entry,
            turn,
            provider_request: provider_request.clone(),
            provider_history: vec![provider_request],
            events: vec![event],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootChildFixture {
    pub root: SemanticSession,
    pub child: SemanticSession,
}

impl RootChildFixture {
    pub fn new(seed: u64) -> Self {
        Self {
            root: SemanticSessionBuilder::new(seed).build(),
            child: SemanticSessionBuilder::new(seed ^ 0x9e37_79b9).build(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LegacyLogFixture {
    value: Value,
    omitted_field: &'static str,
}

impl LegacyLogFixture {
    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn omitted_field(&self) -> &'static str {
        self.omitted_field
    }

    pub fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&self.value).unwrap_or_abort()
    }
}

pub fn legacy_log_fixture(seed: u64, variant: LegacyLogVariant) -> LegacyLogFixture {
    let omitted_field = variant.omitted_field();
    let mut value = json!({
        "schema_version": SCHEMA_VERSION,
        "event_id": format!("legacy-{seed:016x}"),
        "seq": 1,
        "run_id": format!("legacy-run-{seed:016x}"),
        "mono_ms": 1,
        "actor": {"kind": "system"},
        "payload": {
            "boundaries": {"start": 1, "end": 1},
            "metadata": {"source": "legacy"},
            "usage": {"input_tokens": 2, "output_tokens": 3},
            "attachment_metadata": {"media_type": "image/png"},
            "lineage": {"root_run_id": format!("legacy-run-{seed:016x}")}
        }
    });
    value
        .get_mut("payload")
        .and_then(Value::as_object_mut)
        .and_then(|payload| payload.remove(omitted_field));

    LegacyLogFixture {
        value,
        omitted_field,
    }
}

#[derive(Clone, Default)]
pub struct SideEffectRecorder {
    entries: Arc<Mutex<Vec<String>>>,
}

impl SideEffectRecorder {
    pub fn record(&self, effect: &str) {
        self.lock_entries().push(effect.to_string());
    }

    pub fn count(&self, effect: &str) -> usize {
        self.lock_entries()
            .iter()
            .filter(|item| item.as_str() == effect)
            .count()
    }

    pub fn ordered(&self) -> Vec<String> {
        self.lock_entries().clone()
    }

    fn lock_entries(&self) -> MutexGuard<'_, Vec<String>> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

pub fn jsonl(events: &[EventEnvelopeV1]) -> Vec<u8> {
    events
        .iter()
        .flat_map(|event| {
            let mut bytes = serde_json::to_vec(event).unwrap_or_abort();
            bytes.push(b'\n');
            bytes
        })
        .collect()
}

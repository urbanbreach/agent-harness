//! Replay of settled journals: boundaries, native payloads, stop facts, provenance and
//! the display projection, built from synthetic journals so no provider runs.
use super::super::{context::Entry, history};
use crate::{
    event::*,
    session::{AssistantPart, AssistantToolCall},
};
use harness_compaction::{NoteKind, Origin, Stop};
use harness_providers::{CompletionRequest, ContinuationExport, MessageRole, ProviderContinuation};
use std::time::{Duration, UNIX_EPOCH};

type Test = Result<(), Box<dyn std::error::Error>>;
const AGENT: &str = "agent";

struct Journal(Vec<EventEnvelopeV1>);

impl Journal {
    fn push(&mut self, kind: ActorKind, turn: Option<&str>, payload: EventV1) -> u64 {
        let seq = self.0.len() as u64 + 1;
        let time = UNIX_EPOCH + Duration::from_millis(1_000 * seq);
        self.0.push(EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("e-{seq}"),
            seq,
            run_id: "run".into(),
            mono_ms: seq,
            ts: Some(humantime::format_rfc3339_millis(time).to_string()),
            actor: EventActor::new(kind, Some(AGENT.into())),
            correlation_id: turn.map(str::to_owned),
            causation_id: None,
            stream_key: None,
            payload,
        });
        seq
    }

    fn user(&mut self, turn: &str, text: &str) -> u64 {
        let payload = EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: turn.into(),
            text: text.into(),
        });
        self.push(ActorKind::User, Some(turn), payload)
    }

    /// One provider round trip ending in an assistant message.
    fn assistant(
        &mut self,
        turn: &str,
        request: &str,
        finish: &str,
        parts: Vec<AssistantPart>,
    ) -> u64 {
        self.push(
            ActorKind::Worker,
            Some(turn),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request.into(),
                provider_id: "mock".into(),
                model_id: "m".into(),
                prompt_summary: String::new(),
                request_digest: String::new(),
                metadata: None,
            }),
        );
        self.push(
            ActorKind::Worker,
            Some(turn),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request.into(),
                finish_reason: finish.into(),
                output_digest: None,
                usage: Some(harness_providers::CompletionUsage {
                    prompt_tokens: 100,
                    completion_tokens: 5,
                    total_tokens: 105,
                    ..Default::default()
                }),
                metadata: None,
            }),
        );
        self.push(
            ActorKind::Worker,
            Some(turn),
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: request.into(),
                tool_call_count: 0,
                parts,
                provenance: None,
                assistant_message: None,
            }),
        )
    }

    /// Marks the newest finished request as stopped at the output limit.
    fn limit_last(&mut self) {
        for event in self.0.iter_mut().rev() {
            if let EventV1::ProviderRequestFinished(finished) = &mut event.payload {
                finished.metadata = Some(ProviderRequestFinishedMetadata {
                    provider_error_category: Some(
                        harness_providers::ProviderErrorCategory::OutputLimitExceeded,
                    ),
                    ..Default::default()
                });
                return;
            }
        }
    }

    fn compaction(&mut self, edit: impl FnOnce(&mut SessionCompactionEvent)) -> u64 {
        let seq = self.0.len() as u64 + 1;
        let mut event = SessionCompactionEvent {
            agent_id: AGENT.into(),
            summary: "summary".into(),
            first_kept_event_seq: seq,
            trigger_reason: "threshold".into(),
            method: Some(harness_compaction::CompactionMethod::Soft),
            ..Default::default()
        };
        edit(&mut event);
        self.push(ActorKind::Worker, None, EventV1::SessionCompaction(event))
    }
}

fn text(text: &str) -> AssistantPart {
    AssistantPart::Text { text: text.into() }
}

fn rebuilt(
    journal: &Journal,
    request: Option<&CompletionRequest>,
) -> Result<Vec<Entry>, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    Ok(history::rebuild(&journal.0, AGENT, true, "", dir.path(), request)?.entries)
}

fn contents(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|e| e.message.content.as_str()).collect()
}

#[test]
fn empty_retention_keeps_the_turn_that_continues_after_the_boundary() -> Test {
    let mut journal = Journal(Vec::new());
    journal.user("t1", "first request");
    journal.assistant("t1", "r1", "stop", vec![text("old answer")]);
    // Nothing kept: the boundary is the compaction's own sequence, mid-turn.
    journal.compaction(|_| {});
    journal.assistant("t1", "r2", "stop", vec![text("continues")]);
    let entries = rebuilt(&journal, None)?;
    assert!(entries[0].compaction.is_some());
    assert_eq!(contents(&entries[1..]), ["continues"]);
    // A missing kept request id never fails the replay.
    let mut journal = Journal(Vec::new());
    journal.user("t1", "request");
    journal.compaction(|e| {
        e.first_kept_request_id = Some("ghost".into());
        e.first_kept_event_seq = 1;
    });
    assert_eq!(contents(&rebuilt(&journal, None)?[1..]), ["request"]);
    Ok(())
}

#[test]
fn canonical_session_keeps_a_compaction_that_retained_nothing() {
    let mut journal = Journal(Vec::new());
    journal.user("t1", "request");
    journal.compaction(|_| {});
    let mut session = crate::session::CanonicalSession::default();
    for event in &journal.0 {
        session.apply(event);
    }
    assert!(session.active_entries().any(|entry| matches!(
        entry.payload,
        crate::session::SessionEntryPayload::CompactionSummary { .. }
    )));
}

fn native_payload(
    dir: &std::path::Path,
    model: &str,
) -> Result<NativePayloadRef, Box<dyn std::error::Error>> {
    let payload = ProviderContinuation::import(ContinuationExport {
        protocol: "responses".into(),
        provider_id: Some("mock".into()),
        model_id: model.into(),
        data: serde_json::json!([{"type": "compaction", "encrypted_content": "opaque"}]),
    })?;
    let bytes = serde_json::to_vec(&payload.export())?;
    let digest = blake3::hash(&bytes).to_hex().to_string();
    let path = format!("artifacts/native-{digest}.bin");
    std::fs::create_dir_all(dir.join("artifacts"))?;
    std::fs::write(dir.join(&path), &bytes)?;
    Ok(NativePayloadRef {
        path,
        digest,
        bytes: bytes.len() as u64,
    })
}

#[test]
fn native_payload_replays_only_for_its_model_and_covers_entries_through_the_snapshot() -> Test {
    let dir = tempfile::tempdir()?;
    let reference = native_payload(dir.path(), "m")?;
    let mut journal = Journal(Vec::new());
    journal.user("t1", "old request");
    let covered = journal.assistant("t1", "r1", "stop", vec![text("covered")]);
    let uncovered = journal.assistant("t1", "r2", "stop", vec![text("after the snapshot")]);
    journal.compaction(|e| {
        e.summary = "placeholder".into();
        e.first_kept_event_seq = uncovered;
        e.provider_replay_through_seq = Some(covered);
        e.native = Some(NativeCompactionMeta {
            lane: "openai_v1".into(),
            provider_id: Some("mock".into()),
            model_id: Some("m".into()),
            api: Some("responses".into()),
            payload: Some(reference),
        });
    });
    let request = |model: &str| CompletionRequest {
        provider_id: Some("mock".into()),
        model_id: model.into(),
        ..Default::default()
    };
    let matching = history::rebuild(&journal.0, AGENT, true, "", dir.path(), Some(&request("m")))?;
    assert!(matching.entries[0].message.continuation.is_some());
    assert_eq!(contents(&matching.entries[1..]), ["after the snapshot"]);
    // Another model cannot replay it: the originals return and no placeholder is shown.
    for other in [Some(request("other")), None] {
        let fallback = history::rebuild(&journal.0, AGENT, true, "", dir.path(), other.as_ref())?;
        assert!(fallback.entries.iter().all(|e| e.compaction.is_none()));
        assert_eq!(
            contents(&fallback.entries),
            ["old request", "covered", "after the snapshot"]
        );
    }
    Ok(())
}

#[test]
fn stop_facts_decide_which_assistants_replay_and_anchor() -> Test {
    let mut journal = Journal(Vec::new());
    journal.user("t1", "request");
    journal.assistant("t1", "empty-error", "error", vec![]);
    journal.assistant("t1", "cut", "stop", vec![text("partial")]);
    journal.limit_last();
    let entries = rebuilt(&journal, None)?;
    // The empty failed response is dropped; the truncated one stays and anchors.
    assert_eq!(contents(&entries), ["request", "partial"]);
    assert_eq!(entries[1].stop, Stop::Truncated);
    assert!(entries[1].usage.is_some());
    let mut journal = Journal(Vec::new());
    journal.user("t1", "request");
    journal.assistant("t1", "stopped", "cancelled", vec![text("half an answer")]);
    let entries = rebuilt(&journal, None)?;
    assert_eq!(entries[1].stop, Stop::Aborted);
    assert!(entries[1].usage.is_none(), "aborted usage never anchors");
    Ok(())
}

#[test]
fn ordered_parts_and_stored_arguments_replay_verbatim() -> Test {
    let mut journal = Journal(Vec::new());
    journal.user("t1", "request");
    let call = AssistantToolCall {
        tool_call_id: "c1".into(),
        provider_tool_call_id: Some("p1".into()),
        tool_id: "read".into(),
        args_summary: r#"{"path":"b","intent":"look"}"#.into(),
        args_digest: "d".into(),
        provider_call_id: None,
    };
    journal.assistant(
        "t1",
        "r1",
        "stop",
        vec![
            AssistantPart::Reasoning {
                text: "think".into(),
            },
            text("before "),
            AssistantPart::ToolCall(call),
            text("after"),
        ],
    );
    let entries = rebuilt(&journal, None)?;
    let assistant = &entries[1];
    assert_eq!(assistant.message.content, "before after");
    assert_eq!(assistant.parts.len(), 4);
    assert!(matches!(
        assistant.parts[2],
        harness_compaction::ItemPart::Call(0)
    ));
    assert_eq!(
        assistant
            .message
            .assistant_tool_calls
            .as_ref()
            .map(|c| c[0].arguments_json.as_str()),
        Some(r#"{"path":"b","intent":"look"}"#)
    );
    Ok(())
}

#[test]
fn provenance_separates_typed_input_steering_and_notes() -> Test {
    let mut journal = Journal(Vec::new());
    journal.user("t1", "typed");
    journal.push(
        ActorKind::Worker,
        Some("t1"),
        EventV1::RuntimeReminder(RuntimeReminderEvent {
            request_id: "t1".into(),
            kind: RuntimeReminderKind::LengthStop,
            text: "reminder".into(),
            source: None,
        }),
    );
    let entries = rebuilt(&journal, None)?;
    assert_eq!(
        (entries[0].origin, entries[0].note_kind),
        (Origin::User, None)
    );
    assert_eq!(
        (entries[1].origin, entries[1].note_kind),
        (Origin::Host, Some(NoteKind::LengthStop))
    );
    assert_eq!(entries[1].message.role, MessageRole::User);
    Ok(())
}

#[test]
fn summary_entry_carries_commit_time_patches_and_rewrite_marker() -> Test {
    let mut journal = Journal(Vec::new());
    let first = journal.user("t1", "kept request");
    let commit = journal.compaction(|e| {
        e.first_kept_event_seq = first;
        e.from_hook = true;
        e.details = Some(serde_json::json!({"kind": "custom"}));
    });
    journal.push(
        ActorKind::Worker,
        None,
        EventV1::CompactionPatched(CompactionPatchedEvent {
            agent_id: AGENT.into(),
            compaction_seq: commit,
            patch: CompactionPatch::Warning {
                text: "freed too little".into(),
            },
        }),
    );
    let entries = rebuilt(&journal, None)?;
    let summary = &entries[0];
    let state = summary.compaction.as_ref().ok_or("summary missing")?;
    assert_eq!(summary.at_ms, Some(1_000 * commit));
    assert_eq!(state.commit_seq, commit);
    assert!(state.from_hook);
    assert_eq!(state.warning.as_deref(), Some("freed too little"));
    // The marker sits just before the first retained entry.
    assert_eq!(state.history_rewrite_at_ms, Some(1_000 * first - 1));
    let active = history::latest_compaction(&journal.0, AGENT).ok_or("no active compaction")?;
    assert_eq!(
        (active.seq, active.warning()),
        (commit, Some("freed too little"))
    );
    Ok(())
}

#[test]
fn empty_branch_summaries_contribute_nothing() -> Test {
    let mut journal = Journal(Vec::new());
    journal.user("t1", "request");
    for summary in ["  ", "returned from a side branch"] {
        journal.push(
            ActorKind::Worker,
            None,
            EventV1::BranchSummary(BranchSummaryEvent {
                agent_id: AGENT.into(),
                summary: summary.into(),
                ..Default::default()
            }),
        );
    }
    let entries = rebuilt(&journal, None)?;
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.branch_summary.is_some())
            .count(),
        1
    );
    assert_eq!(
        crate::transcript_projection::compaction_cards(&journal.0).len(),
        1
    );
    Ok(())
}

#[test]
fn cards_supersede_per_agent_and_omit_rescued_sources() {
    use crate::transcript_projection::{compaction_cards, SUPERSEDED_SHORT, SUPERSEDED_SUMMARY};
    let mut journal = Journal(Vec::new());
    journal.user("t1", "request");
    let first = journal.compaction(|e| e.short_summary = Some("short".into()));
    journal.user("t2", "more");
    let rescued = journal.compaction(|_| {});
    journal.compaction(|e| e.supersedes_seq = Some(rescued));
    let mut child = journal.0.last().cloned();
    if let Some(event) = child.as_mut()
        && let EventV1::SessionCompaction(data) = &mut event.payload
    {
        data.agent_id = "child".into();
        data.supersedes_seq = None;
        event.seq += 1;
    }
    journal.0.extend(child);
    let cards = compaction_cards(&journal.0);
    let by_agent = |agent: &str| {
        cards
            .iter()
            .filter(move |c| c.agent_id == agent)
            .collect::<Vec<_>>()
    };
    let root = by_agent(AGENT);
    // The rescued source is gone; the first card is superseded, the last stays full.
    assert_eq!(root.len(), 2);
    assert_eq!(
        (root[0].seq, root[0].summary.as_str()),
        (first, SUPERSEDED_SUMMARY)
    );
    assert_eq!(root[0].short_summary.as_deref(), Some(SUPERSEDED_SHORT));
    assert!(!root[1].superseded);
    // A child's own compaction never supersedes the root's latest card.
    assert!(!by_agent("child")[0].superseded);
}

#[test]
fn collapsed_window_trims_an_orphan_head_to_the_first_typed_turn() {
    let mut journal = Journal(Vec::new());
    journal.user("t1", "request");
    let mid = journal.assistant("t1", "r1", "stop", vec![text("kept mid-turn")]);
    let boundary = journal.user("t2", "next typed request");
    journal.assistant("t2", "r2", "stop", vec![text("answer")]);
    journal.compaction(|e| e.first_kept_event_seq = mid);
    let commit = journal.0.len() as u64;
    let window = crate::transcript_projection::collapsed_window(&journal.0, AGENT);
    assert_eq!(
        window.map(|w| (w.compaction_seq, w.first_visible_seq)),
        Some((commit, boundary))
    );
    // With no qualifying boundary the whole kept suffix stays visible.
    let mut single = Journal(Vec::new());
    single.user("t1", "request");
    let kept = single.assistant("t1", "r1", "stop", vec![text("only")]);
    single.compaction(|e| e.first_kept_event_seq = kept);
    let window = crate::transcript_projection::collapsed_window(&single.0, AGENT);
    assert_eq!(window.map(|w| w.first_visible_seq), Some(kept));
}

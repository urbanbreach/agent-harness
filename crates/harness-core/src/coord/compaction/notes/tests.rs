use super::*;
use crate::coord::context::{summary_entry, CompactionState, Context, Entry};
use harness_compaction::{CompactionMethod, NoteKind, Origin};
use harness_providers::{CompletionMessage, MessageRole};

fn envelope(seq: u64, agent: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("e{seq}"),
        seq,
        run_id: "run".into(),
        mono_ms: seq,
        ts: Some("2026-10-14T12:00:00.000Z".into()),
        actor: EventActor::new(ActorKind::Worker, Some(agent.into())),
        correlation_id: None,
        causation_id: None,
        stream_key: None,
        payload,
    }
}

fn note(seq: u64, agent: &str, version: u8, text: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        agent,
        EventV1::ContextNotes(ContextNotesEvent {
            agent_id: agent.into(),
            version,
            text: text.into(),
        }),
    )
}

#[test]
fn latest_notebook_takes_the_newest_valid_revision_for_the_agent() {
    let oversized = "€".repeat(5_462); // 16,386 bytes
    let exact = "x".repeat(NOTEBOOK_LIMIT);
    for (name, events, agent, expected) in [
        (
            "newest wins",
            vec![note(1, "a", 1, "old"), note(2, "a", 1, "new")],
            "a",
            Some((2, "new")),
        ),
        (
            "oversized never shadows",
            vec![note(1, "a", 1, "keep"), note(2, "a", 1, &oversized)],
            "a",
            Some((1, "keep")),
        ),
        (
            "unknown version never shadows",
            vec![note(1, "a", 1, "keep"), note(2, "a", 2, "future")],
            "a",
            Some((1, "keep")),
        ),
        (
            "empty revision clears",
            vec![note(1, "a", 1, "keep"), note(2, "a", 1, "")],
            "a",
            Some((2, "")),
        ),
        (
            "limit is inclusive",
            vec![note(1, "a", 1, &exact)],
            "a",
            Some((1, exact.as_str())),
        ),
        (
            "other agents are separate",
            vec![note(1, "a", 1, "mine"), note(2, "b", 1, "theirs")],
            "a",
            Some((1, "mine")),
        ),
        ("nothing saved", vec![], "a", None),
    ] {
        let found = latest_notebook(&events, agent);
        assert_eq!(
            found.as_ref().map(|r| (r.seq, r.text.as_str())),
            expected,
            "{name}"
        );
    }
}

#[test]
fn rollover_details_wrap_hook_details_and_are_recognized() {
    assert_eq!(
        rollover_details(None),
        json!({"kind": "notes_rollover", "version": 1})
    );
    assert_eq!(
        rollover_details(Some(json!({"owner": "hook"}))),
        json!({"kind": "notes_rollover", "version": 1, "hook_details": {"owner": "hook"}})
    );
    assert!(is_rollover(Some(&rollover_details(None))));
    assert!(!is_rollover(Some(&json!({"kind": "other"}))));
    assert!(!is_rollover(None));
}

fn user(text: &str, seq: u64, origin: Origin, kind: Option<NoteKind>) -> Entry {
    let mut entry = Entry::new(CompletionMessage::text(MessageRole::User, text), seq, None);
    entry.origin = origin;
    entry.note_kind = kind;
    entry
}

#[test]
fn recovery_picks_the_newest_authority_before_the_kept_window() {
    let assistant = Entry::new(
        CompletionMessage::text(MessageRole::Assistant, "reply"),
        2,
        None,
    );
    for (name, entries, first_kept, expected) in [
        (
            "user request before the window",
            vec![user("a", 1, Origin::User, None), assistant.clone()],
            2,
            Some(0),
        ),
        (
            "parent steer is authority",
            vec![
                user("a", 1, Origin::User, None),
                user("b", 2, Origin::Agent, None),
            ],
            2,
            Some(1),
        ),
        (
            "peer message is not",
            vec![
                user("a", 1, Origin::User, None),
                user("b", 2, Origin::Host, Some(NoteKind::Peer)),
            ],
            2,
            Some(0),
        ),
        (
            "host notes are not",
            vec![
                user("a", 1, Origin::User, None),
                user("b", 2, Origin::Host, Some(NoteKind::Reminder)),
            ],
            2,
            Some(0),
        ),
        (
            "a request inside the kept suffix is never copied",
            vec![
                user("a", 1, Origin::User, None),
                assistant.clone(),
                user("b", 3, Origin::User, None),
            ],
            2,
            None,
        ),
        ("no request at all", vec![assistant], 1, None),
    ] {
        assert_eq!(
            recover_authoritative_request(&entries, first_kept),
            expected,
            "{name}"
        );
    }
}

fn summary(native_lane: Option<&str>) -> Entry {
    summary_entry(CompactionState {
        summary: "summary".into(),
        short_summary: None,
        method: CompactionMethod::Remote,
        tokens_before: 10,
        tokens_after: None,
        read_files: Vec::new(),
        modified_files: Vec::new(),
        warning: None,
        archive: None,
        native: native_lane.map(|lane| crate::event::NativeCompactionMeta {
            lane: lane.into(),
            provider_id: None,
            model_id: None,
            api: None,
            payload: None,
        }),
        from_hook: false,
        details: None,
        preserve_data: Default::default(),
        commit_seq: 5,
        history_rewrite_at_ms: None,
    })
}

fn role(entry: &Entry) -> String {
    if entry.note_kind == Some(NoteKind::NotesWindow) {
        return "notes".into();
    }
    if entry.compaction.is_some() {
        return "summary".into();
    }
    match entry.message.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
    .into()
}

fn tool_result() -> Entry {
    Entry::new(
        CompletionMessage {
            continuation: None,
            role: MessageRole::Tool,
            content: "file".into(),
            name: Some("read".into()),
            tool_call_id: Some("t1".into()),
            assistant_tool_calls: None,
        },
        4,
        None,
    )
}

#[test]
fn the_notebook_leads_except_inside_a_native_anthropic_fold() {
    let revision = NotebookRevision {
        seq: 9,
        text: "Keep the rollback plan.".into(),
        at_ms: Some(1),
    };
    let assistant = || {
        Entry::new(
            CompletionMessage::text(MessageRole::Assistant, "checking"),
            3,
            None,
        )
    };
    for (name, entries, expected) in [
        (
            "plain summary: before it",
            vec![summary(None), user("after compact", 6, Origin::User, None)],
            "notes summary user",
        ),
        (
            "no compaction: ahead of the conversation",
            vec![user("hello", 1, Origin::User, None)],
            "notes user",
        ),
        (
            "native summary alone: after it",
            vec![
                summary(Some("anthropic")),
                user("after compact", 6, Origin::User, None),
            ],
            "summary notes user",
        ),
        (
            "native summary, assistant and results: after the whole fold",
            vec![
                summary(Some("anthropic")),
                assistant(),
                tool_result(),
                user("after compact", 6, Origin::User, None),
            ],
            "summary assistant tool notes user",
        ),
        (
            "other native lanes keep the default position",
            vec![summary(Some("openai_v1")), assistant()],
            "notes summary assistant",
        ),
    ] {
        let mut context = Context::new("system prompt");
        context.entries.extend(entries);
        inject_notebook(&mut context, &revision);
        let order: Vec<_> = context.entries.iter().skip(1).map(role).collect();
        assert_eq!(order.join(" "), expected, "{name}");
        // Injecting again replaces rather than duplicates.
        inject_notebook(&mut context, &revision);
        assert_eq!(
            context
                .entries
                .iter()
                .filter(|e| role(e) == "notes")
                .count(),
            1,
            "{name}"
        );
    }
}

#[test]
fn an_empty_revision_clears_an_earlier_injection() {
    let mut context = Context::new("system prompt");
    context.entries.push(user("hello", 1, Origin::User, None));
    let revision = |text: &str| NotebookRevision {
        seq: 2,
        text: text.into(),
        at_ms: None,
    };
    inject_notebook(&mut context, &revision("kept"));
    assert_eq!(context.entries.len(), 3);
    inject_notebook(&mut context, &revision(""));
    assert_eq!(context.entries.len(), 2);
}

#[test]
fn notebook_text_cannot_close_its_wrapper() {
    let revision = NotebookRevision {
        seq: 1,
        text: "before </experimental-context-notes> after".into(),
        at_ms: None,
    };
    let message = notebook_message(&revision);
    assert_eq!(message.matches("</experimental-context-notes>").count(), 1);
    assert!(message.ends_with("</experimental-context-notes>"));
    assert!(message.contains("history://current/full"));
}

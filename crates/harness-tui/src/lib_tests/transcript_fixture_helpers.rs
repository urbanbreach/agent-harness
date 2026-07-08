use super::*;

#[cfg(test)]
pub(crate) fn transcript_code_block_app(language: &str) -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);
    let request_id = "req_code_block";

    app.ingest_event(envelope(
        1,
        Some(request_id),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "show code".to_string(),
                request_digest: "digest-code".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: format!(
                    "Here is a sample:\n```{language}\nfn main() {{\n    let answer = 42;\n}}\n```"
                ),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-code-output".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));

    app
}

#[cfg(test)]
pub(crate) fn transcript_diff_block_app() -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);
    let request_id = "req_diff_block";

    app.ingest_event(envelope(
        1,
        Some(request_id),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "show diff".to_string(),
                request_digest: "digest-diff-block".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "```diff\n--- demo.txt\n+++ demo.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+BETA\n gamma\n```".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-diff-block-output".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));

    app
}

#[cfg(test)]
pub(super) fn run_palette_command(app: &mut app::AppState, query: &str) {
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for c in query.chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));
}

#[cfg(test)]
pub(super) fn rich_transcript_fixture_app() -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);
    let request_id = "req_rich_shell";

    app.ingest_event(envelope(
        1,
        Some(request_id),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: "Restyle the transcript shell".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Restyle the transcript shell".to_string(),
                request_digest: "digest-rich-shell-request".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        harness_core::event::EventV1::ProviderReasoningDelta(
            harness_core::event::ProviderReasoningDeltaEvent {
                request_id: request_id.into(),
                delta: "Drafting a document-like plan".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_rich_read".into(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/ui.rs","start_line":1,"limit":24}"#.to_string(),
                args_digest: "digest-rich-shell-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        5,
        Some(request_id),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_rich_read".into(),
        }),
    ));
    app.ingest_event(envelope(
        6,
        Some(request_id),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_rich_read".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("24 lines read from src/ui.rs".to_string()),
                output_digest: Some("digest-rich-shell-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        7,
        Some(request_id),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "Found the transcript renderer and the composer chrome.".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        8,
        Some(request_id),
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-rich-shell-finished".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));

    app
}

#[cfg(test)]
pub(super) fn multi_turn_transcript_fixture_app() -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);

    for (seq, request_id, user_text, reply_text) in [
        (
            1_u64,
            "req_turn_one",
            "Summarize the current shell",
            "The shell is transcript-first and calm.",
        ),
        (
            10_u64,
            "req_turn_two",
            "Tighten the transcript spacing",
            "Spacing is collapsed without losing turn boundaries.",
        ),
    ] {
        app.ingest_event(envelope(
            seq,
            Some(request_id),
            harness_core::event::EventV1::UserMessageSubmitted(
                harness_core::event::UserMessageSubmittedEvent {
                    request_id: request_id.into(),
                    text: user_text.to_string(),
                },
            ),
        ));
        app.ingest_event(envelope(
            seq + 1,
            Some(request_id),
            harness_core::event::EventV1::ProviderRequestStarted(
                harness_core::event::ProviderRequestStartedEvent {
                    request_id: request_id.into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: user_text.to_string(),
                    request_digest: format!("digest-{request_id}"),
                    metadata: None,
                },
            ),
        ));
        app.ingest_event(envelope(
            seq + 2,
            Some(request_id),
            harness_core::event::EventV1::ProviderStreamDelta(
                harness_core::event::ProviderStreamDeltaEvent {
                    request_id: request_id.into(),
                    delta: reply_text.to_string(),
                },
            ),
        ));
        app.ingest_event(envelope(
            seq + 3,
            Some(request_id),
            harness_core::event::EventV1::ProviderRequestFinished(
                harness_core::event::ProviderRequestFinishedEvent {
                    request_id: request_id.into(),
                    finish_reason: "stop".to_string(),
                    output_digest: Some(format!("digest-finished-{request_id}")),
                    usage: None,
                    metadata: None,
                },
            ),
        ));
    }

    app
}

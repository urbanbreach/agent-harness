use super::*;
use crate::coord::compaction_tests::{answer, settled, SUMMARY};
use crate::{clock::FakeClock, redact::DefaultRedactor};
use harness_providers::{
    mock::MockProvider, CompletionMessage, CompletionRequest, MessageRole, Provider,
    ProviderEventStream,
};

struct FailedNative(Arc<MockProvider>);
#[async_trait::async_trait]
impl Provider for FailedNative {
    fn native_compaction_lanes(
        &self,
        _: &CompletionRequest,
        _: &harness_providers::NativeLaneSettings,
    ) -> Vec<NativeCompactionLane> {
        vec![NativeCompactionLane::OpenAiV1]
    }
    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        self.0.stream_completion(request).await
    }
}

#[tokio::test]
async fn native_boundary_reexpands_before_soft_summary() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(
        MockProvider::script([
            answer("original answer ".repeat(1000)),
            answer("latest answer"),
            answer(SUMMARY),
            answer("checkpoint"),
        ])
        .with_native_compaction("opaque"),
    );
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn Provider>;
    config.compaction.enabled = false;
    config.compaction.async_enabled = false;
    config.compaction.keep_recent_tokens = 100;
    config.compaction.method_order = vec![CompactionMethod::Remote];
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("native", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    for prompt in ["original request ".repeat(1000), "latest request".into()] {
        let turn = coordinator
            .request_agent_turn(handle::system(), &agent, prompt)
            .await?;
        settled(&coordinator, &turn).await?;
    }
    coordinator
        .compact_agent_context(&agent, None, "native")
        .await?;
    coordinator
        .call(|runtime| {
            runtime.config.compaction.method_order = vec![CompactionMethod::Soft];
            Ok(())
        })
        .await?;
    coordinator
        .compact_agent_context(&agent, None, "soft")
        .await?;
    let requests = provider.captured_requests().await;
    assert!(requests
        .iter()
        .skip(2)
        .flat_map(|request| &request.messages)
        .any(|message| message.content.contains("original request")));
    coordinator.stop_run().await?;
    Ok(())
}

#[tokio::test]
async fn native_failure_uses_portable_endpoint_summary_on_resume(
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}/summary", listener.local_addr()?);
    let server = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await?;
            let mut bytes = vec![0; 65536];
            let _ = socket.read(&mut bytes).await?;
            let body = serde_json::json!({"summary":SUMMARY}).to_string();
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
        }
        Ok::<(), std::io::Error>(())
    });
    let temp = tempfile::tempdir()?;
    let mock = Arc::new(MockProvider::script([
        answer("old answer ".repeat(1000)),
        answer("latest answer"),
        answer("resumed"),
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(FailedNative(Arc::clone(&mock)));
    config.compaction.enabled = false;
    config.compaction.async_enabled = false;
    config.compaction.keep_recent_tokens = 100;
    config.compaction.method_order = vec![CompactionMethod::Remote];
    config.compaction.remote_endpoint = Some(endpoint);
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("endpoint", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    for prompt in ["old request ".repeat(1000), "latest request".into()] {
        let turn = coordinator
            .request_agent_turn(handle::system(), &agent, prompt)
            .await?;
        settled(&coordinator, &turn).await?;
    }
    coordinator
        .compact_agent_context(&agent, None, "endpoint")
        .await?;
    server.await??;
    coordinator.stop_run().await?;
    let mut events = crate::store::read_events(&run.events_path)?;
    // Old journals mislabeled readable custom boundaries as native.
    for event in &mut events {
        if let EventV1::SessionCompaction(summary) = &mut event.payload {
            assert!(summary.native.is_none());
            summary.native = Some(NativeCompactionMeta {
                lane: "custom".into(),
                provider_id: None,
                model_id: None,
                api: Default::default(),
                payload: Default::default(),
            });
        }
    }
    let rebuilt = crate::coord::history::messages(&events, &agent, true, "", &run.run_dir)?;
    assert!(rebuilt
        .entries
        .iter()
        .any(|entry| entry
            .compaction
            .as_ref()
            .is_some_and(
                |state| state.summary.contains("Finish the rewrite") && state.native.is_none()
            )));
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed.resume_run(run.run_id.to_string(), "resume").await?;
    let turn = resumed
        .request_agent_turn(handle::system(), &agent, "resume")
        .await?;
    settled(&resumed, &turn).await?;
    assert!(mock
        .captured_requests()
        .await
        .last()
        .ok_or("request missing")?
        .messages
        .iter()
        .any(|message| message.content.contains("Finish the rewrite")));
    resumed.stop_run().await?;
    Ok(())
}

#[test]
fn anthropic_cut_consumes_same_side_and_protects_tool_pairs() {
    use harness_providers::{AssistantToolCall, CompletionMessage};
    let mut context = Context::default();
    for (index, role) in [
        MessageRole::User,
        MessageRole::Assistant,
        MessageRole::Tool,
        MessageRole::User,
        MessageRole::Assistant,
    ]
    .into_iter()
    .enumerate()
    {
        context.push(
            CompletionMessage::text(role, "text"),
            index as u64 + 1,
            None,
        );
    }
    context.entries[1].message.assistant_tool_calls = Some(vec![AssistantToolCall {
        tool_call_id: "call".into(),
        function_name: "read".into(),
        arguments_json: "{}".into(),
    }]);
    context.entries[2].message.tool_call_id = Some("call".into());
    for (start, expected) in [(1, 1), (2, 4), (3, 4), (4, 4)] {
        assert_eq!(anthropic_cut(&context, start, 5), expected);
    }
}

#[test]
fn retention_keeps_only_what_the_human_typed_and_host_notes_never_start_the_tail() {
    use harness_compaction::{NoteKind, Origin};
    let mut context = Context::default();
    for (index, text) in ["typed", "steer", "reminder", "tail"]
        .into_iter()
        .enumerate()
    {
        context.push(
            CompletionMessage::text(MessageRole::User, text),
            index as u64 + 1,
            None,
        );
    }
    context.entries[1].origin = Origin::Agent;
    context.entries[2].origin = Origin::Host;
    context.entries[2].note_kind = Some(NoteKind::Reminder);
    let kept: Vec<_> = retained::retained_messages(&context.entries)
        .into_iter()
        .map(|message| message.content)
        .collect();
    assert_eq!(kept, ["typed", "tail"]);
    // A boundary before a harness note is skipped even where the sides would change.
    context.entries.insert(1, {
        let mut assistant = context.entries[0].clone();
        assistant.message.role = MessageRole::Assistant;
        assistant
    });
    context.entries[2].origin = Origin::Host;
    assert_eq!(anthropic_cut(&context, 2, 5), 5);
}

#[test]
fn carried_file_metadata_cutoff_follows_the_first_kept_entry() {
    use harness_providers::FilesDue;
    let entries = |spec: &[(MessageRole, Option<u64>)]| {
        let mut context = Context::default();
        for (index, (role, at)) in spec.iter().enumerate() {
            context.push_timed(
                CompletionMessage::text(*role, "x"),
                index as u64 + 1,
                None,
                *at,
            );
        }
        context.entries
    };
    use MessageRole::{Assistant, Tool, User};
    // (entries, cut, expected)
    let cases = [
        // The kept tail starts with an assistant: its own time.
        (
            vec![(User, Some(5)), (Assistant, Some(10))],
            1,
            FilesDue::Before(10),
        ),
        // A user starts the tail: the last summarized message that is not a tool result.
        (
            vec![
                (User, Some(5)),
                (Assistant, Some(10)),
                (Tool, Some(12)),
                (User, Some(20)),
            ],
            3,
            FilesDue::Before(10),
        ),
        // Nothing summarized before a user-side tail: everything is carried.
        (vec![(User, Some(20))], 0, FilesDue::Nothing),
        // Nothing kept: everything joins the summarized request.
        (
            vec![(User, Some(5)), (Assistant, Some(10))],
            2,
            FilesDue::All,
        ),
        // No time to judge by: positions decide.
        (vec![(User, None), (Assistant, None)], 1, FilesDue::Position),
    ];
    for (spec, cut, expected) in cases {
        assert_eq!(
            anthropic::files_due(&entries(&spec), cut),
            expected,
            "{spec:?}"
        );
    }
}

use super::*;
use crate::{AssistantToolCall, CompletionMessage, MessageRole, ProviderRequestContext, ToolDef};
use std::os::unix::fs::PermissionsExt;

/// Answers control requests and echoes replayed user messages. Its first turn plans one
/// `Read` (`tool`); a turn ending in "slow" streams "partial" and waits for an interrupt;
/// other turns reply "ok".
const FAKE_CLAUDE: &str = r#"#!/usr/bin/env python3
import json, os, sys, uuid
log = open(os.environ["FAKE_CLAUDE_LOG"], "a", buffering=1)
log.write(json.dumps({"argv": sys.argv[1:], "token": os.environ.get("CLAUDE_CODE_OAUTH_TOKEN")}) + "\n")
reject = os.environ.get("FAKE_CLAUDE_REJECT_FIRST")
reject = bool(reject) and not os.path.exists(reject) and not open(reject, "w").close()
mode, sid = os.environ["FAKE_CLAUDE_MODE"], str(uuid.uuid4())
def out(o): print(json.dumps(dict(o, session_id=sid)), flush=True)
def event(e): out({"type": "stream_event", "event": e, "uuid": str(uuid.uuid4())})
def result(user, stop): out({"type": "result", "subtype": "success", "is_error": False, "result": "", "stop_reason": stop, "usage": {"input_tokens": 1, "output_tokens": 1}, "uuid": str(uuid.uuid4()), "user_message_uuid": user})
turn, waiting = 0, None
for line in sys.stdin:
    msg = json.loads(line)
    if msg["type"] == "control_request":
        subtype = msg["request"]["subtype"]
        log.write(json.dumps({"control": subtype}) + "\n")
        out({"type": "control_response", "response": {"subtype": "success", "request_id": msg["request_id"], "response": {"still_queued": []} if subtype == "interrupt" else {}}})
        if subtype == "interrupt" and waiting and os.environ.get("FAKE_CLAUDE_INTERRUPT") == "limit":
            out({"type": "result", "subtype": "error_during_execution", "is_error": True, "errors": ["API Error: 429 You've hit your session limit"], "api_error_status": 429, "usage": {"input_tokens": 0, "output_tokens": 0}, "uuid": str(uuid.uuid4()), "user_message_uuid": waiting})
            waiting = None
        elif subtype == "interrupt" and waiting:
            result(waiting, "end_turn")
            waiting = None
    if msg["type"] != "user":
        continue
    log.write(json.dumps({"user": msg["message"]["content"]}) + "\n")
    turn += 1
    out(dict(msg, isReplay=True))
    if reject:
        out({"type": "result", "subtype": "error_during_execution", "is_error": True, "errors": ["authentication_failed: Invalid bearer token"], "api_error_status": 401, "usage": {"input_tokens": 0, "output_tokens": 0}, "uuid": str(uuid.uuid4()), "user_message_uuid": msg["uuid"]})
        continue
    last = msg["message"]["content"][-1]["text"]
    if last == "overflow":
        out({"type": "result", "subtype": "error_during_execution", "is_error": True, "errors": ["API Error: 400 prompt is too long: 250000 tokens > 200000 maximum"], "api_error_status": 400, "usage": {"input_tokens": 0, "output_tokens": 0}, "uuid": str(uuid.uuid4()), "user_message_uuid": msg["uuid"]})
        continue
    if last == "compact":
        out({"type": "system", "subtype": "compact_boundary", "uuid": str(uuid.uuid4()), "compact_metadata": {"trigger": "auto", "pre_tokens": 180000, "post_tokens": 20000}})
    if msg["message"]["content"][-1]["text"] == "slow":
        event({"type": "message_start", "message": {"usage": {"input_tokens": 5, "output_tokens": 1}}})
        event({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
        event({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "partial"}})
        waiting = msg["uuid"]
        continue
    if turn == 1 and mode == "tool":
        block = {"type": "tool_use", "id": "toolu_1", "name": "Read", "input": {}}
        event({"type": "content_block_start", "index": 0, "content_block": block})
        event({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"file_path\": \"/tmp/x\"}"}})
        stop, content = "tool_use", [dict(block, input={"file_path": "/tmp/x"})]
    else:
        event({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
        event({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "ok"}})
        stop, content = "end_turn", [{"type": "text", "text": "ok"}]
    event({"type": "content_block_stop", "index": 0})
    out({"type": "assistant", "message": {"role": "assistant", "content": content, "stop_reason": stop}, "parent_tool_use_id": None, "uuid": str(uuid.uuid4())})
    result(msg["uuid"], stop)
"#;

struct Fake {
    dir: tempfile::TempDir,
    provider: AnthropicSubscriptionProvider,
}

impl Fake {
    fn new(mode: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_env(mode, &[], None)
    }

    fn with_env(
        mode: &str,
        extra: &[(&str, &str)],
        store: Option<Arc<dyn SubscriptionAccountStore>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let executable = dir.path().join("claude");
        std::fs::write(&executable, FAKE_CLAUDE)?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
        let home = dir.path().display().to_string();
        let mut provider = AnthropicSubscriptionProvider::new(dir.path().into())
            .with_agent_dir(dir.path().into())
            .with_environment(
                [
                    ("PATH", std::env::var("PATH")?),
                    ("HOME", home.clone()),
                    ("CLAUDE_CONFIG_DIR", home),
                    ("CLAUDE_CODE_EXECUTABLE", executable.display().to_string()),
                    ("CLAUDE_CODE_OAUTH_TOKEN", "token".into()),
                    (
                        "FAKE_CLAUDE_LOG",
                        dir.path().join("claude.log").display().to_string(),
                    ),
                    ("FAKE_CLAUDE_MODE", mode.into()),
                ]
                .map(|(k, v)| (k.to_owned(), v))
                .into_iter()
                .chain(
                    extra
                        .iter()
                        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
                )
                .collect(),
            );
        if let Some(store) = store {
            provider = provider.with_store(store);
        }
        Ok(Self { dir, provider })
    }

    fn log(&self) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
        Ok(std::fs::read_to_string(self.dir.path().join("claude.log"))?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?)
    }

    fn spawns(&self) -> Result<usize, Box<dyn std::error::Error>> {
        Ok(self
            .log()?
            .iter()
            .filter(|line| line.get("argv").is_some())
            .count())
    }
}

fn request(session: &str, messages: Vec<CompletionMessage>) -> CompletionRequest {
    CompletionRequest {
        provider_id: Some("anthropic-subscription".into()),
        model_id: "claude-opus-5-5".into(),
        stream: true,
        messages,
        context: ProviderRequestContext {
            session_id: Some(session.into()),
            main_turn: true,
            ..ProviderRequestContext::default()
        },
        ..CompletionRequest::default()
    }
}

fn user(text: &str) -> CompletionMessage {
    CompletionMessage::text(MessageRole::User, text)
}

#[tokio::test]
async fn resident_session_plans_tools_and_sends_results_as_a_delta(
) -> Result<(), Box<dyn std::error::Error>> {
    let fake = Fake::new("tool")?;
    let mut request = request("resident-delta-test", vec![user("read x")]);
    request.tools = Some(vec![ToolDef {
        tool_id: "read".into(),
        function_name: "read".into(),
        description: None,
        parameters: serde_json::json!({"type": "object", "properties": {"filePath": {"type": "string"}}}),
    }]);
    let first: Vec<_> = fake
        .provider
        .stream_completion(request.clone())
        .await
        .collect()
        .await;
    let Some((tool_call_id, function_name, arguments_json)) =
        first.iter().find_map(|event| match event {
            Event::ToolCallComplete {
                tool_call_id,
                function_name,
                arguments_json,
            } => Some((
                tool_call_id.clone(),
                function_name.clone(),
                arguments_json.clone(),
            )),
            _ => None,
        })
    else {
        return Err(format!("no tool call: {first:?}").into());
    };
    assert_eq!(
        (
            tool_call_id.as_str(),
            function_name.as_str(),
            arguments_json.as_str()
        ),
        ("toolu_1", "read", r#"{"filePath":"/tmp/x"}"#)
    );
    let mut assistant = CompletionMessage::text(MessageRole::Assistant, "");
    assistant.assistant_tool_calls = Some(vec![AssistantToolCall {
        tool_call_id,
        function_name,
        arguments_json,
    }]);
    let mut result = CompletionMessage::text(MessageRole::Tool, "contents");
    result.tool_call_id = Some("toolu_1".into());
    result.name = Some("read".into());
    request.messages.extend([assistant, result]);
    let second: Vec<_> = fake
        .provider
        .stream_completion(request)
        .await
        .collect()
        .await;
    assert!(
        second.contains(&Event::TextDelta("ok".into())),
        "{second:?}"
    );

    assert_eq!(fake.spawns()?, 1);
    let users: Vec<String> = fake
        .log()?
        .iter()
        .filter_map(|line| line.get("user"))
        .map(|u| u.to_string())
        .collect();
    assert_eq!(users.len(), 2, "{users:?}");
    assert!(
        users[1].contains("Tool result (Read, id=toolu_1):") && !users[1].contains("read x"),
        "{}",
        users[1]
    );
    Ok(())
}

/// senpi's abort settles the interrupted turn before the stream ends, so the next turn reuses
/// the process; leaving the lane closes it, and compaction drops the restart binding.
#[tokio::test]
async fn aborts_settle_and_session_events_follow_senpi_hooks(
) -> Result<(), Box<dyn std::error::Error>> {
    let fake = Fake::new("slow")?;
    let session = "abort-and-events-test";
    let abort = CancellationToken::new();
    let mut stream = fake
        .provider
        .stream_completion_abortable(request(session, vec![user("slow")]), abort.clone())
        .await;
    while let Some(event) = stream.next().await {
        if event == Event::TextDelta("partial".into()) {
            break;
        }
    }
    abort.cancel();
    let rest: Vec<_> = stream.collect().await;
    assert!(
        matches!(rest.last(), Some(Event::Aborted { usage: Some(usage) }) if usage.prompt_tokens == 5),
        "{rest:?}"
    );
    assert!(fake
        .log()?
        .iter()
        .any(|line| line["control"] == "interrupt"));

    let history = vec![user("slow"), user("again")];
    let again: Vec<_> = fake
        .provider
        .stream_completion(request(session, history.clone()))
        .await
        .collect()
        .await;
    assert!(again.contains(&Event::TextDelta("ok".into())), "{again:?}");
    assert_eq!(fake.spawns()?, 1);

    // senpi's model_select inside the lane switches the live query in place.
    fake.provider
        .session_event(&ProviderSessionEvent::ModelSelected {
            session_id: session.into(),
            provider_id: "anthropic-subscription".into(),
            model_id: "claude-sonnet-5-5".into(),
        });
    for _ in 0..200 {
        if fake
            .log()?
            .iter()
            .any(|line| line["control"] == "set_model")
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(fake
        .log()?
        .iter()
        .any(|line| line["control"] == "set_model"));
    // thinking_level_select closes the idle query but keeps the binding: the next turn
    // resumes, on the model set above, instead of re-sending history.
    fake.provider
        .session_event(&ProviderSessionEvent::ReasoningSelected {
            session_id: session.into(),
        });
    let mut history = [
        history,
        vec![
            CompletionMessage::text(MessageRole::Assistant, "ok"),
            user("third"),
        ],
    ]
    .concat();
    let sonnet = |history: Vec<CompletionMessage>| CompletionRequest {
        model_id: "claude-sonnet-5-5".into(),
        ..request(session, history)
    };
    let third: Vec<_> = fake
        .provider
        .stream_completion(sonnet(history.clone()))
        .await
        .collect()
        .await;
    assert!(third.contains(&Event::TextDelta("ok".into())), "{third:?}");
    let argvs: Vec<_> = fake
        .log()?
        .into_iter()
        .filter_map(|line| line.get("argv").cloned())
        .collect();
    assert_eq!(argvs.len(), 2);
    assert!(argvs[1]
        .as_array()
        .into_iter()
        .flatten()
        .any(|a| a.as_str().is_some_and(|a| a.starts_with("--resume="))));

    // A request routed to another provider closes the live query too.
    fake.provider.session_event(&ProviderSessionEvent::Routed {
        session_id: session.into(),
        provider_id: "openai".into(),
        model_id: "gpt".into(),
    });
    history.extend([
        CompletionMessage::text(MessageRole::Assistant, "ok"),
        user("fourth"),
    ]);
    let fourth: Vec<_> = fake
        .provider
        .stream_completion(sonnet(history))
        .await
        .collect()
        .await;
    assert!(
        fourth.contains(&Event::TextDelta("ok".into())),
        "{fourth:?}"
    );
    assert_eq!(fake.spawns()?, 3);

    let bindings = BindingStore::new(fake.dir.path());
    assert!(bindings.read(session).is_some());
    fake.provider
        .session_event(&ProviderSessionEvent::Compacted {
            session_id: session.into(),
        });
    assert!(bindings.read(session).is_none());
    Ok(())
}

/// senpi's lane policy: Claude Code's own compactions are recorded and announced, an overflow of
/// the harness's full re-send is the harness's to compact, and one inside the resident session
/// is reported with manual-compaction guidance instead.
#[tokio::test]
async fn native_compactions_are_recorded_and_overflow_ownership_follows_the_lane(
) -> Result<(), Box<dyn std::error::Error>> {
    let fake = Fake::new("text")?;
    let session = "native-context-test";
    let first: Vec<_> = fake
        .provider
        .stream_completion(request(session, vec![user("compact")]))
        .await
        .collect()
        .await;
    assert!(first.contains(&Event::Notice(
        "Claude Code compacted this conversation (180K tokens before)".into()
    )));
    assert!(
        matches!(first.last(), Some(Event::DoneWithMetadata { metadata: Some(metadata), .. })
            if metadata.session_report.as_ref().is_some_and(|r| r.native_compactions.len() == 1
                && r.native_compactions[0].post_tokens == Some(20_000))),
        "{first:?}"
    );
    let history = vec![
        user("compact"),
        CompletionMessage::text(MessageRole::Assistant, "ok"),
        user("overflow"),
    ];
    let resident: Vec<_> = fake
        .provider
        .stream_completion(request(session, history))
        .await
        .collect()
        .await;
    assert!(
        matches!(resident.last(), Some(Event::Error { message, category: Some(Category::Other), .. })
            if message.contains("/compact")),
        "{resident:?}"
    );
    let cold: Vec<_> = fake
        .provider
        .stream_completion(request("cold-overflow-test", vec![user("overflow")]))
        .await
        .collect()
        .await;
    assert!(
        matches!(
            cold.last(),
            Some(Event::Error {
                category: Some(Category::ContextWindowExceeded),
                ..
            })
        ),
        "{cold:?}"
    );
    Ok(())
}

mod accounts;

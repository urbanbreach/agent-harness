use harness::{run, CliDeps, CliIo};
use harness_core::event::{EventV1, LiveEventV1, RuntimeEvent};
use harness_providers::{
    CompletionRequest, Provider, ProviderEventStream, ProviderStreamEvent as Delta,
};
use std::{
    cell::RefCell,
    io::{Cursor, Write},
    rc::Rc,
    sync::Arc,
};
use tokio::sync::Notify;

struct Streaming(Arc<Notify>);
#[async_trait::async_trait]
impl Provider for Streaming {
    async fn stream_completion(&self, _: CompletionRequest) -> ProviderEventStream {
        let ready = Arc::clone(&self.0);
        let (sender, receiver) = tokio::sync::mpsc::channel(2);
        tokio::spawn(async move {
            if sender
                .send(Delta::TextDelta("Ready. ".into()))
                .await
                .is_err()
            {
                return;
            }
            ready.notified().await;
            for event in [
                Delta::ReasoningDelta("A thought. opaque-".into()),
                Delta::ReasoningDelta("live-token".into()),
                Delta::TextDelta("opaque-".into()),
                Delta::TextDelta("live-token Bear".into()),
                Delta::TextDelta("er\nabc.def\n-----BE".into()),
                Delta::TextDelta(
                    "GIN PRIVATE KEY-----\nhidden body\n-----END PRIVATE KEY-----\nFinished."
                        .into(),
                ),
                Delta::Done { usage: None },
            ] {
                if sender.send(event).await.is_err() {
                    return;
                }
            }
        });
        Box::pin(tokio_stream::wrappers::ReceiverStream::new(receiver))
    }
}
struct Output {
    bytes: Rc<RefCell<Vec<u8>>>,
    ready: Arc<Notify>,
}
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut output = self.bytes.borrow_mut();
        output.extend_from_slice(bytes);
        if output.windows(b"Ready.".len()).any(|s| s == b"Ready.") {
            self.ready.notify_one();
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn prompt_streams_before_completion_without_exposing_split_credentials(
) -> Result<(), Box<dyn std::error::Error>> {
    for (format, thinking) in [("default", true), ("streaming-json", true), ("json", false)] {
        let root = tempfile::tempdir()?;
        let config = root.path().join("config.json");
        std::fs::write(&config, serde_json::json!({
            "provider":{"fixture":{"type":"openai_compatible", "apiKey":"opaque-live-token", "models":{"fixture":{}}}},
            "runtime":{"prompt":{"wait_timeout_ms":2000}}
        }).to_string())?;
        let ready = Arc::new(Notify::new());
        let bytes = Rc::new(RefCell::new(Vec::new()));
        let mut output = Output {
            bytes: Rc::clone(&bytes),
            ready: Arc::clone(&ready),
        };
        let (mut input, mut stderr) = (Cursor::new(Vec::new()), Vec::new());
        let mut args = vec![
            "harness",
            "--config",
            config.to_str().ok_or("config path")?,
            "prompt",
            "--mock",
            "--text",
            "Hello",
            "--format",
            format,
        ];
        if thinking {
            args.push("--thinking");
        }
        let result = run(
            args,
            &mut CliIo::new(&mut input, &mut output, &mut stderr),
            CliDeps::real()
                .with_current_dir(root.path().into())
                .with_provider_override(Arc::new(Streaming(ready))),
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
        let stdout = String::from_utf8(bytes.borrow().clone())?;
        for secret in ["opaque-live-token", "abc.def", "hidden body"] {
            assert!(!stdout.contains(secret), "streamed a secret");
        }
        assert!(stdout.contains("Finished."));
        assert_eq!(stdout.contains("A thought."), thinking);
        if format == "default" {
            assert_eq!(
                stdout.matches("Ready.").count(),
                1,
                "committed text repeated the streamed prefix"
            );
        } else {
            let events: Vec<RuntimeEvent> = if format == "json" {
                serde_json::from_str(&stdout)?
            } else {
                stdout
                    .lines()
                    .map(serde_json::from_str)
                    .collect::<Result<_, _>>()?
            };
            assert!(events.iter().any(|e| matches!(e, RuntimeEvent::Live(e) if matches!(e.payload, LiveEventV1::ProviderTextDelta { .. }))));
            assert!(events.iter().any(|e| matches!(e, RuntimeEvent::Durable(e) if matches!(e.payload, EventV1::AssistantMessageFinished(_)))));
        }
        let session = std::fs::read_dir(root.path().join(".agent-harness/sessions"))?
            .next()
            .ok_or("session missing")??
            .path();
        let journal = std::fs::read_to_string(session.join("events.jsonl"))?;
        assert!(!journal.contains("A thought."));
        assert!(!journal.contains("opaque-live-token"));
    }
    Ok(())
}

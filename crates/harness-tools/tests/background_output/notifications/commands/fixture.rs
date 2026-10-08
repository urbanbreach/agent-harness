use super::*;
use harness_providers::CompletionRequest;

pub(super) struct CommandGate {
    pub(super) first: tokio::sync::Semaphore,
    pub(super) requests: tokio::sync::Mutex<Vec<CompletionRequest>>,
    pub(super) continue_first: bool,
    pub(super) block_followup: bool,
    pub(super) followup_started: tokio::sync::Notify,
    pub(super) child: tokio::sync::Semaphore,
}
#[async_trait::async_trait]
impl harness_providers::Provider for CommandGate {
    async fn stream_completion(
        &self,
        request: CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        let child = request
            .messages
            .last()
            .is_some_and(|message| message.content == "native work");
        let first = {
            let mut requests = self.requests.lock().await;
            let first = requests.iter().all(|request| {
                request
                    .messages
                    .last()
                    .is_some_and(|message| message.content == "native work")
            });
            requests.push(request);
            first
        };
        if child {
            if let Ok(permit) = self.child.acquire().await {
                permit.forget();
            }
            return Box::pin(tokio_stream::iter([Stream::error("fixture child failure")]));
        }
        if first {
            if let Ok(permit) = self.first.acquire().await {
                permit.forget();
            }
            if self.continue_first {
                return Box::pin(tokio_stream::iter([
                    Stream::ToolCallComplete {
                        tool_call_id: "continue-reading".into(),
                        function_name: "read".into(),
                        arguments_json: json!({"filePath":"note"}).to_string(),
                    },
                    Stream::Done { usage: None },
                ]));
            }
        }
        if !first {
            self.followup_started.notify_one();
            if self.block_followup
                && let Ok(permit) = self.first.acquire().await
            {
                permit.forget();
            }
        }
        Box::pin(tokio_stream::iter([
            Stream::TextDelta("turn finished".into()),
            Stream::Done { usage: None },
        ]))
    }
}

pub(super) fn assert_burst_receipts(
    events: &[EventEnvelopeV1],
    observed: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reported = Vec::new();
    let mut notices = 0;
    for event in events {
        let EventV1::RuntimeReminder(reminder) = &event.payload else {
            continue;
        };
        if reminder.kind != RuntimeReminderKind::CommandCompleted {
            continue;
        }
        notices += 1;
        let source = reminder.source.as_deref().ok_or("missing burst source")?;
        if source.starts_with('[') {
            let ids: Vec<String> = serde_json::from_str(source)?;
            assert!(ids.len() <= 64);
            reported.extend(ids);
            assert!(reminder.text.len() < 4000);
        } else {
            reported.push(source.to_owned());
        }
    }
    assert!(
        notices <= 18,
        "overflow must coalesce rather than retain one output body per completion"
    );
    assert_eq!(reported.len(), 100);
    assert!(
        !reported.iter().any(|id| id == observed),
        "observation must survive terminal-cache eviction"
    );
    reported.sort();
    reported.dedup();
    assert_eq!(reported.len(), 100, "each completion is delivered once");
    Ok(())
}

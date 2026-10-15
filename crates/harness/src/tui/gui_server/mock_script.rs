use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_providers::mock::MockProvider;
use harness_providers::{CompletionRequest, Provider, ProviderEventStream, ProviderStreamEvent};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_stream::StreamExt;

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Step {
    Text {
        text: String,
        #[serde(default, rename = "delayMs")]
        delay_ms: u64,
    },
    Reasoning {
        text: String,
        #[serde(default, rename = "delayMs")]
        delay_ms: u64,
    },
    Tool {
        id: String,
        #[serde(rename = "toolId")]
        tool_id: String,
        args: Value,
        #[serde(default, rename = "delayMs")]
        delay_ms: u64,
    },
    Stop {
        #[serde(default, rename = "delayMs")]
        delay_ms: u64,
    },
}

#[derive(Clone)]
pub(super) struct Script(Vec<Vec<Step>>);
impl Script {
    pub(super) fn read(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|error| format!("mock script: {error}"))?;
        let mut bytes = Vec::new();
        file.take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > 1024 * 1024 {
            return Err("invalid_params: mock script exceeds 1 MiB".into());
        }
        Self::parse(&bytes)
    }
    fn parse(bytes: &[u8]) -> Result<Self, String> {
        let turns: Vec<Vec<Step>> = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid_params: mock script: {error}"))?;
        if turns.is_empty() || turns.len() > 256 {
            return Err("invalid_params: mock script requires 1 to 256 turns".into());
        }
        for turn in &turns {
            if turn.is_empty()
                || turn.len() > 4096
                || !matches!(turn.last(), Some(Step::Stop { .. }))
            {
                return Err(
                    "invalid_params: each mock turn requires 1 to 4096 steps ending in stop".into(),
                );
            }
            for (index, step) in turn.iter().enumerate() {
                if matches!(step, Step::Stop { .. }) && index + 1 != turn.len() {
                    return Err("invalid_params: stop must be the last step".into());
                }
                let delay = match step {
                    Step::Text { delay_ms, .. }
                    | Step::Reasoning { delay_ms, .. }
                    | Step::Tool { delay_ms, .. }
                    | Step::Stop { delay_ms } => *delay_ms,
                };
                if delay > 60_000 {
                    return Err("invalid_params: mock step delay exceeds 60 seconds".into());
                }
                if let Step::Tool { id, tool_id, .. } = step
                    && (id.is_empty() || tool_id.is_empty())
                {
                    return Err("invalid_params: mock tool id and toolId must not be empty".into());
                }
            }
        }
        Ok(Self(turns))
    }
    pub(super) fn provider(&self) -> Arc<dyn Provider> {
        let mut turns = Vec::with_capacity(self.0.len());
        let mut delays = VecDeque::with_capacity(self.0.len());
        for turn in &self.0 {
            let mut events = vec![ProviderStreamEvent::Start];
            let mut waits = VecDeque::from([0]);
            for step in turn {
                let (event, delay) = match step {
                    Step::Text { text, delay_ms } => {
                        (ProviderStreamEvent::TextDelta(text.clone()), *delay_ms)
                    }
                    Step::Reasoning { text, delay_ms } => {
                        (ProviderStreamEvent::ReasoningDelta(text.clone()), *delay_ms)
                    }
                    Step::Tool {
                        id,
                        tool_id,
                        args,
                        delay_ms,
                    } => (
                        ProviderStreamEvent::ToolCallComplete {
                            tool_call_id: id.clone(),
                            function_name: harness_providers::tool_function_name(tool_id),
                            arguments_json: args.to_string(),
                        },
                        *delay_ms,
                    ),
                    Step::Stop { delay_ms } => {
                        (ProviderStreamEvent::Done { usage: None }, *delay_ms)
                    }
                };
                events.push(event);
                waits.push_back(delay);
            }
            turns.push(events);
            delays.push_back(waits);
        }
        Arc::new(DelayedMock {
            inner: MockProvider::script(turns),
            delays: Mutex::new(delays),
        })
    }
}

struct DelayedMock {
    inner: MockProvider,
    delays: Mutex<VecDeque<VecDeque<u64>>>,
}
#[async_trait]
impl Provider for DelayedMock {
    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        // Match the delay lane with the turn consumed by the existing mock provider.
        let mut lanes = self.delays.lock().await;
        let mut waits = lanes.pop_front().unwrap_or_default();
        let stream = self.inner.stream_completion(request).await;
        drop(lanes);
        Box::pin(
            stream
                .map(move |event| (event, waits.pop_front().unwrap_or_default()))
                .then(|(event, delay)| async move {
                    if delay > 0 {
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    }
                    event
                }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn script_reuses_mock_turn_order_and_delays_tool_delivery() -> Result<(), String> {
        let script = Script::parse(br#"[[{"kind":"text","text":"Working"},{"kind":"tool","id":"call-1","toolId":"bash","args":{"command":"pwd"},"delayMs":50},{"kind":"stop"}],[{"kind":"reasoning","text":"Finished"},{"kind":"stop"}]]"#)?;
        let provider = script.provider();
        let mut first = provider
            .stream_completion(CompletionRequest::default())
            .await;
        assert_eq!(first.next().await, Some(ProviderStreamEvent::Start));
        assert_eq!(
            first.next().await,
            Some(ProviderStreamEvent::TextDelta("Working".into()))
        );
        assert!(tokio::time::timeout(Duration::from_millis(5), first.next())
            .await
            .is_err());
        assert!(
            matches!(first.next().await, Some(ProviderStreamEvent::ToolCallComplete { tool_call_id, function_name, .. }) if tool_call_id == "call-1" && function_name == "bash")
        );
        assert!(matches!(
            first.next().await,
            Some(ProviderStreamEvent::Done { .. })
        ));
        let second = provider
            .stream_completion(CompletionRequest::default())
            .await
            .collect::<Vec<_>>()
            .await;
        assert!(matches!(
            &second[..],
            [
                ProviderStreamEvent::Start,
                ProviderStreamEvent::ReasoningDelta(_),
                ProviderStreamEvent::Done { .. }
            ]
        ));
        assert!(Script::parse(br#"[[{"kind":"stop"},{"kind":"text","text":"late"}]]"#).is_err());
        Ok(())
    }
}

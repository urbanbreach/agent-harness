use harness_core::UnwrapOrAbort;
use super::*;

async fn cooperative_provider_delay(delay: Duration) {
    let ticks = delay.as_millis().min(250) as usize;
    for _ in 0..ticks {
        tokio::task::yield_now().await;
    }
}

pub(super) struct TestShellTool;

#[async_trait]
impl Tool for TestShellTool {
    fn id(&self) -> &str {
        "shell.run"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text(format!("ok {args_json}")))
    }
}

pub(super) struct CountingShellTool {
    pub(super) calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for CountingShellTool {
    fn id(&self) -> &str {
        "shell.run"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::text(format!("counted {args_json}")))
    }
}

pub(super) struct FailingShellTool;

#[async_trait]
impl Tool for FailingShellTool {
    fn id(&self) -> &str {
        "shell.fail"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        _args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        Err(ToolError::Execution("boom".to_string()))
    }
}

pub(super) struct BlockingShellTool {
    pub(super) release: Arc<Notify>,
}

#[async_trait]
impl Tool for BlockingShellTool {
    fn id(&self) -> &str {
        "shell.block"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        _args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        self.release.notified().await;
        Ok(ToolResult::text("unblocked"))
    }
}

pub(super) struct NamedShellTool {
    pub(super) id: &'static str,
    pub(super) output: &'static str,
    pub(super) started: Option<Arc<Notify>>,
    pub(super) release: Option<Arc<Notify>>,
}

#[async_trait]
impl Tool for NamedShellTool {
    fn id(&self) -> &str {
        self.id
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        _args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        if let Some(started) = &self.started {
            started.notify_one();
        }
        if let Some(release) = &self.release {
            release.notified().await;
        }
        Ok(ToolResult::text(self.output))
    }
}

#[derive(Clone)]
pub(super) struct CapturingProvider {
    captured_requests: Arc<Mutex<Vec<CompletionRequest>>>,
    queued_responses: Arc<Mutex<VecDeque<String>>>,
}

impl CapturingProvider {
    pub(super) fn new(responses: Vec<&str>) -> Self {
        Self {
            captured_requests: Arc::new(Mutex::new(Vec::new())),
            queued_responses: Arc::new(Mutex::new(
                responses.into_iter().map(str::to_string).collect(),
            )),
        }
    }

    pub(super) fn requests(&self) -> Vec<CompletionRequest> {
        self.captured_requests
            .lock()
            .unwrap_or_abort()
            .clone()
    }
}

#[async_trait]
impl Provider for CapturingProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        self.captured_requests
            .lock()
            .unwrap_or_abort()
            .push(req);

        let response = self
            .queued_responses
            .lock()
            .unwrap_or_abort()
            .pop_front()
            .unwrap_or_else(|| "ok".to_string());

        Box::pin(tokio_stream::iter(vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(response),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 3,
                    completion_tokens: 3,
                    total_tokens: 6,
                }),
            },
        ]))
    }
}

#[derive(Clone)]
pub(super) struct DelayedCapturingProvider {
    inner: CapturingProvider,
    pub(super) delay: Duration,
}

impl DelayedCapturingProvider {
    pub(super) fn new(responses: Vec<&str>, delay: Duration) -> Self {
        Self {
            inner: CapturingProvider::new(responses),
            delay,
        }
    }

    pub(super) fn requests(&self) -> Vec<CompletionRequest> {
        self.inner.requests()
    }
}

#[async_trait]
impl Provider for DelayedCapturingProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let delay = self.delay;
        let stream = self
            .inner
            .stream_completion(req)
            .await
            .then(move |event| async move {
                cooperative_provider_delay(delay).await;
                event
            });
        Box::pin(stream)
    }
}

#[derive(Clone)]
pub(super) struct SequentialScriptedProvider {
    captured_requests: Arc<Mutex<Vec<CompletionRequest>>>,
    scripted_events: Arc<[Vec<ProviderStreamEvent>]>,
    next_call_index: Arc<Mutex<usize>>,
}

impl SequentialScriptedProvider {
    pub(super) fn new(scripted_events: Vec<Vec<ProviderStreamEvent>>) -> Self {
        Self {
            captured_requests: Arc::new(Mutex::new(Vec::new())),
            scripted_events: Arc::from(scripted_events),
            next_call_index: Arc::new(Mutex::new(0)),
        }
    }

    pub(super) fn requests(&self) -> Vec<CompletionRequest> {
        self.captured_requests
            .lock()
            .unwrap_or_abort()
            .clone()
    }
}

#[async_trait]
impl Provider for SequentialScriptedProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        // Keep the original request for the fallback closure while recording a clone.
        let fallback_text = req
            .messages
            .iter()
            .filter(|m| matches!(m.role, harness_providers::MessageRole::User))
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        self.captured_requests
            .lock()
            .unwrap_or_abort()
            .push(req.clone());

        let mut next_call_index = self
            .next_call_index
            .lock()
            .unwrap_or_abort();
        let call_index = *next_call_index;
        *next_call_index += 1;

        let events = self.scripted_events.get(call_index).cloned().unwrap_or_else(|| {
            // Out-of-scripted-range calls (e.g., LLM summarization during compaction)
            // echo the request's user messages so tests can still assert on captured
            // content without pre-allocating an exact number of scripted responses.
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta(fallback_text),
                ProviderStreamEvent::Done {
                    usage: Some(CompletionUsage {
                        prompt_tokens: 100,
                        completion_tokens: 100,
                        total_tokens: 200,
                    }),
                },
            ]
        });

        Box::pin(tokio_stream::iter(events))
    }
}
pub(super) fn provider_text_events(text: &str) -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta(text.to_string()),
        ProviderStreamEvent::Done {
            usage: Some(CompletionUsage {
                prompt_tokens: 100,
                completion_tokens: 100,
                total_tokens: 200,
            }),
        },
    ]
}
pub(super) fn test_mock_provider() -> MockProvider {
    let mut scripted = BTreeMap::new();

    for prompt in ["alpha-prompt", "beta-prompt"] {
        let request = CompletionRequest {
            provider_id: Some("mock".to_string()),
            model_id: "model-1".to_string(),
            messages: vec![
                CompletionMessage {
                    role: MessageRole::System,
                    content: prompt.to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: prompt.to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
            ],
            temperature: Some(0.0),
            max_tokens: None,
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            thinking: None,
            tools: None,
            tool_choice: None,
            context: Default::default(),
            stream: true,
        };

        scripted.insert(
            request_digest(&request),
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta(format!("{prompt}-delta")),
                ProviderStreamEvent::Done {
                    usage: Some(CompletionUsage {
                        prompt_tokens: 2,
                        completion_tokens: 1,
                        total_tokens: 3,
                    }),
                },
            ],
        );
    }

    MockProvider::new(scripted)
}

#[derive(Clone)]
pub(super) struct SlowMockProvider {
    pub(super) inner: MockProvider,
    pub(super) delay: Duration,
}

#[async_trait]
impl Provider for SlowMockProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let delay = self.delay;
        let stream = self
            .inner
            .stream_completion(req)
            .await
            .then(move |event| async move {
                cooperative_provider_delay(delay).await;
                event
            });
        Box::pin(stream)
    }
}

#[derive(Clone)]
pub(super) struct PromptScriptedProvider {
    scripts: BTreeMap<String, Vec<ProviderStreamEvent>>,
    pub(super) delay: Duration,
}

impl PromptScriptedProvider {
    pub(super) fn new(scripts: BTreeMap<String, Vec<ProviderStreamEvent>>, delay: Duration) -> Self {
        Self { scripts, delay }
    }
}

#[async_trait]
impl Provider for PromptScriptedProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let prompt = req
            .messages
            .iter()
            .rev()
            .find(|message| message.role == MessageRole::User)
            .map(|message| message.content.clone())
            .unwrap_or_default();

        let events = self.scripts.get(&prompt).cloned().unwrap_or_else(|| {
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta("ok".to_string()),
                ProviderStreamEvent::Done {
                    usage: Some(CompletionUsage {
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        total_tokens: 2,
                    }),
                },
            ]
        });

        let delay = self.delay;
        let stream = tokio_stream::iter(events).then(move |event| async move {
            cooperative_provider_delay(delay).await;
            event
        });
        Box::pin(stream)
    }
}

//! The provider (senpi `stream.ts`, `index.ts`, `tool-watch.ts`).
use super::auth_lane::{
    now_ms, one_shot_attempt, query_with_auth_lane, AuthLaneInput, LaneReport, OptionsBuilder,
};
use super::cold_seed::{learn_from_overflow, raise_calibration};
use super::errors::{
    classify_lane_error, refusal_error, sdk_assistant_failure, sdk_result_failure,
    with_auth_guidance, LaneError, SdkErrorKind,
};
use super::executable::{resolve_claude_code_run, ClaudeCodeRun};
use super::options::{
    build_query_options, AnthropicSubscriptionSettings, QueryOptionsInput, ResumeMode,
};
use super::prompt::{
    build_prompt_blocks, dedupe_ultrawork_blocks, prompt_message, LaneContext, LaneMessage,
};
use super::session::binding::{assistant_content_hash, stored_binding, BindingStore};
use super::session::observability::{emit_continuity_observation, ContinuityObservation};
use super::session::reattach::{get_binding, remember_binding_invalidation};
use super::session::registry::get_session;
use super::session::stream::{resident_session_messages, DispatchShape, ResidentInput};
use super::session::sync::{sent_message_hashes, sent_messages};
use super::session::wiring::{handle_session_event, record_session_provider};
use super::store::SubscriptionAccountStore;
use super::stream_events::{is_context_overflow, StreamState};
use super::tools::{custom_tool_servers, map_host_tool_name_to_sdk, resolve_sdk_tools};
use crate::{
    CompletionRequest, Provider, ProviderErrorCategory as Category, ProviderEventStream,
    ProviderSessionEvent, ProviderStreamEvent as Event, ToolChoice,
};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex},
};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

pub struct AnthropicSubscriptionProvider {
    store: Option<Arc<dyn SubscriptionAccountStore>>,
    settings: AnthropicSubscriptionSettings,
    /// Harness data directory: config-dir accounts, bindings, the pinned Claude Code.
    agent_dir: Option<PathBuf>,
    cwd: PathBuf,
    context_windows: BTreeMap<String, u64>,
    environment: Option<BTreeMap<String, String>>,
}

impl AnthropicSubscriptionProvider {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            store: None,
            settings: AnthropicSubscriptionSettings::default(),
            agent_dir: None,
            cwd,
            context_windows: BTreeMap::new(),
            environment: None,
        }
    }
    pub fn with_store(mut self, store: Arc<dyn SubscriptionAccountStore>) -> Self {
        self.store = Some(store);
        self
    }
    pub fn with_settings(mut self, settings: AnthropicSubscriptionSettings) -> Self {
        self.settings = settings;
        self
    }
    pub fn with_agent_dir(mut self, agent_dir: PathBuf) -> Self {
        self.agent_dir = Some(agent_dir);
        self
    }
    pub fn with_context_windows(mut self, windows: BTreeMap<String, u64>) -> Self {
        self.context_windows = windows;
        self
    }
    /// A fixed environment instead of the process's (tests).
    pub fn with_environment(mut self, environment: BTreeMap<String, String>) -> Self {
        self.environment = Some(environment);
        self
    }
}

/// `toolWatch.buildPromptNote`: tool calls the history shows without a result.
fn tool_watch_note(context: &LaneContext, custom: &BTreeMap<String, String>) -> Option<String> {
    let results: HashSet<&str> = context
        .messages
        .iter()
        .filter_map(|m| match m {
            LaneMessage::ToolResult { tool_call_id, .. } => Some(tool_call_id.as_str()),
            _ => None,
        })
        .collect();
    let mut pending: Vec<(&str, &str, i64)> = context
        .messages
        .iter()
        .filter_map(|m| match m {
            LaneMessage::Assistant {
                tool_calls,
                timestamp,
                ..
            } => Some(
                tool_calls
                    .iter()
                    .map(move |c| (c.id.as_str(), c.name.as_str(), *timestamp)),
            ),
            _ => None,
        })
        .flatten()
        .filter(|(id, _, _)| !results.contains(id))
        .collect();
    pending.sort_by_key(|entry| std::cmp::Reverse(entry.2));
    let notes: Vec<String> = pending
        .into_iter()
        .take(4)
        .map(|(id, name, _)| {
            format!(
                "TOOL RESULT (missing execution {}, id={id}, status=error):\nTool execution did not complete or its result was not observed. Do not guess. Call the tool again.",
                map_host_tool_name_to_sdk(name, Some(custom))
            )
        })
        .collect();
    (!notes.is_empty()).then(|| notes.join("\n\n"))
}

/// senpi `parseCompactBoundaryMessage`, reduced to what the harness records.
fn native_compaction(message: &serde_json::Value) -> crate::ProviderNativeCompaction {
    let metadata = &message["compact_metadata"];
    crate::ProviderNativeCompaction {
        provider_session_id: message["session_id"].as_str().unwrap_or_default().into(),
        boundary_id: message["uuid"].as_str().unwrap_or_default().into(),
        trigger: metadata["trigger"].as_str().unwrap_or("unknown").into(),
        pre_tokens: metadata["pre_tokens"].as_u64(),
        post_tokens: metadata["post_tokens"].as_u64(),
    }
}

const NATIVE_OVERFLOW_GUIDANCE: &str = "Claude Code manages this conversation's context and could not fit it. Run /compact to summarize it here and start a fresh Claude Code session from the summary.";

fn error_category(error: &LaneError, message: &str) -> (Category, Option<u64>) {
    if matches!(error, LaneError::ColdSeedOverflow { .. }) || is_context_overflow(message) {
        return (Category::ContextWindowExceeded, None);
    }
    let blocked = match error {
        LaneError::AllAccountsBlocked(e) => Some(e),
        LaneError::Classified { original, .. } => match original.as_ref() {
            LaneError::AllAccountsBlocked(e) => Some(e),
            _ => None,
        },
        _ => None,
    };
    if let Some(blocked) = blocked {
        if blocked.auth_error {
            return (Category::InvalidCredentials, None);
        }
        let wait = blocked
            .soonest_unblock_at
            .and_then(|at| u64::try_from(at - now_ms()).ok());
        return (Category::RateLimited, wait);
    }
    if error.is_refusal() {
        return (Category::Other, None);
    }
    let classification = match error {
        LaneError::Classified { classification, .. } => classification.clone(),
        other => classify_lane_error(other),
    };
    let category = match classification.kind {
        SdkErrorKind::RateLimit => Category::RateLimited,
        SdkErrorKind::AuthError
        | SdkErrorKind::Billing
        | SdkErrorKind::OrgNotAllowed
        | SdkErrorKind::Entitlement => Category::InvalidCredentials,
        SdkErrorKind::Overloaded => Category::TransportFailure,
        SdkErrorKind::Other if classification.retryable => Category::TransportFailure,
        SdkErrorKind::Other => Category::Other,
    };
    (category, None)
}

static CALIBRATION_RESTORED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);

#[async_trait::async_trait]
impl Provider for AnthropicSubscriptionProvider {
    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        self.stream_completion_abortable(request, CancellationToken::new())
            .await
    }
    /// senpi's `isSdkNativeCompactionLane`: a resident main turn's context is Claude Code's.
    fn manages_context(&self, request: &CompletionRequest) -> bool {
        let settings = match &self.environment {
            Some(environment) => self
                .settings
                .clone()
                .with_env(&|name| environment.get(name).cloned()),
            None => self
                .settings
                .clone()
                .with_env(&|name| std::env::var(name).ok()),
        };
        request.context.main_turn
            && request.context.session_id.is_some()
            && settings.resume_mode != Some(ResumeMode::Off)
    }
    fn session_event(&self, event: &ProviderSessionEvent) {
        let store = self.agent_dir.as_deref().map(BindingStore::new);
        handle_session_event(event, store.as_ref());
    }
    async fn stream_completion_abortable(
        &self,
        request: CompletionRequest,
        abort: CancellationToken,
    ) -> ProviderEventStream {
        let environment: BTreeMap<String, String> = self
            .environment
            .clone()
            .unwrap_or_else(|| std::env::vars().collect());
        let store = self.store.clone();
        let settings = self
            .settings
            .clone()
            .with_env(&|name| environment.get(name).cloned());
        let agent_dir = self.agent_dir.clone();
        let cwd = self.cwd.clone();
        let context_window = self.context_windows.get(&request.model_id).copied();
        let binding_store = agent_dir
            .as_deref()
            .map(|dir| Arc::new(BindingStore::new(dir)));
        // Senpi's abort signal: the lanes interrupt Claude Code, settle, and the stream ends `Aborted`.
        let cancel = abort.child_token();
        let report = Arc::new(Mutex::new(LaneReport::default()));
        Box::pin(async_stream::stream! {
            let mut native_compactions = Vec::new();
            let context = Arc::new(LaneContext::from_request(&request));
            let resolved = resolve_sdk_tools(context.tools.as_deref());
            let session_id = request.context.session_id.clone();
            let note = session_id.as_ref().and_then(|_| tool_watch_note(&context, &resolved.custom_tool_name_to_sdk));
            let tool_less = request.tool_choice == Some(ToolChoice::None);
            let custom_tools = if tool_less { Vec::new() } else { custom_tool_servers(&resolved.custom_tools) };
            let env = |name: &str| environment.get(name).cloned();
            let run: ClaudeCodeRun = match resolve_claude_code_run(&env) {
                Ok(run) => run,
                Err(message) => { yield Event::categorized_error(message, Category::Other); return; }
            };
            if let (Some(store), Some(id)) = (&binding_store, &session_id)
                && CALIBRATION_RESTORED.lock().is_ok_and(|mut seen| seen.insert(id.clone()))
                && let Some((estimated, reported)) = store.read_calibration(id)
            {
                raise_calibration(id, estimated, reported);
            }
            let builder: OptionsBuilder = {
                let (context, settings, cwd, agent_dir, model, sdk_tools, executable, session_id, reasoning) = (
                    Arc::clone(&context), settings.clone(), cwd.clone(), agent_dir.clone(), request.model_id.clone(),
                    resolved.sdk_tools.clone(), run.executable.clone(), session_id.clone(), request.reasoning_effort.clone(),
                );
                Arc::new(move |auth_lane| {
                    let (mut options, guidance) = build_query_options(&QueryOptionsInput {
                        model: &model,
                        context: &context,
                        reasoning: reasoning.as_deref(),
                        tool_less,
                        cwd: &cwd,
                        agent_dir: agent_dir.as_deref(),
                        settings: &settings,
                        auth_lane,
                        tools: &sdk_tools,
                        executable: &executable,
                        session_id: session_id.as_deref(),
                    })?;
                    if let Some(guidance) = guidance {
                        tracing::warn!(guidance, "claude_sdk_oauth_deprecation");
                    }
                    options.custom_tools.clone_from(&custom_tools);
                    Ok(options)
                })
            };
            let lane = AuthLaneInput {
                store,
                settings: settings.clone(),
                environment: environment.clone(),
                agent_dir: agent_dir.clone(),
                session_id: session_id.clone(),
                model: request.model_id.clone(),
                pinned_account: None,
                build_options: builder,
                create_attempt: one_shot_attempt(prompt_message(
                    dedupe_ultrawork_blocks(build_prompt_blocks(&context, Some(&resolved.custom_tool_name_to_sdk), note.as_deref())).blocks,
                ), cancel.clone()),
                cancel: cancel.clone(),
                report: Arc::clone(&report),
            };
            let resident = request.context.main_turn && settings.resume_mode != Some(ResumeMode::Off) && session_id.is_some();
            if request.context.main_turn && !resident {
                emit_continuity_observation(&ContinuityObservation {
                    kind: "disabled",
                    reason: if settings.resume_mode == Some(ResumeMode::Off) { "resume_mode_off" } else { "registry_miss" },
                    delta_messages: None,
                    payload_bytes: None,
                    collapsed_directives: None,
                }, session_id.as_deref());
            }
            let shape = Arc::new(Mutex::new(DispatchShape::default()));
            let _cancel_on_drop = cancel.clone().drop_guard();
            let mut messages = match (&session_id, resident) {
                (Some(id), true) => {
                    record_session_provider(id, request.provider_id.as_deref().unwrap_or_default());
                    resident_session_messages(Arc::new(ResidentInput {
                    session_id: id.clone(),
                    model: request.model_id.clone(),
                    context: Arc::clone(&context),
                    custom_to_sdk: resolved.custom_tool_name_to_sdk.clone(),
                    tool_watch_note: note.clone(),
                    context_window,
                    cancel: cancel.clone(),
                    environment: Arc::new(environment.clone()),
                    binding_store: binding_store.clone(),
                    shape: Arc::clone(&shape),
                }), lane)
                }
                _ => query_with_auth_lane(lane),
            };
            let mut state = StreamState::new();
            let mut started = false;
            let mut failure = None;
            while let Some(item) = messages.next().await {
                let message = match item {
                    Ok(message) => message,
                    Err(error) => { failure = Some(error); break; }
                };
                let failed = refusal_error(&message).or_else(|| match message["type"].as_str() {
                    Some("assistant") => sdk_assistant_failure(&message),
                    Some("result") => sdk_result_failure(&message),
                    _ => None,
                });
                if let Some(error) = failed {
                    failure = Some(error);
                    break;
                }
                if !started {
                    yield Event::Start;
                    started = true;
                }
                for notice in LaneReport::take_notices(&report) {
                    yield Event::Notice(notice);
                }
                match message["type"].as_str() {
                    Some("stream_event") => {
                        for event in state.apply(&message["event"], &resolved.custom_tool_name_to_host) {
                            yield event;
                        }
                    }
                    // senpi mirrors Claude Code's own compactions into the session history.
                    Some("system") if message["subtype"] == "compact_boundary" => {
                        let compaction = native_compaction(&message);
                        yield Event::Notice(match compaction.pre_tokens {
                            Some(tokens) => format!("Claude Code compacted this conversation ({}K tokens before)", tokens / 1000),
                            None => "Claude Code compacted this conversation".into(),
                        });
                        native_compactions.push(compaction);
                    }
                    Some("result") if message["subtype"] == "success" => {
                        for event in state.apply_success_result(&message) {
                            yield event;
                        }
                    }
                    _ => {}
                }
            }
            for notice in LaneReport::take_notices(&report) {
                yield Event::Notice(notice);
            }
            if cancel.is_cancelled() {
                // An aborted turn is never committed: its partial output only bills.
                yield Event::Aborted { usage: state.completion_usage() };
                return;
            }
            if let Some(error) = failure {
                let message = with_auth_guidance(&error, &error.message(), Some(&run));
                let shape = shape.lock().map(|s| s.clone()).unwrap_or_default();
                if shape.cold_seed && is_context_overflow(&message)
                    && let Some(id) = &session_id
                {
                    learn_from_overflow(&error.message(), shape.estimated_tokens, Some(id));
                    if let (Some(store), Some(estimated), Some((reported, _))) =
                        (&binding_store, shape.estimated_tokens, super::cold_seed::parse_reported_overflow_tokens(&error.message()))
                    {
                        store.write_calibration(id, estimated, reported);
                    }
                }
                // Only an overflow of the harness's own re-send is the harness's to compact away;
                // inside a resident session Claude Code owns the context (senpi `ownsCompaction`).
                if resident && !shape.cold_seed && is_context_overflow(&message) {
                    yield Event::categorized_error(format!("{message}\n{NATIVE_OVERFLOW_GUIDANCE}"), Category::Other);
                    return;
                }
                let (category, retry_after_ms) = error_category(&error, &message);
                yield Event::categorized_error_with_retry_after_ms(message, category, retry_after_ms);
                return;
            }
            if resident && let Some(id) = &session_id {
                let hash = assistant_content_hash(&state.text(), &state.tool_calls());
                if let Some(entry) = get_session(id) {
                    let mut entry_state = entry.lock();
                    let sent = entry_state.sent_count;
                    entry_state.provider_final.insert(sent, hash.clone());
                }
                let hashes = sent_message_hashes(&sent_messages(&context));
                if let (Some(store), Some(binding)) = (&binding_store, get_binding(id))
                    && let Some(record) = stored_binding(id, &binding, &hashes, hash)
                    && store.write(&record).is_ok()
                {
                    remember_binding_invalidation(id, None);
                }
            }
            let mut done = state.done();
            if let Event::DoneWithMetadata { metadata: Some(metadata), .. } = &mut done {
                metadata.session_report = Some(Box::new(crate::ProviderSessionReport {
                    account: report.lock().ok().and_then(|report| report.account.clone()),
                    native_compactions,
                }));
            }
            yield done;
        })
    }
}

#[cfg(all(test, unix))]
mod tests;

//! Selection, request and result shaping for one branch-return summary.
use super::super::{
    compaction::generate::extract::{is_cancelled, Reader, RetryMode},
    compaction::{CompactionError, CompactionErrorKind},
    context::Context,
    *,
};
use harness_compaction::{
    files, prompts, serialize,
    tokenizer::{Counter, ReaderApi},
    Item, ItemKind,
};
use harness_providers::{
    CacheRetention, CompletionMessage, CompletionRequest, MessageRole, ProviderRequestContext,
    ProviderRequestInitiator,
};

/// Fallback context window when the model reports none.
const FALLBACK_WINDOW: u32 = 128_000;
/// The branch summary output cap.
const MAX_TOKENS: u32 = 2048;
/// Summary text when nothing was selected to summarize.
pub(super) const NO_CONTENT: &str = "No content to summarize";

/// What one branch summarization produced. Exactly one shape is set: the summary (alone for
/// an empty selection, with file lists after a generation), an abort, or an error.
#[derive(Debug, Default)]
pub(in crate::coord) struct BranchSummaryOutcome {
    pub summary: Option<String>,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
    pub aborted: bool,
    pub error: Option<CompactionError>,
}

impl BranchSummaryOutcome {
    fn summary_only(summary: &str) -> Self {
        Self {
            summary: Some(summary.to_owned()),
            ..Self::default()
        }
    }

    fn aborted() -> Self {
        Self {
            aborted: true,
            ..Self::default()
        }
    }

    fn error(error: CompactionError) -> Self {
        Self {
            error: Some(error),
            ..Self::default()
        }
    }

    /// The journal event this outcome commits, if any. An abort, an error and an empty
    /// selection commit nothing; the first two are reported by the caller.
    pub(super) fn into_event(
        self,
        agent_id: String,
        from_event_seq: u64,
        task: &str,
    ) -> Result<BranchSummaryEvent, CoordinatorError> {
        if let Some(error) = self.error {
            return Err(CoordinatorError::Compaction(error));
        }
        match self.summary {
            Some(summary)
                if !(self.aborted || summary == NO_CONTENT && self.read_files.is_empty()) =>
            {
                Ok(BranchSummaryEvent {
                    agent_id,
                    summary,
                    from_event_seq,
                    read_files: self.read_files,
                    modified_files: self.modified_files,
                    from_hook: false,
                    details: Option::default(),
                })
            }
            _ => Err(CoordinatorError::Cancelled(task.to_owned())),
        }
    }
}

/// Everything a branch summarization job owns.
pub(super) struct BranchJob {
    pub handle: CoordinatorHandle,
    pub provider: Arc<dyn harness_providers::Provider>,
    pub permits: Arc<tokio::sync::Semaphore>,
    pub actor: EventActor,
    pub id: String,
    pub cancel: CancellationToken,
    pub target: ResolvedModelTarget,
    pub reserve: u32,
    pub workspace: PathBuf,
    pub session: String,
    /// Replaces the branch template when truthy.
    pub custom_instructions: Option<String>,
}

/// The conversation tags plus the instruction: a truthy custom instruction replaces the
/// branch template instead of being appended to it.
pub(super) fn branch_request_text(transcript: &str, custom_instructions: Option<&str>) -> String {
    let instructions = custom_instructions
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| prompts::branch_summary_prompt());
    format!("<conversation>\n{transcript}\n</conversation>\n\n{instructions}")
}

/// The window the selection budget derives from: a missing or zero window falls back.
fn window(limits: &crate::config::ResolvedModelLimits) -> u32 {
    limits
        .context_window_tokens()
        .filter(|window| *window > 0)
        .unwrap_or(FALLBACK_WINDOW)
}

/// Selected entries, newest first walked and returned oldest first, with file operations.
struct Selection<'a> {
    items: Vec<Item<'a>>,
    ops: files::FileOps,
}

/// Newest-to-oldest selection under `budget` (zero or negative means unlimited). File
/// operations are read from each assistant before its budget check. A summary may exceed
/// the budget only while total use is strictly below ninety percent, and ends the scan.
fn select<'a>(
    context: &'a Context,
    items: &[Item<'a>],
    budget: u32,
    counter: &Counter,
    mut ops: files::FileOps,
) -> Selection<'a> {
    let mut used = 0u32;
    let mut selected = Vec::new();
    for (entry, item) in context
        .entries
        .iter()
        .skip(context.body_start())
        .zip(items)
        .rev()
        .filter(|(_, item)| !(item.is_tool_result() && item.tool.useless && !item.tool.is_error))
    {
        files::collect_file_ops(std::slice::from_ref(item), &mut ops);
        let tokens = if item.is_tool_result() {
            let serialized = serialize::serialize_for_summary(
                std::slice::from_ref(item),
                serialize::Dialect::Default,
            );
            counter.count_text(
                serialized
                    .strip_prefix("[Tool Result]: ")
                    .unwrap_or(&serialized),
            )
        } else if let Some(summary) = &entry.compaction {
            counter.count_text(&summary.summary)
        } else if let Some(summary) = &entry.branch_summary {
            counter.count_text(&summary.summary)
        } else if item.is_assistant() {
            // Opaque replay material never counts as summarizable text.
            item.tokens_floor
        } else {
            item.tokens
        };
        if budget > 0 && used.saturating_add(tokens) > budget {
            if matches!(item.kind, ItemKind::Summary | ItemKind::BranchSummary)
                && u64::from(used) * 10 < u64::from(budget) * 9
            {
                selected.push(*item);
            }
            break;
        }
        used = used.saturating_add(tokens);
        selected.push(*item);
    }
    selected.reverse();
    Selection {
        items: selected,
        ops,
    }
}

/// Summarizes the abandoned branch in `context`.
pub(super) async fn generate(job: BranchJob, context: Context) -> BranchSummaryOutcome {
    let counter = Counter::for_model(&job.target.provider, &job.target.model, ReaderApi::Other);
    let mut ops = files::FileOps::default();
    for entry in &context.entries {
        if let Some(branch) = &entry.branch_summary
            && !branch.from_hook
        {
            files::merge_previous(&mut ops, &branch.read_files, &branch.modified_files);
        }
    }
    let items = context.items_with(&counter);
    let budget = window(&job.target.limits).saturating_sub(job.reserve);
    let Selection {
        items: mut selected,
        mut ops,
    } = select(&context, &items, budget, &counter, ops);
    if selected.is_empty() {
        return BranchSummaryOutcome::summary_only(NO_CONTENT);
    }
    // Keep excluded result identities so serialization also drops their assistant calls.
    selected.extend(
        items
            .iter()
            .filter(|item| item.is_tool_result() && item.tool.useless && !item.tool.is_error),
    );
    let transcript = serialize::serialize_for_summary(
        &selected,
        serialize::dialect_for_model(&job.target.model),
    );
    let text = match request(&job, &transcript).await {
        Ok(text) => format!("{}{text}", prompts::branch_preamble()),
        Err(outcome) => return *outcome,
    };
    let reads = ops.read.iter().cloned().collect::<Vec<_>>();
    ops.read.clear();
    files::merge_previous(&mut ops, &reads, &[]);
    let (read_files, modified_files) = files::file_lists(&ops);
    let block = files::render_files_block(&read_files, &modified_files, &ops.read);
    let mut summary = files::upsert_files_block(&text, &block);
    if summary.is_empty() {
        "No summary generated".clone_into(&mut summary);
    }
    BranchSummaryOutcome {
        summary: Some(summary),
        read_files,
        modified_files,
        ..BranchSummaryOutcome::default()
    }
}

/// Runs the summarizer request. An abort or a cancelled backoff is the aborted outcome, a
/// resolved provider error the error outcome.
async fn request(job: &BranchJob, transcript: &str) -> Result<String, Box<BranchSummaryOutcome>> {
    let target = &job.target;
    let request = CompletionRequest {
        provider_id: Some(target.provider.clone()),
        model_id: target.model.clone(),
        variant: target.variant.clone(),
        // The exact formula cap; the provider-validity clamp belongs to the encoder.
        max_tokens: Some(MAX_TOKENS),
        messages: vec![
            CompletionMessage::text(MessageRole::System, prompts::summarization_system_prompt()),
            CompletionMessage::text(
                MessageRole::User,
                branch_request_text(transcript, job.custom_instructions.as_deref()),
            ),
        ],
        context: ProviderRequestContext {
            working_directory: Some(job.workspace.clone()),
            session_id: Some(format!(
                "{}:side:{}",
                job.session,
                uuid::Uuid::now_v7().simple()
            )),
            cache_key: Some(job.session.clone()),
            initiator: ProviderRequestInitiator::Agent,
            cache_retention: CacheRetention::None,
            main_turn: false,
            ..Default::default()
        },
        stream: true,
        ..Default::default()
    };
    let reader = Reader {
        handle: &job.handle,
        provider: job.provider.as_ref(),
        permits: &job.permits,
        actor: &job.actor,
        task: &job.id,
        cancel: &job.cancel,
        preview: None,
    };
    match reader
        .run(request, CompactionErrorKind::Branch, RetryMode::Default)
        .await
    {
        Ok(extracted) if extracted.aborted => Err(Box::new(BranchSummaryOutcome::aborted())),
        Ok(extracted) => Ok(extracted.text),
        Err(error) if job.cancel.is_cancelled() || is_cancelled(&error) => {
            Err(Box::new(BranchSummaryOutcome::aborted()))
        }
        Err(error) => Err(Box::new(BranchSummaryOutcome::error(error))),
    }
}

#[cfg(test)]
mod tests;

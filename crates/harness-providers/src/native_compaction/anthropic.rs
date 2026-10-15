//! Anthropic signed-summary lane: payload metadata, request stripping, beta routing and
//! the capability and route gates.
use super::{
    stream, HttpProvider, NativeCompactionError, NativeCompactionLane, NativeCompactionRequest,
    NativeCompactionResult, ProviderContinuation,
};
use crate::{CompletionRequest, Protocol, COMPACTION_PAYLOAD_BYTE_LIMIT};
use serde_json::{json, Value};
use std::sync::LazyLock;

/// Beta that enables the on-demand summarize edit.
pub(crate) const COMPACTION_BETA: &str = "compact-2026-09-04";
/// Longest summary a signed compaction may write.
const MAX_SUMMARY_TOKENS: u32 = 16384;

impl HttpProvider {
    /// Asks for a signed summary of the whole request; the retained tail is not sent.
    pub(super) async fn compact_anthropic(
        &self,
        compaction: &NativeCompactionRequest,
    ) -> Result<NativeCompactionResult, NativeCompactionError> {
        let req = &compaction.request;
        if compaction.max_tokens == 0 {
            return Err(NativeCompactionError::new(
                "positive compaction output limit required",
            ));
        }
        let mut body =
            crate::wire::encode(req, Protocol::Anthropic).map_err(NativeCompactionError::new)?;
        let mut route = self.route(req, &mut body).await?;
        body["max_tokens"] = compaction.max_tokens.min(MAX_SUMMARY_TOKENS).into();
        body["compaction"] = if compaction.instructions.is_empty() {
            json!({"type":"summarize"})
        } else {
            json!({"type":"summarize","instructions":compaction.instructions})
        };
        strip_anthropic(&mut body);
        route_anthropic_beta(&mut body, &mut route.headers, &route.endpoint)?;
        let response = self
            .send_lane(route, Protocol::Anthropic, &body, true)
            .await?;
        let parsed = stream::parse(response, NativeCompactionLane::Anthropic).await?;
        let summary = parsed.item["content"].as_str().map(str::to_owned);
        let mut replay = ProviderContinuation::with_limit(
            Protocol::Anthropic,
            json!({"type":"anthropicCompaction","block":parsed.item}),
            COMPACTION_PAYLOAD_BYTE_LIMIT,
        )
        .map_err(NativeCompactionError::new)?;
        replay.provider_id.clone_from(&req.provider_id);
        replay.model_id.clone_from(&req.model_id);
        Ok(NativeCompactionResult {
            lane: NativeCompactionLane::Anthropic,
            summary,
            replay: Box::new(replay),
            used_tokens: parsed.usage.as_ref().map_or(0, |u| u.prompt_tokens),
            usage: parsed.usage,
            snapshot_through_seq: None,
            short_summary: None,
        })
    }
}

/// Takes the controls of the last live request when it was for the same model, so a
/// signed summary introduces no tool roster or effort change.
pub(super) fn apply_saved_controls(request: &mut CompletionRequest, saved: &CompletionRequest) {
    if saved.provider_id != request.provider_id || saved.model_id != request.model_id {
        return;
    }
    request.tools.clone_from(&saved.tools);
    request.tool_choice = saved.tool_choice;
    request.thinking.clone_from(&saved.thinking);
    request.reasoning_effort.clone_from(&saved.reasoning_effort);
    request.variant.clone_from(&saved.variant);
    request.text_verbosity.clone_from(&saved.text_verbosity);
    request
        .reasoning_summary
        .clone_from(&saved.reasoning_summary);
}
/// File metadata queued behind a signed summary until a safe boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedFile {
    /// Messages after the summary the metadata waits for; the order when times are unknown.
    pub after: usize,
    /// Wall-clock time the metadata is due after, when known.
    pub at_ms: Option<u64>,
    pub text: String,
}

/// Which carried metadata belongs to the summarized prefix rather than the kept tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesDue {
    /// No message is kept: all metadata joins the summarized request.
    All,
    /// Metadata due before this time joins the summarized request; the rest is carried.
    Before(u64),
    /// A user-side message starts the kept tail with nothing summarized before it: every
    /// piece of metadata is carried.
    Nothing,
    /// The kept tail has no known time: judge every piece by its position.
    Position,
}

impl ProviderContinuation {
    /// Keep display file metadata outside the byte-identical signed summary block. `text` is
    /// due after the summary's own time, which is the commit time of the message carrying it.
    pub fn set_anthropic_files(
        &mut self,
        text: String,
        tail: usize,
        retained: Vec<RetainedFile>,
    ) -> Result<(), &'static str> {
        if self.data["type"] != "anthropicCompaction" {
            return Ok(());
        }
        self.data["filesText"] = text.into();
        self.data["exactTail"] = true.into();
        self.data["retainedTail"] = tail.into();
        self.data["retainedFiles"] = retained
            .into_iter()
            .map(|file| {
                let mut value = json!({"after":file.after,"text":file.text});
                if let Some(at) = file.at_ms {
                    value["at"] = at.into();
                }
                value
            })
            .collect();
        self.refresh_native_size()
    }
    /// Splits the metadata a previous summary carries between the summarized prefix request
    /// (what stays in this payload) and the kept tail (what is returned). A piece with a time
    /// is judged against `due`; a piece without one by its position against `cut`. The
    /// previous summary's own text is due after `summary_at_ms`.
    pub fn retain_anthropic_files(
        &mut self,
        summary_at_ms: Option<u64>,
        due: FilesDue,
        index: usize,
        cut: usize,
        end: usize,
    ) -> Result<Vec<RetainedFile>, &'static str> {
        if self.data["type"] != "anthropicCompaction" {
            return Ok(Vec::new());
        }
        let mut metadata = match self.data["retainedFiles"].take() {
            Value::Array(files) => files,
            _ => Vec::new(),
        };
        if let Some(text) = self.data["filesText"]
            .as_str()
            .filter(|text| !text.is_empty())
        {
            let mut own = json!({"text":text,"after":self.data["retainedTail"]});
            if let Some(at) = summary_at_ms {
                own["at"] = at.into();
            }
            metadata.push(own);
        }
        let mut kept = Vec::new();
        let mut prefix = Vec::new();
        for file in metadata {
            let Some(text) = file["text"].as_str() else {
                continue;
            };
            let after = file["after"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(end)
                .saturating_add(index);
            let at_ms = file["at"].as_u64();
            let carried = match (due, at_ms) {
                (FilesDue::Position, _) => after >= cut,
                (FilesDue::All, _) => false,
                (FilesDue::Nothing, _) => true,
                (FilesDue::Before(cutoff), Some(at)) => at >= cutoff,
                (FilesDue::Before(_), None) => after >= cut,
            };
            if carried {
                kept.push(RetainedFile {
                    after: after.saturating_sub(cut).saturating_add(1),
                    at_ms,
                    text: text.to_owned(),
                });
            } else {
                prefix.push(file);
            }
        }
        self.data["filesText"] = Value::Null;
        self.data["retainedFiles"] = prefix.into();
        self.refresh_native_size()?;
        Ok(kept)
    }
    fn refresh_native_size(&mut self) -> Result<(), &'static str> {
        let mut updated = Self::with_limit(
            self.protocol,
            self.data.take(),
            crate::COMPACTION_PAYLOAD_BYTE_LIMIT,
        )?;
        updated.provider_id = self.provider_id.take();
        updated.model_id = std::mem::take(&mut self.model_id);
        *self = updated;
        Ok(())
    }
}

/// Remove the fields a summarize request cannot carry: stop sequences, context
/// management, forced tool choice, structured output and the remaining task budget.
pub(super) fn strip_anthropic(body: &mut Value) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    object.remove("stop_sequences");
    object.remove("context_management");
    if object
        .get("tool_choice")
        .is_some_and(|choice| matches!(choice["type"].as_str(), Some("any" | "tool")))
    {
        object.insert("tool_choice".into(), json!({"type":"auto"}));
    }
    if let Some(config) = object
        .get_mut("output_config")
        .and_then(Value::as_object_mut)
    {
        config.remove("format");
        if let Some(budget) = config.get_mut("task_budget").and_then(Value::as_object_mut) {
            budget.remove("remaining");
        }
        if config.is_empty() {
            object.remove("output_config");
        }
    }
}

/// Claude 4.6 and 4.7 support server-side summaries without an opt-in.
pub(crate) fn anthropic_capable(model: &str) -> bool {
    model.contains("claude")
        && (model.contains("4-6")
            || model.contains("4.6")
            || model.contains("4-7")
            || model.contains("4.7"))
}

fn vertex(host: &str) -> bool {
    static VERTEX: LazyLock<Option<regex::Regex>> = LazyLock::new(|| {
        regex::Regex::new(r"^(?:[a-z0-9-]+[-.])?aiplatform\.googleapis\.com$").ok()
    });
    VERTEX
        .as_ref()
        .is_some_and(|pattern| pattern.is_match(host))
}

/// Anthropic's own host and Vertex need no opt-in for signed summaries.
pub(crate) fn first_party_anthropic(url: &reqwest::Url) -> bool {
    url.host_str()
        .is_some_and(|host| host == "api.anthropic.com" || vertex(host))
}

/// Hosts that serve the summarize edit: Anthropic, Vertex, Azure AI and the AWS route.
pub(crate) fn supported_anthropic(url: &reqwest::Url) -> bool {
    static AWS: LazyLock<Option<regex::Regex>> =
        LazyLock::new(|| regex::Regex::new(r"^aws-external-anthropic\.[a-z0-9-]+\.api\.aws$").ok());
    first_party_anthropic(url)
        || url.host_str().is_some_and(|host| {
            host.ends_with(".services.ai.azure.com")
                && (url.path() == "/"
                    || url.path() == "/anthropic"
                    || url.path().starts_with("/anthropic/"))
                || AWS.as_ref().is_some_and(|pattern| pattern.is_match(host))
        })
}

/// Vertex takes the beta through the body; every other route through the header.
pub(crate) fn route_anthropic_beta(
    body: &mut Value,
    headers: &mut reqwest::header::HeaderMap,
    endpoint: &reqwest::Url,
) -> Result<(), NativeCompactionError> {
    if !endpoint
        .host_str()
        .is_some_and(|host| host.ends_with("aiplatform.googleapis.com"))
    {
        return add_beta(headers);
    }
    let mut betas = body
        .get("anthropic_beta")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(header) = headers.remove("anthropic-beta") {
        for beta in header
            .to_str()
            .map_err(|_| NativeCompactionError::new("invalid beta header"))?
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if !betas.iter().any(|value| value == beta) {
                betas.push(beta.into());
            }
        }
    }
    if !betas.iter().any(|value| value == COMPACTION_BETA) {
        betas.push(COMPACTION_BETA.into());
    }
    body["anthropic_beta"] = betas.into();
    Ok(())
}

pub(crate) fn add_beta(
    headers: &mut reqwest::header::HeaderMap,
) -> Result<(), NativeCompactionError> {
    let old = headers
        .get("anthropic-beta")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let beta = if old.is_empty() {
        COMPACTION_BETA.to_owned()
    } else if old.split(',').any(|s| s.trim() == COMPACTION_BETA) {
        old.to_owned()
    } else {
        format!("{old},{COMPACTION_BETA}")
    };
    headers.insert(
        "anthropic-beta",
        reqwest::header::HeaderValue::from_str(&beta)
            .map_err(|_| NativeCompactionError::new("invalid beta header"))?,
    );
    Ok(())
}

//! The two OpenAI Responses lanes: V2 streams one replacement item, V1 posts to the compact
//! endpoint. Both replace the whole snapshot with opaque provider-native history.
use super::{
    dispatch::Route, fit, input, retention, stream, HttpProvider, NativeCompactionError,
    NativeCompactionLane, NativeCompactionRequest, NativeCompactionResult, NativeEndpointInput,
    Protocol, ProviderContinuation,
};
use crate::{CacheRetention, CompletionRequest, MessageRole, COMPACTION_PAYLOAD_BYTE_LIMIT};
use harness_tokenizer::Counter;
use reqwest::header::HeaderValue;
use serde_json::{json, Value};

impl HttpProvider {
    pub(super) async fn compact_openai(
        &self,
        compaction: &NativeCompactionRequest,
        lane: NativeCompactionLane,
    ) -> Result<NativeCompactionResult, NativeCompactionError> {
        let req = &compaction.request;
        let v1 = lane == NativeCompactionLane::OpenAiV1;
        let mut body =
            crate::wire::encode(req, Protocol::Responses).map_err(NativeCompactionError::new)?;
        let system = system_text(req);
        let wire_model = compaction
            .settings
            .model
            .as_ref()
            .and_then(|model| model.model.clone())
            .unwrap_or_else(|| req.model_id.clone());
        body["model"] = wire_model.clone().into();
        let items = body["input"]
            .as_array_mut()
            .ok_or_else(|| NativeCompactionError::new("invalid native input"))?;
        items.retain(|item| item["role"] != "system");
        if let Some(previous) =
            input::unattached_previous(req, compaction.previous_replay.as_deref())
        {
            prepend_previous(items, previous);
        }
        input::finish_items(items, input::payload_from_other_model(req));
        if items.is_empty() {
            return Err(NativeCompactionError::new("native compaction input empty"));
        }
        let mut route = self.route(req, &mut body).await?;
        let codex = self.auth_profile == Some(crate::ProviderAuthProfile::Codex);
        let endpoints = super::resolve_native_endpoints(&NativeEndpointInput {
            base: Some(route.endpoint.as_str()),
            codex,
            endpoint: compaction.settings.endpoint.as_deref(),
            v2_endpoint: compaction.settings.v2_endpoint.as_deref(),
            legacy_v2_endpoint: None,
        });
        route.endpoint = lane_endpoint(if v1 { &endpoints.v1 } else { &endpoints.v2 })?;
        // V1 falls back to the shared summarization prompt; V2 sends the live system text.
        body["instructions"] = if system.is_empty() && v1 {
            compaction.instructions.clone().into()
        } else {
            system.into()
        };
        if v1 {
            // The compact endpoint accepts only these three fields.
            body = json!({"model":body["model"],"input":body["input"],"instructions":body["instructions"]});
        } else {
            prepare_stream(&mut body, &mut route, compaction)?;
        }
        let counter = counter(req, &wire_model, &route);
        fit::protect(
            &mut body,
            compaction.context_window.unwrap_or(0),
            &counter,
            &req.model_id,
        )?;
        let response = self
            .send_lane(route, Protocol::Responses, &body, !v1)
            .await?;
        let (data, usage) = if v1 {
            (compact_history(read_json(response).await?)?, None)
        } else {
            let parsed = stream::parse(response, lane).await?;
            let history = replacement_history(compaction, parsed.item)?;
            let image_count = history
                .iter()
                .flat_map(|item| item["content"].as_array().into_iter().flatten())
                .filter(|part| part["type"] == "input_image")
                .count();
            let mut data = json!({"type":"openaiResponsesHistory","version":"v2","items":history,"usedTokens":parsed.usage.as_ref().map_or(0, |usage| usage.prompt_tokens),"retainedImageCount":image_count});
            if let Some(details) = parsed.usage_details {
                data["usage"] = details;
            }
            (data, parsed.usage)
        };
        let mut replay = ProviderContinuation::with_limit(
            Protocol::Responses,
            data,
            COMPACTION_PAYLOAD_BYTE_LIMIT,
        )
        .map_err(NativeCompactionError::new)?;
        replay.provider_id.clone_from(&req.provider_id);
        replay.model_id.clone_from(&req.model_id);
        Ok(NativeCompactionResult {
            lane,
            summary: None,
            replay: Box::new(replay),
            used_tokens: usage.as_ref().map_or(0, |u| u.prompt_tokens),
            usage,
            snapshot_through_seq: None,
            short_summary: None,
        })
    }
}

fn system_text(req: &CompletionRequest) -> String {
    req.messages
        .iter()
        .filter(|m| m.role == MessageRole::System)
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Puts the previous replacement history first.
fn prepend_previous(items: &mut Vec<Value>, previous: &ProviderContinuation) {
    let history = previous.data["items"]
        .as_array()
        .into_iter()
        .flatten()
        .cloned();
    items.splice(0..0, history);
}

fn lane_endpoint(target: &str) -> Result<reqwest::Url, NativeCompactionError> {
    reqwest::Url::parse(target)
        .ok()
        .filter(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.username().is_empty()
                && url.password().is_none()
        })
        .ok_or_else(|| NativeCompactionError::new("invalid native compaction endpoint"))
}

fn counter(req: &CompletionRequest, wire_model: &str, route: &Route) -> Counter {
    Counter::new(Some(crate::token_reader::reader_for(
        req,
        wire_model,
        crate::Protocol::Responses,
        route.endpoint.host_str(),
    )))
}

/// The V2 body: the trigger as the final input item, forced streaming without storage, the
/// cache identity and the separate routing identity.
fn prepare_stream(
    body: &mut Value,
    route: &mut Route,
    compaction: &NativeCompactionRequest,
) -> Result<(), NativeCompactionError> {
    let req = &compaction.request;
    body["input"]
        .as_array_mut()
        .ok_or_else(|| NativeCompactionError::new("invalid native input"))?
        .push(json!({"type":"compaction_trigger"}));
    body["stream"] = true.into();
    body["store"] = false.into();
    let cache = compaction
        .cache_key
        .as_deref()
        .or_else(|| body["prompt_cache_key"].as_str())
        .map(input::identity);
    match cache {
        Some(key) if req.context.cache_retention != CacheRetention::None => {
            body["prompt_cache_key"] = key.into();
        }
        _ => {
            if let Some(object) = body.as_object_mut() {
                object.remove("prompt_cache_key");
            }
        }
    }
    for (name, value) in [
        (
            "session_id",
            req.context.session_id.as_deref().map(input::identity),
        ),
        ("x-client-request-id", req.context.request_id.clone()),
    ] {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            route.headers.entry(name).or_insert(
                HeaderValue::from_str(&value)
                    .map_err(|_| NativeCompactionError::new("invalid request identity"))?,
            );
        }
    }
    if body["tools"]
        .as_array()
        .is_some_and(|tools| !tools.is_empty())
    {
        body["tool_choice"] = "auto".into();
    }
    if body.get("reasoning").is_some() {
        body["include"] = json!(["reasoning.encrypted_content"]);
        body["reasoning"]["summary"] = "auto".into();
    }
    if let Some(object) = body.as_object_mut() {
        object.remove("max_output_tokens");
        object.remove("temperature");
    }
    Ok(())
}

/// A response body read under the compaction payload cap.
async fn read_json(mut response: reqwest::Response) -> Result<Value, NativeCompactionError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| NativeCompactionError::new("provider connection closed"))?
    {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > COMPACTION_PAYLOAD_BYTE_LIMIT {
            return Err(NativeCompactionError::new("native response exceeds 32 MiB"));
        }
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| NativeCompactionError::new("native response parse failed"))
}

/// V1 keeps compaction items and user or assistant messages, in order, and needs one valid
/// compaction item among them.
fn compact_history(value: Value) -> Result<Value, NativeCompactionError> {
    let history: Vec<Value> = value["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| {
            matches!(
                v["type"].as_str(),
                Some("compaction" | "compaction_summary")
            ) || v["type"] == "message" && matches!(v["role"].as_str(), Some("user" | "assistant"))
        })
        .cloned()
        .collect();
    if !history.iter().any(|v| {
        v["type"] == "compaction_summary"
            || v["type"] == "compaction" && v["encrypted_content"].is_string()
    }) {
        return Err(NativeCompactionError::new(
            "Remote compaction response missing compaction item",
        ));
    }
    Ok(json!({"type":"openaiResponsesHistory","items":history}))
}

/// V2 history: the retained user messages, then the one compaction item.
fn replacement_history(
    compaction: &NativeCompactionRequest,
    item: Value,
) -> Result<Vec<Value>, NativeCompactionError> {
    let req = &compaction.request;
    let mut retained = input::retained_request(req, &compaction.retained);
    retained.tools = None;
    let mut candidates: Vec<Value> =
        input::previous_history(req, compaction.previous_replay.as_deref())
            .into_iter()
            .cloned()
            .collect();
    let mut serialized =
        crate::wire::encode(&retained, Protocol::Responses).map_err(NativeCompactionError::new)?;
    if let Some(items) = serialized["input"].as_array_mut() {
        input::finish_items(items, false);
        candidates.extend(items.iter().cloned());
    }
    Ok(retention::retain(
        &candidates,
        compaction.retained_message_budget,
        item,
    ))
}

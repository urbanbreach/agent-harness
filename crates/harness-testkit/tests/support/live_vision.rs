use std::fs;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

use super::json_file::read_required_json;

const RESPONSES_ENDPOINT_SUFFIX: &str = "/responses";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveVisionProxyConfig {
    provider_name: String,
    base_url: String,
    api_key: String,
    model_id: String,
}

impl LiveVisionProxyConfig {
    pub(crate) fn new(
        provider_name: String,
        base_url: String,
        api_key: String,
        model_id: String,
    ) -> Result<Self, String> {
        let provider_name =
            required_trimmed(&provider_name, "live vision provider name cannot be empty")?;
        required_trimmed(&base_url, "live vision base URL cannot be empty")?;
        let api_key = required_trimmed(&api_key, "live vision API key cannot be empty")?;
        let model_id = required_trimmed(&model_id, "live vision model ID cannot be empty")?;

        Ok(Self {
            provider_name: provider_name.to_string(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model_id: model_id.to_string(),
        })
    }

    pub(crate) fn model_id(&self) -> &str {
        &self.model_id
    }

    pub(crate) fn responses_endpoint(&self) -> String {
        format!("{}{RESPONSES_ENDPOINT_SUFFIX}", self.base_url)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveVisionVerdict {
    checkpoint_id: String,
    status: String,
    reasons: Vec<String>,
    observed_markers: Vec<String>,
    artifact_path: PathBuf,
}

impl LiveVisionVerdict {
    pub(crate) fn checkpoint_id(&self) -> &str {
        &self.checkpoint_id
    }

    pub(crate) fn status(&self) -> &str {
        &self.status
    }

    pub(crate) fn reasons(&self) -> &[String] {
        &self.reasons
    }

    pub(crate) fn observed_markers(&self) -> &[String] {
        &self.observed_markers
    }

    pub(crate) fn artifact_path(&self) -> &Path {
        &self.artifact_path
    }
}

pub(crate) fn verdict_artifact_path(png_path: &Path) -> PathBuf {
    png_path.with_extension("verdict.json")
}

#[expect(
    dead_code,
    reason = "reserved for the ignored live visual-verifier lane"
)]
pub(crate) fn write_structured_vision_verdict(
    checkpoint_id: &str,
    verdict: &LiveVisionVerdict,
) -> Result<PathBuf, String> {
    let verdict_json = read_required_json(verdict.artifact_path())?;
    let structured_path = verdict
        .artifact_path()
        .with_file_name(format!("{checkpoint_id}.vision.json"));
    let rendered = serde_json::to_string_pretty(&verdict_json)
        .map_err(|err| format!("failed to serialize structured vision verdict JSON: {err}"))?;
    fs::write(&structured_path, rendered).map_err(|err| {
        format!(
            "failed to write structured vision verdict artifact {}: {err}",
            structured_path.display()
        )
    })?;

    Ok(structured_path)
}

pub(crate) async fn verify_checkpoint(
    client: &Client,
    config: &LiveVisionProxyConfig,
    checkpoint_id: &str,
    png_path: &Path,
    expected_markers: &[&str],
) -> Result<LiveVisionVerdict, String> {
    required_trimmed(checkpoint_id, "live vision checkpoint ID cannot be empty")?;

    let png_bytes = fs::read(png_path).map_err(|err| {
        format!(
            "failed to read live vision PNG {}: {err}",
            png_path.display()
        )
    })?;
    let encoded_png = BASE64_STANDARD.encode(png_bytes);
    let endpoint = config.responses_endpoint();

    let response = client
        .post(&endpoint)
        .bearer_auth(&config.api_key)
        .json(&build_responses_request(
            config,
            checkpoint_id,
            &encoded_png,
            expected_markers,
        ))
        .send()
        .await
        .map_err(|err| format!("live vision verifier request to {endpoint} failed: {err}"))?;

    if response.status() != StatusCode::OK {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|err| format!("<failed reading response body: {err}>"));
        return Err(format!(
            "live vision verifier request to {endpoint} failed with status {status}: {body}"
        ));
    }

    let response_json: Value = response.json().await.map_err(|err| {
        format!("live vision verifier response from {endpoint} was not valid JSON: {err}")
    })?;
    let verdict_text = extract_output_text(&response_json)?;

    let (checkpoint_id, status, reasons, observed_markers) =
        parse_verdict_text(checkpoint_id, verdict_text)?;
    let artifact_path = write_verdict_artifact(
        png_path,
        config,
        &checkpoint_id,
        &status,
        &reasons,
        &observed_markers,
        expected_markers,
    )?;

    Ok(LiveVisionVerdict {
        checkpoint_id,
        status,
        reasons,
        observed_markers,
        artifact_path,
    })
}

fn build_responses_request(
    config: &LiveVisionProxyConfig,
    checkpoint_id: &str,
    encoded_png: &str,
    expected_markers: &[&str],
) -> Value {
    json!({
        "model": config.model_id(),
        "input": [{
            "role": "user",
            "content": [
                {
                    "type": "input_text",
                    "text": build_verifier_prompt(checkpoint_id, expected_markers),
                },
                {
                    "type": "input_image",
                    "image_url": format!("data:image/png;base64,{encoded_png}"),
                }
            ]
        }],
        "text": {
            "format": {
                "type": "json_schema",
                "name": "live_vision_verdict",
                "strict": true,
                "schema": verdict_schema(),
            }
        }
    })
}

fn build_verifier_prompt(checkpoint_id: &str, expected_markers: &[&str]) -> String {
    let markers = if expected_markers.is_empty() {
        "(none provided)".to_string()
    } else {
        expected_markers.join(", ")
    };

    format!(
        concat!(
            "You are verifying a single screenshot for checkpoint `{checkpoint_id}`. ",
            "Inspect the attached PNG and decide whether the checkpoint is satisfied. ",
            "Expected markers to look for: {markers}. ",
            "Return strict JSON only with fields checkpoint_id, status, reasons, observed_markers. ",
            "Set checkpoint_id to `{checkpoint_id}` exactly. ",
            "Use a short status string, keep reasons concise, and only include markers visibly confirmed in the screenshot."
        ),
        checkpoint_id = checkpoint_id,
        markers = markers,
    )
}

fn verdict_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["checkpoint_id", "status", "reasons", "observed_markers"],
        "properties": {
            "checkpoint_id": {
                "type": "string",
                "minLength": 1,
            },
            "status": {
                "type": "string",
                "minLength": 1,
            },
            "reasons": {
                "type": "array",
                "items": {
                    "type": "string"
                }
            },
            "observed_markers": {
                "type": "array",
                "items": {
                    "type": "string"
                }
            }
        }
    })
}

fn extract_output_text(response_json: &Value) -> Result<&str, String> {
    let output = response_json
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| "live vision verifier response missing `output` array".to_string())?;

    let mut output_text = None;
    for item in output {
        if item.get("type").and_then(Value::as_str) == Some("output_text") {
            let text = item.get("text").and_then(Value::as_str).ok_or_else(|| {
                "live vision verifier output_text item missing `text`".to_string()
            })?;
            if output_text.replace(text).is_some() {
                return Err(
                    "live vision verifier response contained multiple output text items"
                        .to_string(),
                );
            }
            continue;
        }

        let Some(content) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        for content_item in content {
            if content_item.get("type").and_then(Value::as_str) != Some("output_text") {
                continue;
            }

            let text = content_item
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    "live vision verifier message content missing output text payload".to_string()
                })?;
            if output_text.replace(text).is_some() {
                return Err(
                    "live vision verifier response contained multiple output text items"
                        .to_string(),
                );
            }
        }
    }

    output_text.ok_or_else(|| {
        "live vision verifier response did not include an output_text verdict".to_string()
    })
}

fn parse_verdict_text(
    expected_checkpoint_id: &str,
    verdict_text: &str,
) -> Result<(String, String, Vec<String>, Vec<String>), String> {
    let verdict_json: Value = serde_json::from_str(verdict_text)
        .map_err(|err| format!("live vision verifier returned invalid JSON verdict: {err}"))?;

    let checkpoint_id = required_non_empty_string(&verdict_json, "checkpoint_id")?;
    if checkpoint_id != expected_checkpoint_id {
        return Err(format!(
            "live vision verifier checkpoint mismatch: expected `{expected_checkpoint_id}`, found `{checkpoint_id}`"
        ));
    }

    let status = required_non_empty_string(&verdict_json, "status")?;
    let reasons = required_string_array(&verdict_json, "reasons")?;
    let observed_markers = required_string_array(&verdict_json, "observed_markers")?;

    Ok((checkpoint_id, status, reasons, observed_markers))
}

fn required_non_empty_string(value: &Value, field: &str) -> Result<String, String> {
    let text = value.get(field).and_then(Value::as_str).ok_or_else(|| {
        format!("live vision verifier verdict missing non-empty string field `{field}`")
    })?;

    let trimmed = required_trimmed(
        text,
        format!("live vision verifier verdict missing non-empty string field `{field}`"),
    )?;

    Ok(trimmed.to_string())
}

fn required_trimmed(value: &str, error: impl Into<String>) -> Result<&str, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(error.into())
    } else {
        Ok(trimmed)
    }
}

fn required_string_array(value: &Value, field: &str) -> Result<Vec<String>, String> {
    let items = value.get(field).and_then(Value::as_array).ok_or_else(|| {
        format!("live vision verifier verdict missing string array field `{field}`")
    })?;

    items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            item.as_str()
                .map(|text| text.to_string())
                .ok_or_else(|| {
                    format!(
                        "live vision verifier verdict field `{field}` contains non-string element at index {idx}"
                    )
                })
        })
        .collect()
}

fn write_verdict_artifact(
    png_path: &Path,
    config: &LiveVisionProxyConfig,
    checkpoint_id: &str,
    status: &str,
    reasons: &[String],
    observed_markers: &[String],
    expected_markers: &[&str],
) -> Result<PathBuf, String> {
    let artifact_path = verdict_artifact_path(png_path);
    let artifact = json!({
        "checkpoint_id": checkpoint_id,
        "status": status,
        "reasons": reasons,
        "observed_markers": observed_markers,
        "request": {
            "provider_name": config.provider_name.as_str(),
            "model_id": config.model_id.as_str(),
            "png_path": png_path.display().to_string(),
            "expected_markers": expected_markers,
        }
    });

    let rendered = serde_json::to_string_pretty(&artifact)
        .map_err(|err| format!("failed to serialize live vision verdict artifact: {err}"))?;
    fs::write(&artifact_path, rendered).map_err(|err| {
        format!(
            "failed to write live vision verdict artifact {}: {err}",
            artifact_path.display()
        )
    })?;

    Ok(artifact_path)
}

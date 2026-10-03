use super::{
    runtime::{JobKind, Runtime},
    *,
};
use crate::tool::ArtifactRef;
use serde_json::json;

const INLINE_BYTES: usize = 51_200;
impl CoordinatorHandle {
    pub async fn retain_tool_binary(
        &self,
        task: String,
        bytes: Vec<u8>,
    ) -> Result<ArtifactRef, CoordinatorError> {
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(CoordinatorError::Invalid(
                "binary artifact exceeds 8 MiB".into(),
            ));
        }
        self.call(move |s| {
            s.check_task(&task)?;
            let job = &s.running[&task];
            if job.join_id.is_none() || !matches!(job.kind, JobKind::Tool { .. }) {
                return Err(CoordinatorError::PermissionDenied(
                    "binary artifacts require an approved tool call".into(),
                ));
            }
            let text = String::from_utf8_lossy(&bytes);
            if s.redactor.redact_text(&text) != text {
                return Err(CoordinatorError::Invalid(
                    "binary artifact contains a credential; no artifact was written".into(),
                ));
            }
            let (extension, mime) = if bytes.starts_with(b"%PDF-") {
                ("pdf", "application/pdf")
            } else {
                ("bin", "application/octet-stream")
            };
            let actor = job.actor.clone();
            s.write_blob(
                &actor,
                Some(&task),
                extension,
                &bytes,
                BTreeMap::from([("mime".into(), mime.into())]),
            )
        })
        .await
    }

    pub async fn record_lsp_install_decision(
        &self,
        task: String,
        server: String,
        allowed: bool,
    ) -> Result<ArtifactRef, CoordinatorError> {
        self.call(move |s| {
            s.check_task(&task)?;
            let job = &s.running[&task];
            if job.join_id.is_none()
                || !matches!(&job.kind, JobKind::Tool { tool_id, .. } if tool_id == "lsp")
            {
                return Err(CoordinatorError::PermissionDenied(
                    "installation receipts require an approved LSP call".into(),
                ));
            }
            if server.trim().is_empty()
                || server.len() > 256
                || server.chars().any(char::is_control)
                || s.redactor.redact_text(&server) != server
            {
                return Err(CoordinatorError::Invalid(
                    "invalid or sensitive LSP server identifier".into(),
                ));
            }
            let actor = job.actor.clone();
            let receipt = json!({"operation":"installDecision","serverId":server,"decision":if allowed {"allowed"} else {"declined"},"recorded_only":true});
            s.write_artifact(&actor, Some(&task), "json", &receipt.to_string())
        }).await
    }
}
impl Runtime {
    pub fn bound_tool_output(
        &mut self,
        id: &str,
        actor: &EventActor,
        mut output: ToolResult,
        retain_structured: bool,
    ) -> Result<ToolResult, CoordinatorError> {
        output.display_text = self.redactor.redact_text(&output.display_text);
        if let Some(json) = &mut output.structured_json {
            crate::redact::redact_in_place(self.redactor.as_ref(), json);
        }
        super::prompt::validate_attachments(&output.attachments, self.redactor.as_ref())?;
        let end = output
            .display_text
            .match_indices('\n')
            .nth(2000)
            .map_or(output.display_text.len(), |(i, _)| i);
        let end = output
            .display_text
            .floor_char_boundary(end.min(INLINE_BYTES));
        let json_large = output
            .structured_json
            .as_ref()
            .is_some_and(|value| value.to_string().len() > INLINE_BYTES);
        if end < output.display_text.len() || json_large {
            let full = serde_json::to_string(
                &json!({"display_text":output.display_text, "structured_json":output.structured_json}),
            )?;
            if full.len() > 8 * 1024 * 1024 {
                return Err(CoordinatorError::Invalid(
                    "tool returned more than 8 MiB; output was not retained".into(),
                ));
            }
            let artifact = self.write_artifact(actor, Some(id), "json", &full)?;
            if json_large && !retain_structured {
                output.structured_json = Some(
                    json!({"is_error":output.is_error(), "truncated":true, "artifact":artifact.path}),
                );
            }
            let footer = if retain_structured && end < output.display_text.len() {
                output
                    .display_text
                    .rsplit_once('\n')
                    .map(|(_, line)| line)
                    .filter(|line| line.len() <= 1024)
                    .map(str::to_owned)
            } else {
                None
            };
            let end = output.display_text.floor_char_boundary(
                end.saturating_sub(footer.as_ref().map_or(0, |line| line.len() + 1)),
            );
            output.display_text.truncate(end);
            if let Some(footer) = footer {
                output.display_text.push('\n');
                output.display_text.push_str(&footer);
            }
            output.display_text.push_str(&format!(
                "\n[Output shortened. Retained output: {}]",
                artifact.path
            ));
            output.artifacts.push(artifact);
        }
        output
            .artifacts
            .extend(self.persist_attachments(actor, Some(id), &output.attachments)?);
        Ok(output)
    }

    pub fn write_artifact(
        &mut self,
        actor: &EventActor,
        tool: Option<&str>,
        extension: &str,
        text: &str,
    ) -> Result<ArtifactRef, CoordinatorError> {
        if !matches!(extension, "json" | "txt" | "diff") || text.len() > 8 * 1024 * 1024 {
            return Err(CoordinatorError::Invalid(
                "invalid artifact format or size".into(),
            ));
        }
        let safe = if extension == "json" {
            let mut value = serde_json::from_str(text)?;
            crate::redact::redact_in_place(self.redactor.as_ref(), &mut value);
            serde_json::to_string(&value)?
        } else {
            self.redactor.redact_text(text)
        };
        self.write_blob(actor, tool, extension, safe.as_bytes(), BTreeMap::new())
    }
    pub fn write_blob(
        &mut self,
        actor: &EventActor,
        tool: Option<&str>,
        extension: &str,
        bytes: &[u8],
        metadata: BTreeMap<String, String>,
    ) -> Result<ArtifactRef, CoordinatorError> {
        let digest = blake3::hash(bytes).to_hex().to_string();
        let path = format!("artifacts/{digest}.{extension}");
        crate::store::create_private_dir(&self.info()?.artifacts_dir)?;
        crate::store::write_private_atomic(&self.info()?.run_dir.join(&path), bytes)?;
        self.emit(
            actor.clone(),
            tool.map(str::to_owned),
            EventV1::ArtifactWritten(ArtifactWrittenEvent {
                path: path.clone(),
                digest: digest.clone(),
                bytes: bytes.len() as u64,
                tool_call_id: tool.map(Into::into),
                tool_metadata: None,
                metadata,
            }),
        )?;
        Ok(ArtifactRef { path, digest })
    }
}

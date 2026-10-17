use base64::{engine::general_purpose::STANDARD, Engine as _};
use harness_core::{
    event::RuntimeEvent,
    perm::{PermissionDecision, PermissionGrantScope},
};
use harness_tui::{
    app::{InterruptReason, LaunchMetadata, UiIntent},
    attachment_lifecycle::MimeKind,
    composer_atoms::AttachmentId,
    composer_integration::SubmissionAttachment,
    CompactionCommandKind, CompactionCommandOutcome, LiveUpdate, OperatorNoticeLevel,
};
use serde::Deserialize;
use serde_json::{json, Value};

fn field<'a, T: Deserialize<'a>>(value: &'a Value, name: &str) -> Result<T, String> {
    T::deserialize(value.get(name).ok_or_else(|| format!("missing {name}"))?)
        .map_err(|error| format!("invalid {name}: {error}"))
}

fn optional<'a, T: Deserialize<'a>>(value: &'a Value, name: &str) -> Result<Option<T>, String> {
    value
        .get(name)
        .map(|value| T::deserialize(value).map_err(|error| format!("invalid {name}: {error}")))
        .transpose()
}

fn attachments(value: &Value) -> Result<Vec<SubmissionAttachment>, String> {
    let Some(values) = value.get("attachments") else {
        return Ok(Vec::new());
    };
    let values = values.as_array().ok_or("attachments must be an array")?;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let mime = match field::<&str>(value, "mime")? {
                "image/png" => MimeKind::Png,
                "image/jpeg" => MimeKind::Jpeg,
                "text/plain" => MimeKind::PlainText,
                "application/octet-stream" => MimeKind::Binary,
                mime => return Err(format!("unsupported attachment MIME type: {mime}")),
            };
            let bytes = STANDARD
                .decode(field::<&str>(value, "dataBase64")?)
                .map_err(|error| format!("invalid attachment dataBase64: {error}"))?;
            let id = u64::try_from(index).map_err(|error| error.to_string())?;
            Ok(SubmissionAttachment {
                id: AttachmentId::new(id),
                mime,
                bytes,
            })
        })
        .collect()
}

pub(super) fn selection(
    value: &Value,
    metadata: &LaunchMetadata,
) -> Result<LaunchMetadata, String> {
    let profile: &str = field(value, "profile")?;
    let model: Option<&str> = optional(value, "model")?;
    let variant: Option<&str> = optional(value, "variant")?;
    let provider: Option<&str> = optional(value, "provider")?;
    let option = metadata
        .available_models()
        .iter()
        .find(|option| {
            option.profile == profile
                && model.is_none_or(|model| option.model == model)
                && provider.is_none_or(|provider| option.provider == provider)
                && option.variant() == variant
        })
        .ok_or_else(|| format!("model selection is unavailable for profile {profile}"))?;
    Ok(crate::live::model_selection::launch_metadata_from_model_option(metadata, option))
}

pub(super) fn intent(value: &Value, metadata: &LaunchMetadata) -> Result<UiIntent, String> {
    Ok(match field::<&str>(value, "kind")? {
        kind @ ("submit" | "followUp") => {
            let text = field(value, "text")?;
            let selected_file_tags = optional(value, "fileTags")?.unwrap_or_default();
            let selected_agent_tags = optional(value, "agentTags")?.unwrap_or_default();
            let selected_resource_tags = optional(value, "resourceTags")?.unwrap_or_default();
            let attachments = attachments(value)?;
            let launch_metadata = metadata.clone();
            if kind == "submit" {
                UiIntent::SubmitPrompt {
                    text,
                    selected_file_tags,
                    selected_agent_tags,
                    selected_resource_tags,
                    attachments,
                    launch_metadata,
                }
            } else {
                UiIntent::FollowUpPrompt {
                    text,
                    selected_file_tags,
                    selected_agent_tags,
                    selected_resource_tags,
                    attachments,
                    launch_metadata,
                }
            }
        }
        "steer" => UiIntent::SteerPrompt {
            text: field(value, "text")?,
            attachments: attachments(value)?,
            launch_metadata: metadata.clone(),
        },
        "queued" => {
            let id = field(value, "id")?;
            let prompt = value.get("prompt").ok_or("missing prompt")?;
            // The live handler unwraps one queue receipt, then dispatches a prompt only.
            if !matches!(
                field::<&str>(prompt, "kind")?,
                "submit" | "followUp" | "steer"
            ) {
                return Err("queued prompt must be submit, followUp, or steer".to_owned());
            }
            UiIntent::QueuedPrompt {
                id,
                prompt: Box::new(intent(prompt, metadata)?),
            }
        }
        "withdrawQueued" => UiIntent::WithdrawQueuedInput,
        "withdrawAccepted" => UiIntent::WithdrawAcceptedInput,
        "interrupt" => UiIntent::InterruptSession {
            task_ids: field(value, "taskIds")?,
            reason: match field::<&str>(value, "reason")? {
                "user" => InterruptReason::User,
                "replace" => InterruptReason::Replace,
                "task" => InterruptReason::Task,
                "sendNow" => InterruptReason::SendNow,
                reason => return Err(format!("invalid interrupt reason: {reason}")),
            },
        },
        "resolvePermission" => UiIntent::ResolvePermission {
            permission_id: field(value, "permissionId")?,
            decision: match field::<&str>(value, "decision")? {
                "allow" => PermissionDecision::Allow,
                "deny" => PermissionDecision::Deny,
                decision => return Err(format!("invalid permission decision: {decision}")),
            },
            reason: optional(value, "reason")?,
            grant_scope: match optional::<&str>(value, "grantScope")? {
                None | Some("once") => None,
                Some("session") => Some(PermissionGrantScope::Session),
                Some(scope) => return Err(format!("invalid permission grant scope: {scope}")),
            },
        },
        "setYolo" => UiIntent::SetYoloMode {
            enabled: field(value, "enabled")?,
        },
        "switchModel" => {
            let launch_metadata =
                selection(value.get("selection").ok_or("missing selection")?, metadata)?;
            UiIntent::SwitchModel {
                profile: launch_metadata.profile().to_owned(),
                launch_metadata,
            }
        }
        "compact" => UiIntent::CompactSession {
            instructions: optional(value, "instructions")?,
            mode: None,
        },
        "handoff" => UiIntent::Handoff {
            focus: optional(value, "focus")?,
        },
        "shake" => UiIntent::Shake {
            mode: harness_compaction::method::parse_shake_args(
                optional::<&str>(value, "args")?.unwrap_or_default(),
            )?,
        },
        "cancelCompaction" => UiIntent::CancelCompaction {
            agent_id: field(value, "agentId")?,
        },
        "backgroundForeground" => UiIntent::BackgroundForegroundSubagents,
        "demoteChild" => UiIntent::DemoteForegroundChildTask {
            handle_id: field(value, "handleId")?,
        },
        "cancelSubagent" => UiIntent::CancelSubagent {
            session_id: field(value, "sessionId")?,
        },
        "inspectCommand" => UiIntent::InspectCommand {
            task_id: optional(value, "taskId")?,
        },
        "loadRewindPoints" => UiIntent::LoadRewindPoints {
            generation: field(value, "generation")?,
            cancel_task_ids: field(value, "cancelTaskIds")?,
        },
        "rewind" => UiIntent::RewindConversation {
            generation: field(value, "generation")?,
            request_id: field(value, "requestId")?,
        },
        "revertWorkspace" => UiIntent::RevertWorkspace {
            snapshot_request_id: field(value, "snapshotRequestId")?,
        },
        "updateTitle" => UiIntent::UpdateSessionTitle {
            title: field(value, "title")?,
        },
        "runShell" => UiIntent::RunShellCommand {
            command: field(value, "command")?,
        },
        // These require coordinator/run context and are handled by session.intent.
        "setAccessMode" | "fork" | "clone" => {
            return Err("intent requires live session context".to_owned());
        }
        kind => return Err(format!("unknown intent kind: {kind}")),
    })
}

fn level(value: OperatorNoticeLevel) -> &'static str {
    match value {
        OperatorNoticeLevel::Info => "info",
        OperatorNoticeLevel::Error => "error",
    }
}

pub(super) fn update(value: LiveUpdate) -> Value {
    match value {
        LiveUpdate::Event(event) => match *event {
            RuntimeEvent::Durable(event) => {
                json!({ "kind": "event", "delivery": "durable", "event": event })
            }
            RuntimeEvent::Live(event) => {
                json!({ "kind": "event", "delivery": "live", "event": event })
            }
        },
        LiveUpdate::Status(text) => json!({ "kind": "status", "text": text }),
        LiveUpdate::ModelPromptNotice(message) => {
            json!({ "kind": "modelNotice", "message": message })
        }
        LiveUpdate::OperatorNotice {
            message,
            level: notice_level,
        } => {
            json!({ "kind": "notice", "level": level(notice_level), "message": message })
        }
        LiveUpdate::YoloModeChanged { enabled } => json!({ "kind": "yolo", "enabled": enabled }),
        LiveUpdate::YoloModeChangeFailed => json!({ "kind": "yoloFailed" }),
        LiveUpdate::CompactionCommand { kind, outcome } => {
            let command = match kind {
                CompactionCommandKind::Compact => "compact",
                CompactionCommandKind::Handoff => "handoff",
                CompactionCommandKind::Shake => "shake",
                CompactionCommandKind::Idle => "idle",
            };
            let mut result = json!({ "kind": "compaction", "command": command });
            match outcome {
                CompactionCommandOutcome::Done {
                    message,
                    tokens_after,
                    saved_path,
                    ..
                } => {
                    result["outcome"] = json!("done");
                    if let Some(message) = message {
                        result["message"] = json!(message);
                    }
                    if let Some(tokens) = tokens_after {
                        result["tokensAfter"] = json!(tokens);
                    }
                    if let Some(path) = saved_path {
                        result["savedPath"] = json!(path);
                    }
                }
                CompactionCommandOutcome::Warning(message) => {
                    result["outcome"] = json!("warning");
                    result["message"] = json!(message);
                }
                CompactionCommandOutcome::Cancelled => result["outcome"] = json!("cancelled"),
                CompactionCommandOutcome::Failed(message) => {
                    result["outcome"] = json!("failed");
                    result["message"] = json!(message);
                }
            }
            result
        }
        LiveUpdate::CommandOutput(snapshot) => {
            json!({ "kind": "commandOutput", "snapshot": snapshot })
        }
        LiveUpdate::RewindPoints { generation, result } => match result {
            Ok(points) => {
                json!({ "kind": "rewindPoints", "generation": generation, "points": points })
            }
            Err(error) => {
                json!({ "kind": "rewindPoints", "generation": generation, "error": error })
            }
        },
        LiveUpdate::RewindComplete { generation, result } => match result {
            Ok(point) => {
                json!({ "kind": "rewindComplete", "generation": generation, "point": point })
            }
            Err(error) => {
                json!({ "kind": "rewindComplete", "generation": generation, "error": error })
            }
        },
        LiveUpdate::RestoreQueuedInput(texts) => json!({ "kind": "restoreQueued", "texts": texts }),
        LiveUpdate::QueuedPromptReceipt { id, result } => match result {
            Ok(()) => json!({ "kind": "queuedReceipt", "id": id }),
            Err(error) => json!({ "kind": "queuedReceipt", "id": id, "error": error }),
        },
        LiveUpdate::ContinueSession {
            run_id,
            run_dir,
            prompt_draft,
        } => {
            json!({ "kind": "forked", "runId": run_id, "runDir": run_dir, "draft": prompt_draft })
        }
        LiveUpdate::AuthBackendResult {
            success, message, ..
        } => {
            json!({ "kind": "notice", "level": if success { "info" } else { "error" }, "message": message })
        }
        LiveUpdate::AuthBackendNotice {
            message,
            level: notice_level,
            ..
        } => {
            json!({ "kind": "notice", "level": level(notice_level), "message": message })
        }
        // GUI catalogs are requested separately, rather than embedded in session updates.
        LiveUpdate::SessionHistory(_) => {
            json!({ "kind": "status", "text": "Session history refreshed" })
        }
        LiveUpdate::AuthProviderCatalogRefreshed { .. } => {
            json!({ "kind": "status", "text": "Provider catalog refreshed" })
        }
    }
}

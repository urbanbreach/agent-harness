//! Sent-stream digests and the session config fingerprint.
use crate::anthropic_subscription::{
    options::TokenInjection,
    prompt::{hex, Content, LaneContext, LaneMessage},
    protocol::{QueryOptions, SystemPrompt, Thinking},
    tools::HOST_TOOL_POLICY_FINGERPRINT,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// sha256 hex of the key-sorted JSON (`stableValue` + `JSON.stringify`).
pub fn digest(value: &Value) -> String {
    hex(&Sha256::digest(value.to_string().as_bytes()))
}

/// A user message with no content blocks is transient and never hash-significant.
fn is_contentless_user(message: &LaneMessage) -> bool {
    matches!(message, LaneMessage::User { content: Content::Blocks(blocks) } if blocks.is_empty())
}

/// The one selection rule for "message the provider was sent".
pub fn is_transmitted(message: &LaneMessage) -> bool {
    matches!(
        message,
        LaneMessage::User { .. } | LaneMessage::ToolResult { .. }
    ) && !is_contentless_user(message)
}

pub fn sent_messages(context: &LaneContext) -> Vec<&LaneMessage> {
    context
        .messages
        .iter()
        .filter(|m| is_transmitted(m))
        .collect()
}

pub fn sent_message_hashes(messages: &[&LaneMessage]) -> Vec<String> {
    messages
        .iter()
        .filter(|m| is_transmitted(m))
        .map(|message| match message {
            LaneMessage::User { content } => {
                digest(&json!({"role": "user", "content": content.to_value()}))
            }
            LaneMessage::ToolResult {
                tool_call_id,
                tool_name,
                content,
                ..
            } => digest(&json!({
                "role": "toolResult",
                "toolCallId": tool_call_id,
                "toolName": tool_name,
                "content": content.to_value(),
            })),
            LaneMessage::Assistant { .. } => String::new(),
        })
        .collect()
}

pub fn sent_hash_prefix_digest(hashes: &[String], count: usize) -> String {
    digest(&json!(hashes[..count.min(hashes.len())]))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfigFingerprint {
    pub system_prompt_hash: String,
    pub toolset_hash: String,
}

pub fn system_prompt_value(prompt: &SystemPrompt) -> Value {
    match prompt {
        SystemPrompt::Custom(text) => json!(text),
        SystemPrompt::Preset { append } => {
            let mut value = json!({"type": "preset", "preset": "claude_code"});
            if let Some(append) = append {
                value["append"] = json!(append);
            }
            value
        }
    }
}

pub fn config_fingerprint(
    options: &QueryOptions,
    context: &LaneContext,
    auth_lane: TokenInjection,
    account_name: &str,
) -> SessionConfigFingerprint {
    let (thinking, max_thinking_tokens) = match &options.thinking {
        Some(Thinking::Adaptive { display }) => {
            (json!({"type": "adaptive", "display": display}), Value::Null)
        }
        Some(Thinking::Budget(tokens)) => (Value::Null, json!(tokens)),
        None => (Value::Null, Value::Null),
    };
    let mut reasoning = serde_json::Map::new();
    for (key, value) in [
        ("thinking", thinking),
        (
            "effort",
            options.effort.as_ref().map_or(Value::Null, |e| json!(e)),
        ),
        ("maxThinkingTokens", max_thinking_tokens),
    ] {
        if !value.is_null() {
            reasoning.insert(key.into(), value);
        }
    }
    let extra_args: serde_json::Map<String, Value> = options
        .extra_args
        .iter()
        .map(|(k, v)| (k.clone(), v.as_ref().map_or(Value::Null, |v| json!(v))))
        .collect();
    SessionConfigFingerprint {
        system_prompt_hash: digest(&system_prompt_value(&options.system_prompt)),
        toolset_hash: digest(&json!({
            "tools": options.tools,
            "reasoning": reasoning,
            "contextTools": context.tools.iter().flatten().map(|tool| json!({
                "name": tool.function_name,
                "description": tool.description,
                "parameters": tool.parameters,
            })).collect::<Vec<_>>(),
            "cwd": options.cwd,
            "authLane": auth_lane.as_str(),
            "accountName": account_name,
            "permissionMode": options.permission_mode,
            "hostToolPolicy": HOST_TOOL_POLICY_FINGERPRINT,
            "settingSources": options.setting_sources,
            "extraArgs": extra_args,
            "pathToClaudeCodeExecutable": options.executable,
            "includePartialMessages": options.include_partial_messages,
        })),
    }
}

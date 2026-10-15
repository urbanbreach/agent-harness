//! Claude Pro/Max subscription lane over the real Claude Code binary.
//!
//! Turns run through the real Claude Code binary over its
//! stream-json control protocol, as `@anthropic-ai/claude-agent-sdk` does.
//!
//! Claude Code plans; the harness executes. Every tool call Claude Code makes is denied on its
//! side (permission prompt, `PreToolUse` hook, and the `custom-tools` MCP handler) and streamed
//! back as a harness tool call; the result returns as the next user message. Main turns keep
//! one resident Claude Code session per harness session and send only the new messages
//! (delta), re-attaching or forking that lineage when it diverges and re-sending the history
//! (cold seed) only when nothing safer exists. Managed accounts fail over before any visible
//! output.
pub mod accounts;
pub mod auth_lane;
pub mod cold_seed;
pub mod errors;
pub mod executable;
pub mod limits;
pub mod options;
pub mod prompt;
pub mod protocol;
mod provider;
pub mod session;
pub mod store;
pub mod stream_events;
pub mod tools;
pub mod transcript;

pub use accounts::{AccountPool, AccountSlot, AccountSource, ModelBlock, ModelBlocks, SlotState};
pub use options::{AnthropicSubscriptionSettings, ResumeMode, SystemPromptMode, TokenInjection};
pub use provider::AnthropicSubscriptionProvider;
pub use store::{AccountStoreError, SubscriptionAccountStore};

pub const PROVIDER_ID: &str = "anthropic-subscription";
/// Wire identity, frozen at the lane's original name.
pub const CLAUDE_SDK_OAUTH_API_ID: &str = "claude-sdk-oauth";

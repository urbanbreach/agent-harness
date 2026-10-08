//! Runtime authority, durable sessions, and the contracts consumed by the TUI.
pub mod agent;
pub mod attachment_transport;
pub mod auth;
pub mod auto_fallback;
pub mod browser_oidc;
pub mod clock;
pub mod code_graph;
pub mod config;
pub mod context_budget;
pub mod conversation_rewind;
pub mod coord;
pub mod cow_worktree;
pub mod crash_recovery;
#[cfg(test)]
#[path = "crash_recovery/tests.rs"]
mod crash_tests;
pub mod edit_attribution;
pub mod event;
pub mod file_tag;
pub mod foreground_demote;
pub mod foreign_session;
pub mod ids;
pub mod integrations;
pub mod jujutsu;
#[cfg(test)]
#[path = "session_lineage/tests.rs"]
mod lineage_tests;
pub mod mcp_oauth;
pub mod memory;
pub mod model_resolution;
pub mod perm;
pub mod plan;
pub mod process;
pub mod proj;
pub mod provider_catalog;
pub mod redact;
pub mod sandbox;
pub mod session;
pub mod session_lineage;
pub mod session_title;
pub mod sleep_wake_auth;
pub mod storage_paths;
pub mod store;
pub mod subagent;
pub mod system_prompt;
pub mod tool;
pub mod transcript_projection;
pub mod workspace;
pub mod workspace_hub;
pub mod worktree;
pub use context_budget::{
    compute_request_budget, BudgetStatus, RequestBudget, RequestBudgetComponents,
    RequestBudgetError, RequestBudgetInput, RequestBudgetSnapshot,
};
pub use harness_providers::UnwrapOrAbort;

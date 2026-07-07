use harness_core::config::PermissionMode;
use harness_core::event::{ActorKind, EventActor};
use harness_core::perm::PermissionPolicy;
use harness_core::tool::ToolError;

mod event_log;
mod event_reader;
mod mcp_server;
mod question_events;
pub(crate) mod remote_search_env;
mod repo_root;
mod single_surface_live;
mod tool_context;
mod workspace;

pub(crate) use event_log::{
    find_finished, read_events, wait_for_request_terminal, wait_for_succeeded_tool_call_finish,
    wait_for_tool_call_finish,
};
pub(crate) use mcp_server::{
    install_fake_mcp_server, install_fake_mcp_server_with_tools,
    install_stateful_terminal_mcp_server,
};
pub(crate) use question_events::wait_for_question_permission;
pub(crate) use repo_root::repo_root;
pub(crate) use single_surface_live::{SingleSurfaceShellRunner, SingleSurfaceWebFetchTransport};
pub use tool_context::{test_context, test_context_with_tool_state};
pub use workspace::{setup_workspace, setup_workspace_fixture};

pub fn worker_actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}

pub fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()))
}

pub fn anonymous_supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, None)
}

pub fn allow_all_permission_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    )
}

pub fn edit_only_permission_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Deny,
    )
}

pub fn ask_edit_permission_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Ask,
        PermissionMode::Deny,
        PermissionMode::Deny,
    )
    .with_ask_timeout_ms(1_000)
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
pub fn expect_invalid_arguments(error: ToolError, expected: &str) {
    match error {
        ToolError::InvalidArguments(message) => assert!(
            message.contains(expected),
            "expected invalid-arguments error containing {expected:?}, got {message:?}"
        ),
        _ => panic!("abort"),
    }
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
pub fn expect_execution_error(error: ToolError, expected: &str) {
    match error {
        ToolError::Execution(message) => assert!(
            message.contains(expected),
            "expected execution error containing {expected:?}, got {message:?}"
        ),
        _ => panic!("abort"),
    }
}

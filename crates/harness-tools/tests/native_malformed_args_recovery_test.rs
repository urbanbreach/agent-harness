mod common;

use common::{setup_workspace_fixture, test_context};
use harness_core::config::ShellAllowlist;
use harness_tools::coordinator_registry;
use harness_tools::UnwrapOrAbort;
use serde_json::json;

#[tokio::test]
async fn malformed_native_args_return_actionable_recovery_message() {
    // arrange
    let workspace = setup_workspace_fixture();
    let context = test_context(
        workspace.workspace(),
        "run-malformed-args-recovery",
        "toolcall-malformed-read",
    );
    let registry = coordinator_registry(ShellAllowlist::default());

    // act
    let error = registry
        .get("read")
        .unwrap_or_abort()
        .call(
            context,
            json!({
                "filePath": 123
            }),
        )
        .await
        .expect_err("malformed read args should fail");
    let message = error.to_string();

    // assert
    assert!(
        message.contains("The tool call arguments are invalid."),
        "missing invalid-arguments summary: {message}"
    );
    assert!(
        message.contains("Rewrite the JSON arguments to match this tool's schema."),
        "missing recovery instruction: {message}"
    );
    assert!(
        message.contains("invalid type: integer `123`, expected a string"),
        "missing serde parse detail: {message}"
    );
}

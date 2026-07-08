use super::{
    build_recursive_tree, AgentOpsExecutor, BackgroundOutputTool, BatchArgs, ControlPlaneExecutor,
    QuestionArgs, SkillArgs, SkillTool, TaskArgs, TaskTool,
};
use crate::UnwrapOrAbort;
use std::sync::Arc;

use harness_core::clock::RealClock;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::{ActorKind, EventActor};
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{Tool, ToolContext, ToolRunState};
use serde_json::{json, Value};

#[tokio::test]
async fn recursive_tree_renders_direct_children_once_in_sorted_order() {
    let tempdir = tempfile::tempdir().unwrap_or_abort();
    let root = tempdir.path();
    std::fs::create_dir_all(root.join("src/zeta")).unwrap_or_abort();
    std::fs::create_dir_all(root.join("src/alpha")).unwrap_or_abort();
    std::fs::write(root.join("src/zeta/mod.rs"), "").unwrap_or_abort();
    std::fs::write(root.join("src/alpha/lib.rs"), "").unwrap_or_abort();
    std::fs::write(root.join("README.md"), "").unwrap_or_abort();

    let coordinator = spawn_coordinator(
        CoordinatorConfig::default(),
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let ctx = ToolContext {
        run_id: "run-tree-tests".into(),
        workspace_root: root.to_path_buf(),
        artifacts_dir: root.join("artifacts"),
        actor: EventActor::new(ActorKind::Worker, Some("worker-1".to_string())),
        category: Some("quick".to_string()),
        tool_call_id: "tree-test".into(),
        current_model_ref: None,
        current_model_settings: None,
        tool_state: ToolRunState::default(),
        coordinator,
    };

    let tree = build_recursive_tree(&ctx, root, &[], 100).unwrap_or_abort();

    assert_eq!(tree.count, 3);
    assert!(!tree.truncated);
    assert_eq!(
        tree.rendered,
        format!(
            "{}/\n  src/\n    alpha/\n      lib.rs\n    zeta/\n      mod.rs\n  README.md",
            root.display()
        )
    );
}

#[test]
fn question_args_accept_top_level_array_and_prompt_aliases() {
    let args: QuestionArgs = serde_json::from_value(json!([
        {
            "prompt": "Choose a mode",
            "title": "Mode",
            "choices": ["fast", "thorough"],
            "allowMultiple": true
        }
    ]))
    .unwrap_or_abort();

    assert_eq!(args.questions.len(), 1);
    assert_eq!(args.questions[0].question, "Choose a mode");
    assert_eq!(args.questions[0].header, "Mode");
    assert_eq!(args.questions[0].options[0].label, "fast");
    assert_eq!(args.questions[0].multiple, Some(true));
}

#[test]
fn question_args_accept_allow_freeform_legacy_field() {
    let args: QuestionArgs = serde_json::from_value(json!({
        "questions": [
            {
                "question": "Choose a mode",
                "options": ["fast", "thorough"],
                "allowFreeform": false
            }
        ]
    }))
    .unwrap_or_abort();

    assert_eq!(args.questions.len(), 1);
    assert_eq!(args.questions[0].question, "Choose a mode");
    assert_eq!(args.questions[0].options[1].label, "thorough");
}

#[test]
fn skill_args_match_harness_name_only_schema() {
    let args: SkillArgs = serde_json::from_value(json!({
        "name": "git-master"
    }))
    .unwrap_or_abort();

    assert_eq!(args.name, "git-master");

    let err = serde_json::from_value::<SkillArgs>(json!({
        "name": "git-master",
        "user_message": "extra context"
    }))
    .expect_err("skill args should reject non-Harness user_message field");
    assert!(err.to_string().contains("unknown field `user_message`"));

    let skill = SkillTool::new(Arc::new(ControlPlaneExecutor::new()));
    let schema = skill.parameters_json_schema();
    assert_eq!(schema["required"], json!(["name"]));
    assert_eq!(schema["additionalProperties"], json!(false));
    assert!(schema["properties"].get("name").is_some());
    assert!(schema["properties"].get("user_message").is_none());
}

#[test]
fn task_args_accept_agent_alias_fields() {
    let args: TaskArgs = serde_json::from_value(json!({
        "description": "Explore codebase",
        "prompt": "Find auth flow",
        "agent": "explorer",
        "background": true,
        "load_skills": []
    }))
    .unwrap_or_abort();

    assert_eq!(args.subagent_type.as_deref(), Some("explorer"));
    assert!(args.run_in_background);
}

#[test]
fn task_args_accept_skills_alias_for_load_skills() {
    let args: TaskArgs = serde_json::from_value(json!({
        "description": "Explore codebase",
        "prompt": "Find auth flow",
        "category": "explore",
        "run_in_background": false,
        "skills": ["rust-best-practices"]
    }))
    .unwrap_or_abort();

    assert_eq!(args.load_skills, vec!["rust-best-practices".to_string()]);
}

#[test]
fn task_args_require_explicit_background_and_skills_fields() {
    let missing_background = serde_json::from_value::<TaskArgs>(json!({
        "description": "Explore codebase",
        "prompt": "Find auth flow",
        "category": "explore",
        "load_skills": []
    }))
    .expect_err("task args should require run_in_background");
    assert!(missing_background
        .to_string()
        .contains("missing required field `run_in_background`"));

    let missing_skills = serde_json::from_value::<TaskArgs>(json!({
        "description": "Explore codebase",
        "prompt": "Find auth flow",
        "category": "explore",
        "run_in_background": true
    }))
    .expect_err("task args should require load_skills");
    assert!(missing_skills
        .to_string()
        .contains("missing required field `load_skills`"));
}

#[test]
fn task_and_background_output_descriptions_prefer_completion_notification() {
    let executor = Arc::new(AgentOpsExecutor::new());
    let task = TaskTool::new(Arc::clone(&executor));
    let background_output = BackgroundOutputTool::new(executor);

    let task_description = task.description();
    assert!(task_description.contains("`run_in_background` is required"));
    assert!(task_description.contains("`load_skills` is required"));
    assert!(task_description.contains("run_in_background=true"));
    assert!(task_description.contains("returns task_id/request_id immediately"));
    assert!(
        task_description.contains("sync child tasks do not emit background wakeup notifications")
    );
    assert!(task_description.contains("testing or exercising background scheduling"));
    assert!(task_description.contains("injected into the child prompt"));
    assert!(task_description.contains("background_output"));
    assert!(task_description.contains("completion notification"));
    assert!(task_description.contains("wait for the coordinator"));
    assert!(task_description.contains("interim status checks"));
    assert!(task_description.contains("cancellation"));
    assert!(task_description.contains("final result"));
    assert!(
        !task_description.contains("do not wait for that reminder"),
        "task description must not keep the old do-not-wait guidance"
    );
    for field in [
        "context",
        "goal",
        "downstream use",
        "request",
        "required tools",
        "must-do",
        "must-not-do",
    ] {
        assert!(
            task_description.contains(field),
            "task description should document structured delegation field {field:?}"
        );
    }

    let task_schema = task.parameters_json_schema();
    let prompt_description = task_schema
        .pointer("/properties/prompt/description")
        .and_then(Value::as_str)
        .unwrap_or_abort();
    for field in [
        "context",
        "goal",
        "downstream use",
        "request",
        "required tools",
        "must-do",
        "must-not-do",
    ] {
        assert!(
            prompt_description.contains(field),
            "task prompt schema should document structured delegation field {field:?}"
        );
    }
    let run_in_background_description = task_schema
        .pointer("/properties/run_in_background/description")
        .and_then(Value::as_str)
        .unwrap_or_abort();
    assert!(run_in_background_description.contains("interim status checks"));
    assert!(run_in_background_description.contains("cancel=true anytime"));
    assert!(run_in_background_description.contains("completion notification"));
    assert!(run_in_background_description.contains("final result retrieval"));

    let background_output_description = background_output.description();
    assert!(background_output_description.contains("completion notification"));
    assert!(background_output_description.contains("interim status checks"));
    assert!(background_output_description.contains("final result"));
    assert!(background_output_description.contains("cancel=true"));
    assert!(
        !background_output_description.contains("do not replace explicit retrieval"),
        "background_output description must not keep the old notifications-only guidance"
    );

    let background_output_schema = background_output.parameters_json_schema();
    let request_id_description = background_output_schema
        .pointer("/properties/request_id/description")
        .and_then(Value::as_str)
        .unwrap_or_abort();
    assert!(request_id_description.contains("interim status checks"));
    assert!(request_id_description.contains("completion notification"));
    let block_description = background_output_schema
        .pointer("/properties/block/description")
        .and_then(Value::as_str)
        .unwrap_or_abort();
    assert!(block_description.contains("interim status checks"));
    assert!(block_description.contains("completion notification"));
    let cancel_description = background_output_schema
        .pointer("/properties/cancel/description")
        .and_then(Value::as_str)
        .unwrap_or_abort();
    assert!(cancel_description.contains("cancel=true"));
    assert!(cancel_description.contains("allowed anytime"));
}

#[test]
fn batch_args_accept_parallel_wrapper_shape() {
    let args: BatchArgs = serde_json::from_value(json!({
        "tool_uses": [
            {
                "recipient_name": "functions.read",
                "parameters": {"filePath": "/tmp/demo.txt"}
            },
            {
                "recipient_name": "functions.bash",
                "arguments": {"command": "ls"}
            }
        ]
    }))
    .unwrap_or_abort();

    assert_eq!(args.tool_calls.len(), 2);
    assert_eq!(args.tool_calls[0].tool, "read");
    assert_eq!(args.tool_calls[1].tool, "bash");
    assert_eq!(args.tool_calls[1].parameters, json!({"command": "ls"}));
}

#[test]
fn batch_args_accept_wrapper_shape_inside_tool_calls() {
    let args: BatchArgs = serde_json::from_value(json!({
        "tool_calls": [
            {
                "recipient_name": "functions.read",
                "parameters": {"filePath": "Cargo.toml"}
            }
        ]
    }))
    .unwrap_or_abort();

    assert_eq!(args.tool_calls.len(), 1);
    assert_eq!(args.tool_calls[0].tool, "read");
    assert_eq!(
        args.tool_calls[0].parameters,
        json!({"filePath": "Cargo.toml"})
    );
}

#[test]
fn batch_args_accept_args_alias_inside_tool_calls() {
    let args: BatchArgs = serde_json::from_value(json!({
        "tool_calls": [
            {
                "tool": "read",
                "args": {"filePath": "Cargo.toml"}
            }
        ]
    }))
    .unwrap_or_abort();

    assert_eq!(args.tool_calls.len(), 1);
    assert_eq!(args.tool_calls[0].tool, "read");
    assert_eq!(
        args.tool_calls[0].parameters,
        json!({"filePath": "Cargo.toml"})
    );
}

pub(crate) fn mapping_status(tool_id: &str) -> &'static str {
    match tool_id {
        "apply_patch" | "codesearch" | "glob" | "invalid" | "question" | "skill" | "websearch" => {
            "parity_ready"
        }
        "bash" | "batch" | "edit" | "grep" | "read" | "task" | "todoread" | "todowrite"
        | "webfetch" | "write" => "harness_adapted",
        "ast_grep_replace"
        | "ast_grep_search"
        | "background_cancel"
        | "background_output"
        | "github.issue"
        | "github.pull_request"
        | "list"
        | "lsp"
        | "lsp.rename"
        | "plan_enter"
        | "plan_exit"
        | "session_info"
        | "session_list"
        | "session_read"
        | "session_search"
        | "shell.run" => "harness_only",
        _ => "harness_only",
    }
}

pub(crate) fn equivalent_id(tool_id: &str) -> Option<&'static str> {
    match tool_id {
        "bash" => Some("bash"),
        "apply_patch" => Some("apply_patch"),
        "batch" => Some("batch"),
        "codesearch" => Some("codesearch"),
        "edit" => Some("edit"),
        "glob" => Some("glob"),
        "grep" => Some("grep"),
        "invalid" => Some("invalid"),
        "question" => Some("question"),
        "read" => Some("read"),
        "skill" => Some("skill"),
        "task" => Some("task"),
        "todoread" | "todowrite" => Some("todo"),
        "webfetch" => Some("webfetch"),
        "websearch" => Some("websearch"),
        "write" => Some("write"),
        _ => None,
    }
}

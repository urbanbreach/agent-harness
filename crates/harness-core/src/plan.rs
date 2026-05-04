use std::path::{Path, PathBuf};

pub const BUILD_AGENT_NAME: &str = "build";
pub const PLAN_AGENT_NAME: &str = "plan";
pub const PLAN_EXIT_TOOL_ID: &str = "plan_exit";
pub const PLAN_DIR: &str = ".agent-harness/plans";

pub fn plan_file_relative_path(run_id: &str) -> PathBuf {
    Path::new(PLAN_DIR).join(format!("{}.md", sanitize_plan_slug(run_id)))
}

pub fn plan_file_display_path(run_id: &str) -> String {
    plan_file_relative_path(run_id)
        .to_string_lossy()
        .to_string()
}

fn sanitize_plan_slug(value: &str) -> String {
    let mut slug = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "plan".to_string()
    } else {
        slug.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_file_path_is_workspace_relative_and_sanitized() {
        assert_eq!(
            plan_file_display_path("run/with spaces"),
            ".agent-harness/plans/run-with-spaces.md"
        );
        assert_eq!(
            plan_file_display_path("---"),
            ".agent-harness/plans/plan.md"
        );
    }
}

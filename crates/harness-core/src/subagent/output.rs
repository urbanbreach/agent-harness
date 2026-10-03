use super::GetCommandOrSubagentOutputResult;

impl GetCommandOrSubagentOutputResult {
    /// Model-facing output shared by polling and completion reminders.
    pub fn to_prompt_text(&self) -> String {
        let result = self;
        let mut lines = vec![
            format!("=== Task {} ===", result.task_id),
            format!("Command: {}", result.command),
            format!("Status: {}", result.status),
            format!("Duration: {:.2}s", result.duration_secs),
        ];
        if let Some(code) = result.exit_code {
            lines.push(format!("Exit Code: {code}"));
        }
        if !result.output_file.is_empty() {
            lines.push(format!("Output File: {}", result.output_file));
        }
        lines.push(String::new());
        lines.push("=== Output ===".into());
        lines.push(if result.output.is_empty() {
            if result.status == "running" {
                "(no output yet)".into()
            } else {
                "(no output)".into()
            }
        } else {
            result.output.clone()
        });
        if result.truncated {
            lines.push(result.truncation_hint.clone());
        }
        lines.join("\n")
    }
}

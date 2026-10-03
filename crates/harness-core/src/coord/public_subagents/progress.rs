use super::*;

impl Runtime {
    pub(super) fn refresh_native_progress(&mut self, id: &str) {
        let (Some(child), Some(agent)) = (self.native_subagents.get_mut(id), self.agents.get(id))
        else {
            return;
        };
        if matches!(child.phase, NativePhase::Finalizing | NativePhase::Terminal) {
            return;
        }
        let elapsed = Duration::from_millis(self.clock.mono_ms().saturating_sub(child.started_ms))
            .as_secs_f64();
        let kind = &child.registration.subagent_type;
        let description = &child.registration.description;
        let output = if child.phase == NativePhase::Running {
            let tools = if agent.tools_used.is_empty() {
                "none yet".into()
            } else {
                agent.tools_used.join(", ")
            };
            let tokens = thousands(agent.native_context_tokens);
            let capacity = thousands(agent.native_context_window);
            let percentage = agent
                .native_context_tokens
                .zip(agent.native_context_window)
                .and_then(|(tokens, window)| tokens.saturating_mul(100).checked_div(window))
                .map_or_else(
                    || "unknown".into(),
                    |percent| format!("{}%", percent.min(100)),
                );
            format!("Subagent is still running.\nType: {kind}\nDescription: {description}\nElapsed: {elapsed:.1}s\nProgress: turn {}, {} tool calls, {tokens}/{capacity} tokens ({percentage} context)\nTools used: {tools}\nErrors: {}",
                agent.prompt_turns, agent.tool_calls, agent.error_count)
        } else {
            format!("Subagent is initializing (creating worktree, resolving config).\nType: {kind}\nDescription: {description}\nElapsed: {elapsed:.1}s")
        };
        child.updates.send_modify(|snapshot| {
            snapshot.result.duration_secs = elapsed;
            snapshot.result.raw_output_bytes = output.len();
            snapshot.result.output = output;
        });
    }
}

fn thousands(tokens: Option<u64>) -> String {
    tokens.map_or_else(|| "unknown".into(), |tokens| format!("{}K", tokens / 1000))
}

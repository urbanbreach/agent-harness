use super::*;
use crate::event::SubagentProgressEvent;
use tokio::time::Instant;

type Signature = (u32, u32, Option<u8>, u32, Option<u64>);

pub(super) struct Publisher {
    next: Instant,
    last_emit: Instant,
    signature: Option<Signature>,
}

impl Publisher {
    pub(super) fn new() -> Self {
        let now = Instant::now();
        Self {
            next: now + Duration::from_secs(2),
            last_emit: now,
            signature: None,
        }
    }
}

fn context_percentage(tokens: Option<u64>, window: Option<u64>) -> Option<u8> {
    tokens.zip(window).and_then(|(tokens, window)| {
        tokens
            .saturating_mul(100)
            .checked_div(window)
            .map(|pct| pct.min(100) as u8)
    })
}

impl Runtime {
    pub(in crate::coord) fn native_progress_deadline(&self) -> Option<Instant> {
        self.native_subagents
            .values()
            .filter(|child| {
                child.phase == NativePhase::Running && !child.cancellation.is_cancelled()
            })
            .filter_map(|child| child.progress.as_ref().map(|publisher| publisher.next))
            .min()
    }

    pub(in crate::coord) fn publish_native_progress(&mut self) {
        let now = Instant::now();
        let mut updates = Vec::new();
        for (id, child) in &mut self.native_subagents {
            if child.phase != NativePhase::Running || child.cancellation.is_cancelled() {
                continue;
            }
            let (Some(publisher), Some(agent), Some(attempt)) =
                (&mut child.progress, self.agents.get(id), &child.request)
            else {
                continue;
            };
            if publisher.next > now {
                continue;
            }
            publisher.next = now + Duration::from_secs(2);
            let percentage =
                context_percentage(agent.native_context_tokens, agent.native_context_window);
            let signature = (
                agent.prompt_turns,
                agent.tool_calls,
                percentage,
                agent.error_count,
                agent.native_context_tokens,
            );
            if publisher.signature == Some(signature)
                && now.duration_since(publisher.last_emit) < Duration::from_secs(8)
            {
                continue;
            }
            publisher.signature = Some(signature);
            publisher.last_emit = now;
            updates.push(SubagentProgressEvent {
                child_id: id.clone(),
                attempt_id: attempt.clone(),
                generation: agent.generation,
                duration_ms: self.clock.mono_ms().saturating_sub(child.started_ms),
                turn_count: agent.prompt_turns,
                tool_call_count: agent.tool_calls,
                tokens_used: agent.native_context_tokens,
                context_window_tokens: agent.native_context_window,
                context_usage_pct: percentage,
                tools_used: agent.tools_used.clone(),
                error_count: agent.error_count,
            });
        }
        for progress in updates {
            let _ = self.live(
                EventActor::new(ActorKind::Worker, Some(progress.child_id.clone())),
                progress.attempt_id.clone(),
                LiveEventV1::SubagentProgress(progress),
            );
        }
    }

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
            let percentage =
                context_percentage(agent.native_context_tokens, agent.native_context_window)
                    .map_or_else(|| "unknown".into(), |percent| format!("{percent}%"));
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

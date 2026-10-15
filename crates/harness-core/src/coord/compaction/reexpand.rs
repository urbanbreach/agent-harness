//! Recover readable journal history before replacing an incompatible opaque boundary.
use super::super::{context::Context, turn::Worker, *};
use harness_providers::NativeCompactionLane;

impl Worker {
    /// When the context starts at a provider-native replacement history that the next
    /// compaction cannot use, the journal's own history in its place; `None` keeps the context.
    ///
    /// A boundary is kept only while the current method is a native lane of the same provider
    /// and the opaque payload is attached (a stored payload is attached on restore only when it
    /// belongs to the live provider, model and protocol). Otherwise the journal is rebuilt:
    /// for a native method newest-to-oldest, taking the first boundary whose payload loads for
    /// the live request and else the previous readable one; for any other method the readable
    /// boundary and the originals it kept.
    pub(super) async fn reexpand_native(
        &self,
        context: &Context,
        reusable: bool,
    ) -> Result<Option<Context>, CoordinatorError> {
        let Some(boundary) = context.entries.get(context.body_start()).filter(|entry| {
            entry
                .compaction
                .as_ref()
                .and_then(|state| state.native.as_ref())
                .is_some_and(|native| native.lane.starts_with("openai_"))
        }) else {
            return Ok(None);
        };
        let live = self.request(context);
        let same_provider = boundary
            .compaction
            .as_ref()
            .and_then(|state| state.native.as_ref())
            .is_some_and(|native| native.provider_id == live.provider_id);
        let compatible = self
            .provider
            .native_compaction_lanes(&live, &self.native_lane_settings(&live))
            .iter()
            .any(|lane| {
                matches!(
                    lane,
                    NativeCompactionLane::OpenAiV1 | NativeCompactionLane::OpenAiV2
                )
            });
        let attached = boundary.message.continuation.is_some();
        if reusable && same_provider && compatible && attached {
            return Ok(None);
        }
        let agent = self
            .actor
            .agent_id
            .clone()
            .ok_or_else(|| CoordinatorError::Invalid("compaction needs an agent".into()))?;
        let request = (reusable && compatible).then_some(live);
        let mut restored = self.handle.call(move |runtime| {
            let info = runtime.info()?;
            let events = crate::store::read_events(&info.events_path)?;
            let primary = events.iter().any(|event| matches!(&event.payload,
                EventV1::AgentSpawned(spawn) if spawn.agent_id == agent && spawn.parent_agent_id.is_none()));
            super::super::history::rebuild(&events, &agent, primary, "", &info.run_dir, request.as_ref())
        }).await?;
        // The journal has settled records; keep the live tail, including in-flight tools.
        let tail = &context.entries[context.body_start() + 1..];
        if let Some(first) = tail.first() {
            if let Some(index) = restored.entries.iter().position(|entry| {
                entry.seq == first.seq
                    && entry.message.role == first.message.role
                    && entry.message.tool_call_id == first.message.tool_call_id
            }) {
                restored.entries.truncate(index);
            } else {
                restored.entries.retain(|entry| entry.seq <= boundary.seq);
            }
            restored.entries.extend_from_slice(tail);
        }
        restored.entries.splice(
            0..0,
            context.entries[..context.body_start()].iter().cloned(),
        );
        restored.model_request = None;
        Ok(Some(restored))
    }
}

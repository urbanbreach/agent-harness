use super::*;
use crate::proj::{RecordedRuntimeContext, RunMetadata};

impl Runtime {
    pub fn new_metadata(&self) -> Result<RunMetadata, CoordinatorError> {
        let run = self.info()?;
        Ok(RunMetadata {
            run_id: run.run_id.to_string(),
            run_name: run.run_name.to_string(),
            workspace_root: run.workspace_root.to_string_lossy().into(),
            created_at: self.clock.system_time_rfc3339_millis(),
            config_digest: self.config.config_digest.clone(),
            harness_version: self.config.harness_version.clone(),
            recorded_runtime_context: None,
            mode_source: self.config.session_mode_source,
        })
    }
    pub fn write_metadata(&mut self) -> Result<(), CoordinatorError> {
        let Some(metadata) = &self.metadata else {
            return Ok(());
        };
        let mut value = serde_json::to_value(metadata)?;
        if let Some(mut saved) = crate::proj::read_metadata_value(&self.info()?.run_dir)? {
            if let (Some(saved), Some(current)) = (saved.as_object_mut(), value.as_object_mut()) {
                saved.extend(std::mem::take(current));
            }
            value = saved;
        }
        crate::redact::redact_in_place(self.redactor.as_ref(), &mut value);
        if let Err(error) = crate::store::write_private_atomic(
            &self.info()?.run_dir.join(crate::proj::META_FILE_NAME),
            &serde_json::to_vec_pretty(&value)?,
        ) {
            let error = EventStoreError::Io(error);
            self.storage_failed(&error);
            return Err(error.into());
        }
        Ok(())
    }
    pub fn record_selection(&mut self, agent_id: &str) -> Result<(), CoordinatorError> {
        let agent = self
            .agents
            .get(agent_id)
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent_id.into()))?;
        let mut context = agent.target.as_ref().map_or_else(
            || {
                RecordedRuntimeContext::from_profile_model(
                    &agent.profile.name,
                    &agent.info.model_ref,
                )
            },
            |target| RecordedRuntimeContext::from_model_target(&agent.profile.name, target),
        );
        context.variant = agent.settings.variant.clone();
        context.reasoning_effort = agent.settings.reasoning_effort.clone();
        context.text_verbosity = agent.settings.text_verbosity.clone();
        context.thinking = agent.settings.thinking.clone();
        if agent.info.parent_agent_id.is_some() {
            return self
                .write_child_selection(agent_id, &context)
                .map_err(|error| {
                    self.storage_failed(&error);
                    error.into()
                });
        }
        if let Some(metadata) = &mut self.metadata {
            if metadata.recorded_runtime_context.as_ref() == Some(&context) {
                return Ok(());
            }
            metadata.recorded_runtime_context = Some(context);
        }
        self.write_metadata()
    }
}

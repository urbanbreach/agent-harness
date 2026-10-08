//! Subagent results held to the caller's output schema.
use super::*;

impl Worker {
    /// A correction when this subagent's final answer does not match its output schema.
    pub(super) async fn output_contract_reminder(
        &mut self,
        text: &str,
    ) -> Result<Option<String>, CoordinatorError> {
        if !self.native
            || self.guidance.output_attempts >= self.behavior.output_contract.max_retries
        {
            return Ok(None);
        }
        let Some(agent) = self.actor.agent_id.clone() else {
            return Ok(None);
        };
        let contract = self.handle.native_output_contract(agent).await?;
        let (_, errors) = crate::subagent::output_contract::validate(contract.as_deref(), text);
        if errors.is_empty() {
            return Ok(None);
        }
        self.guidance.output_attempts += 1;
        Ok(Some(format!(
            "Your final answer does not match the caller's output_schema. First problem:\n- {}\n\nReturn only the corrected JSON object matching the schema in your assignment, without prose or markdown fences.",
            errors.join("\n- ")
        )))
    }
}

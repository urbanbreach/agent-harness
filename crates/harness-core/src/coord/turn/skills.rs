use super::*;

impl Worker {
    pub(super) async fn preload_skills(&mut self) -> Result<(), CoordinatorError> {
        if self.skill_preloads.is_none() && !self.skill_preload_names.is_empty() {
            let loaded = self.load_startup_skills().await?;
            let owner = self.actor.agent_id.clone();
            let cached = loaded.clone();
            self.handle
                .call(move |runtime| {
                    if let Some(agent) = owner.and_then(|id| runtime.agents.get_mut(&id)) {
                        agent.skill_preloads = Some(cached);
                    }
                    Ok(())
                })
                .await?;
            self.skill_preloads = Some(loaded);
        }
        if let (Some(metadata), Some(loaded)) = (&mut self.skill_metadata, &self.skill_preloads) {
            let mut value: Value = serde_json::from_str(metadata)?;
            if let Some(entries) = value
                .get_mut("available_skills")
                .and_then(Value::as_array_mut)
            {
                entries.retain(|entry| !loaded.iter().any(|(name, _)| entry_matches(entry, name)));
            }
            *metadata = value.to_string();
        }
        Ok(())
    }

    async fn load_startup_skills(&self) -> Result<Vec<(String, String)>, CoordinatorError> {
        let catalog: Value = self
            .skill_metadata
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_default();
        let names = configured_names(&catalog, &self.skill_preload_names);
        let mut loaded = Vec::new();
        let mut total_bytes: usize = 0;
        for (index, name) in names.into_iter().enumerate() {
            // Explicit preloads use the ordinary skill tool's availability,
            // trust, permission and cancellation gates.
            let id = format!("{}-preload-{index}", self.turn.id);
            let result = self
                .handle
                .execute_tool(
                    self.actor.clone(),
                    Some(self.turn.id.clone()),
                    Some(id.clone()),
                    "skill".into(),
                    serde_json::json!({"name": name}),
                )
                .await;
            self.handle
                .call(move |runtime| {
                    runtime.raw_tool_results.remove(&id);
                    Ok(())
                })
                .await?;
            match result {
                Ok(output)
                    if !output.is_error()
                        && total_bytes.saturating_add(output.display_text.len()) <= 1_048_576 =>
                {
                    total_bytes += output.display_text.len();
                    loaded.push((name, output.display_text));
                }
                Err(
                    error @ (CoordinatorError::Cancelled(_)
                    | CoordinatorError::Stopping
                    | CoordinatorError::Closed),
                ) => return Err(error),
                // Missing, denied or unavailable optional skills do not stop
                // startup, and their bodies never reach the model.
                _ => {}
            }
        }
        Ok(loaded)
    }
}

fn entry_matches(entry: &Value, name: &str) -> bool {
    ["name", "stable_id"].iter().any(|key| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case(name))
    })
}

fn configured_names(catalog: &Value, configured: &[String]) -> Vec<String> {
    let Some(entries) = catalog.get("available_skills").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for name in configured {
        let found = entries
            .iter()
            .find(|entry| entry_matches(entry, name))
            .and_then(|entry| entry.get("name"))
            .and_then(Value::as_str);
        if let Some(name) = found.filter(|name| !names.iter().any(|loaded| loaded == name)) {
            names.push(name.to_owned());
        }
    }
    names
}

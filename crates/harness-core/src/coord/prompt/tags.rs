use super::*;

pub(super) fn validate(text: &str, tags: &SelectedPromptTags) -> Result<(), CoordinatorError> {
    let mut sources: Vec<_> = tags
        .files
        .iter()
        .map(|t| &t.source)
        .chain(tags.agents.iter().map(|t| &t.source))
        .chain(tags.resources.iter().map(|t| &t.source))
        .collect();
    if sources.len() > 32 || serde_json::to_vec(tags)?.len() > 64 * 1024 {
        return Err(CoordinatorError::Invalid(
            "selected tags exceed their size limit".into(),
        ));
    }
    sources.sort_unstable_by_key(|s| s.start);
    if sources.windows(2).any(|pair| pair[0].end > pair[1].start) {
        return Err(CoordinatorError::Invalid("selected tags overlap".into()));
    }
    let length = text.chars().count();
    for source in sources {
        if source.end <= source.start
            || source.end > length
            || source.value.len() > 4096
            || text
                .chars()
                .skip(source.start)
                .take(source.end - source.start)
                .collect::<String>()
                != source.value
        {
            return Err(CoordinatorError::Invalid(
                "selected tag no longer matches the prompt".into(),
            ));
        }
    }
    for file in &tags.files {
        if file.path.is_empty()
            || file.path.chars().any(char::is_control)
            || file
                .line_range
                .is_some_and(|r| r.start == 0 || r.end.is_some_and(|end| end < r.start))
        {
            return Err(CoordinatorError::Invalid(
                "invalid selected file path or line range".into(),
            ));
        }
    }
    Ok(())
}

impl super::super::turn::Worker {
    pub(in crate::coord) async fn prompt_context(
        &self,
        tags: &SelectedPromptTags,
    ) -> Result<String, CoordinatorError> {
        let mut context = String::new();
        for file in &tags.files {
            let directory = file.mime == "application/x-directory";
            let mut args = if directory {
                serde_json::json!({"path":file.path,"pattern":"*"})
            } else {
                serde_json::json!({"filePath":file.path})
            };
            if let Some(range) = file.line_range {
                args["offset"] = range.start.into();
                args["limit"] = range
                    .end
                    .unwrap_or(range.start)
                    .saturating_sub(range.start)
                    .saturating_add(1)
                    .min(2000)
                    .into();
            }
            let output = self
                .handle
                .execute_tool(
                    self.actor.clone(),
                    Some(self.turn.id.clone()),
                    None,
                    if directory { "glob" } else { "read" }.into(),
                    args,
                )
                .await?;
            if output.is_error() {
                return Err(CoordinatorError::Invalid(output.display_text));
            }
            context.push_str(&format!(
                "Selected file {}:\n{}\n",
                file.path, output.display_text
            ));
        }
        for agent in &tags.agents {
            context.push_str(&format!(
                "Selected agent profile: {}\n",
                serde_json::to_string(&agent.name)?
            ));
        }
        for resource in &tags.resources {
            context.push_str(&format!(
                "Selected resource: {}\n",
                serde_json::to_string(resource)?
            ));
        }
        if context.is_empty() {
            return Ok(context);
        }
        if context.len() > 1024 * 1024 {
            return Err(CoordinatorError::Invalid(
                "selected context exceeds 1 MiB".into(),
            ));
        }
        let (owner, task) = (self.actor.clone(), self.turn.id.clone());
        self.handle
            .call(move |s| {
                s.check_task(&task)?;
                let context = s.redactor.redact_text(&context);
                s.write_blob(
                    &owner,
                    None,
                    "txt",
                    context.as_bytes(),
                    [("prompt_context".into(), task)].into(),
                )?;
                Ok(context)
            })
            .await
    }
}

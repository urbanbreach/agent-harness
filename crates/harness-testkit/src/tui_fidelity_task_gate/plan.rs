use super::TaskGateError;

pub(super) fn ensure_open_task(plan: &str, task: &str) -> Result<(), TaskGateError> {
    let (_, checked) = task_line(plan, task)?;
    if checked {
        return Err(TaskGateError::Invalid(format!(
            "task {task} is already checked or was manually transitioned"
        )));
    }
    Ok(())
}

pub(super) fn complete_task_checkbox(plan: &str, task: &str) -> Result<String, TaskGateError> {
    let (line_index, checked) = task_line(plan, task)?;
    if checked {
        return Err(TaskGateError::Invalid(format!(
            "task {task} is already checked or was manually transitioned"
        )));
    }
    let mut updated = String::with_capacity(plan.len());
    for (index, segment) in plan.split_inclusive('\n').enumerate() {
        if index == line_index {
            updated.push_str(&segment.replacen("- [ ]", "- [x]", 1));
        } else {
            updated.push_str(segment);
        }
    }
    if !plan.ends_with('\n') && line_index == plan.lines().count().saturating_sub(1) {
        updated = updated.trim_end_matches('\n').to_owned();
    }
    Ok(updated)
}

fn task_line(plan: &str, task: &str) -> Result<(usize, bool), TaskGateError> {
    let label = format!("{task}.");
    for (index, line) in plan.lines().enumerate() {
        let Some(rest) = line.strip_prefix("- [") else {
            continue;
        };
        let checked = rest.starts_with('x');
        if (checked || rest.starts_with(' '))
            && rest.get(1..3) == Some("] ")
            && rest.get(3..).is_some_and(|title| title.starts_with(&label))
        {
            return Ok((index, checked));
        }
    }
    Err(TaskGateError::Invalid(format!(
        "task {task} is missing from the reviewed plan"
    )))
}

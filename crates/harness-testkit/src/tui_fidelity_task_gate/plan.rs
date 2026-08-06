use super::{storage, TaskGateError};

const MACHINE_TASKS: [&str; 15] = [
    "49", "50", "51", "52", "53", "54", "55", "56", "57", "58", "59", "F1", "F2", "F3", "F4",
];

pub(super) fn contract_sha256(plan: &str) -> Result<String, TaskGateError> {
    let mut normalized = String::with_capacity(plan.len());
    for segment in plan.split_inclusive('\n') {
        let mut normalized_segment = segment.to_owned();
        if let Some(marker) = machine_task_marker(segment) {
            normalized_segment.replace_range(marker + 3..marker + 4, " ");
        }
        normalized.push_str(&normalized_segment);
    }
    storage::digest(normalized.as_bytes())
}

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

fn machine_task_marker(line: &str) -> Option<usize> {
    let marker = line.find("- [")?;
    let bytes = line.as_bytes();
    let checkbox = *bytes.get(marker + 3)?;
    if !matches!(checkbox, b' ' | b'x' | b'X') || bytes.get(marker + 4) != Some(&b']') {
        return None;
    }
    let task = line
        .get(marker + 5..)?
        .split_whitespace()
        .next()?
        .trim_end_matches('.');
    MACHINE_TASKS.contains(&task).then_some(marker)
}

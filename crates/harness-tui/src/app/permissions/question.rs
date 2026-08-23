use super::ActivePermissionView;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionPromptView {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOptionView>,
    pub multiple: bool,
    pub custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionOptionView {
    pub label: String,
    pub description: String,
    pub preview: Option<String>,
}

pub(super) fn parse_question_prompts(kind: &str, summary: &str) -> Option<Vec<QuestionPromptView>> {
    if !kind.eq_ignore_ascii_case("question")
        && !kind.eq_ignore_ascii_case("ask")
        && !kind.eq_ignore_ascii_case("ask_user")
    {
        return None;
    }

    let value = serde_json::from_str::<serde_json::Value>(summary).ok()?;
    let questions = value.get("questions")?.as_array()?;
    let prompts = questions
        .iter()
        .map(|question| {
            Some(QuestionPromptView {
                question: question.get("question")?.as_str()?.to_string(),
                header: question.get("header")?.as_str()?.to_string(),
                options: question
                    .get("options")?
                    .as_array()?
                    .iter()
                    .map(|option| {
                        Some(QuestionOptionView {
                            label: option.get("label")?.as_str()?.to_string(),
                            description: option.get("description")?.as_str()?.to_string(),
                            preview: option
                                .get("preview")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
                multiple: question
                    .get("multiple")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                custom: question
                    .get("custom")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true),
            })
        })
        .collect::<Option<Vec<_>>>()?;

    Some(prompts)
}

pub(super) fn build_question_answers(
    prompts: &[QuestionPromptView],
    current_answers: &[Vec<String>],
) -> Result<Vec<Vec<String>>, String> {
    let mut answers = Vec::with_capacity(prompts.len());

    for (index, prompt) in prompts.iter().enumerate() {
        let values = current_answers.get(index).cloned().unwrap_or_default();
        if !prompt.multiple && values.len() > 1 {
            return Err(format!(
                "Question {} ({}) accepts only one answer.",
                index + 1,
                prompt.header
            ));
        }

        answers.push(
            values
                .into_iter()
                .map(|value| {
                    prompt
                        .options
                        .iter()
                        .find(|option| option.label.eq_ignore_ascii_case(&value))
                        .map(|option| option.label.clone())
                        .unwrap_or_else(|| value.to_string())
                })
                .collect(),
        );
    }

    Ok(answers)
}

pub(super) fn question_prompt_is_single_select(prompts: &[QuestionPromptView]) -> bool {
    prompts.len() == 1 && !prompts[0].multiple
}

pub(super) fn question_prompt_tab_count(prompts: &[QuestionPromptView]) -> usize {
    prompts.len().max(1)
}

pub(super) fn question_prompt_choice_count(prompt: &QuestionPromptView) -> usize {
    prompt.options.len() + usize::from(prompt.custom)
}

pub(crate) fn question_option_shortcut_label(index: usize) -> Option<char> {
    let index = u8::try_from(index).ok()?;
    match index {
        0..=8 => Some(char::from(b'1'.saturating_add(index))),
        9..=14 => Some(char::from(b'a'.saturating_add(index.saturating_sub(9)))),
        _ => None,
    }
}

pub(super) fn question_option_index_for_key(key: char) -> Option<usize> {
    match key {
        '1'..='9' => key
            .to_digit(10)
            .and_then(|digit| usize::try_from(digit.saturating_sub(1)).ok()),
        'a'..='f' => usize::try_from(u32::from(key).saturating_sub(u32::from('a')))
            .ok()
            .map(|index| index.saturating_add(9)),
        _ => None,
    }
}

pub(crate) fn permission_display_summary(permission: &ActivePermissionView) -> String {
    if permission.question_prompts.is_some() {
        "Question requested".to_string()
    } else {
        permission.summary.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::question_option_shortcut_label;

    #[test]
    fn shortcuts_stop_before_owned_navigation_keys() {
        // Given: the final canonical letter shortcut and the next option.
        // When: their display labels are requested.
        // Then: only the conflict-free a-f range is advertised.
        assert_eq!(question_option_shortcut_label(14), Some('f'));
        assert_eq!(question_option_shortcut_label(15), None);
    }
}

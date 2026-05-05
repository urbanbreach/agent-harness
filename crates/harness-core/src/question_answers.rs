pub trait QuestionAnswerPrompt {
    fn header(&self) -> &str;
    fn multiple(&self) -> bool;
    fn canonical_option_label<'a>(&'a self, answer: &str) -> Option<&'a str>;
}

pub fn validate_question_answers<P>(
    prompts: &[P],
    answers: Vec<Vec<String>>,
) -> Result<Vec<Vec<String>>, String>
where
    P: QuestionAnswerPrompt,
{
    if answers.len() != prompts.len() {
        return Err(format!(
            "Expected {} answer group(s) for {} question(s); received {}.",
            prompts.len(),
            prompts.len(),
            answers.len()
        ));
    }

    prompts
        .iter()
        .zip(answers)
        .enumerate()
        .map(|(index, (prompt, answers))| normalize_question_answers(index, prompt, answers))
        .collect()
}

fn normalize_question_answers<P>(
    index: usize,
    prompt: &P,
    answers: Vec<String>,
) -> Result<Vec<String>, String>
where
    P: QuestionAnswerPrompt,
{
    let answers = answers
        .into_iter()
        .map(|answer| answer.trim().to_string())
        .filter(|answer| !answer.is_empty())
        .collect::<Vec<_>>();
    if answers.is_empty() {
        return Ok(Vec::new());
    }

    if !prompt.multiple() && answers.len() != 1 {
        return Err(format!(
            "Question {} ({}) accepts only one answer.",
            index + 1,
            prompt.header()
        ));
    }

    Ok(answers
        .into_iter()
        .map(|answer| {
            prompt
                .canonical_option_label(&answer)
                .map(str::to_string)
                .unwrap_or(answer)
        })
        .collect())
}

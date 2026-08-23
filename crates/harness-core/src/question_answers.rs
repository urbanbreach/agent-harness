use crate::UnwrapOrAbort;
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
        .filter_map(|answer| {
            let trimmed = answer.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(
                    prompt
                        .canonical_option_label(trimmed)
                        .map(str::to_string)
                        .unwrap_or(answer),
                )
            }
        })
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

    Ok(answers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    struct Prompt {
        header: &'static str,
        multiple: bool,
    }

    impl QuestionAnswerPrompt for Prompt {
        fn header(&self) -> &str {
            self.header
        }

        fn multiple(&self) -> bool {
            self.multiple
        }

        fn canonical_option_label<'a>(&'a self, answer: &str) -> Option<&'a str> {
            match answer {
                "y" => Some("Yes"),
                "n" => Some("No"),
                _ => None,
            }
        }
    }

    #[test]
    fn validates_question_answer_count_before_normalizing() {
        // arrange
        let prompts = [Prompt {
            header: "Proceed?",
            multiple: false,
        }];

        // act
        let error = validate_question_answers(&prompts, Vec::new()).expect_err("answer count");

        // assert
        assert_eq!(
            error,
            "Expected 1 answer group(s) for 1 question(s); received 0."
        );
    }

    #[test]
    fn rejects_multiple_answers_for_single_select_question() {
        // arrange
        let prompts = [Prompt {
            header: "Proceed?",
            multiple: false,
        }];

        // act
        let error = validate_question_answers(
            &prompts,
            vec![vec![" y ".to_string(), "n".to_string(), " ".to_string()]],
        )
        .expect_err("single-select overflow");

        // assert
        assert_eq!(error, "Question 1 (Proceed?) accepts only one answer.");
    }

    #[test]
    fn trims_filters_and_canonicalizes_valid_answers() {
        // arrange
        let prompts = [Prompt {
            header: "Choices",
            multiple: true,
        }];

        // act
        let answers = validate_question_answers(
            &prompts,
            vec![vec![
                " y ".to_string(),
                " custom ".to_string(),
                " ".to_string(),
            ]],
        )
        .unwrap_or_abort();

        // assert
        assert_eq!(
            answers,
            vec![vec!["Yes".to_string(), " custom ".to_string()]]
        );
    }
}

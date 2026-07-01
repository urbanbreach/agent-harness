use harness_core::tool::ToolError;

mod candidates;

use candidates::replacement_candidates;

pub(crate) struct ReplacementPlan {
    pub(crate) matched_text: String,
    pub(crate) replacements: usize,
}

pub(crate) fn select_replacement_plan(
    content: &str,
    old_string: &str,
    replace_all: bool,
) -> Result<ReplacementPlan, ToolError> {
    let mut found_match = false;
    for candidate in replacement_candidates(content, old_string) {
        let occurrences = count_occurrences(content, &candidate);
        if occurrences == 0 {
            continue;
        }
        found_match = true;
        reject_disproportionate_match(&candidate, old_string)?;
        if replace_all || occurrences == 1 {
            return Ok(ReplacementPlan {
                matched_text: candidate,
                replacements: occurrences,
            });
        }
    }

    if found_match {
        return Err(ToolError::Execution(
            "Found multiple matches for oldString. Provide more surrounding context to make the match unique."
                .to_string(),
        ));
    }

    Err(ToolError::Execution(
        "Could not find oldString in the file. It must match exactly, including whitespace, indentation, and line endings."
            .to_string(),
    ))
}

fn count_occurrences(content: &str, search: &str) -> usize {
    if search.is_empty() {
        return content.len() + 1;
    }
    content.match_indices(search).count()
}

fn reject_disproportionate_match(search: &str, old_string: &str) -> Result<(), ToolError> {
    let old_lines = old_string.split('\n').count();
    let search_lines = search.split('\n').count();
    if search_lines >= std::cmp::max(old_lines + 3, old_lines * 2) {
        return Err(disproportionate_match_error());
    }
    if old_lines == 1 {
        return Ok(());
    }
    let old_len = old_string.trim().len();
    if search.trim().len() > std::cmp::max(old_len + 500, old_len * 4) {
        return Err(disproportionate_match_error());
    }
    Ok(())
}

fn disproportionate_match_error() -> ToolError {
    ToolError::Execution(
        "Refusing replacement because the matched span is much larger than oldString. Re-read the file and provide the full exact oldString for the intended replacement."
            .to_string(),
    )
}

#[derive(Debug, Clone, Copy)]
enum SafeCutContent<'a> {
    Text(&'a str),
    Atomic(u32),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SafeCutCandidate<'a> {
    content: SafeCutContent<'a>,
    joins_previous: bool,
    joins_next: bool,
}

impl<'a> SafeCutCandidate<'a> {
    pub(crate) const fn text(text: &'a str) -> Self {
        Self {
            content: SafeCutContent::Text(text),
            joins_previous: false,
            joins_next: false,
        }
    }

    pub(crate) const fn atomic(tokens: u32, joins_previous: bool, joins_next: bool) -> Self {
        Self {
            content: SafeCutContent::Atomic(tokens),
            joins_previous,
            joins_next,
        }
    }

    pub(super) fn tokens(self, estimate_text_tokens: fn(&str) -> u32) -> u32 {
        match self.content {
            SafeCutContent::Text(text) => estimate_text_tokens(text),
            SafeCutContent::Atomic(tokens) => tokens,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Utf8TextSplit {
    pub(crate) entry_index: usize,
    pub(crate) byte_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SafeCutPlan {
    pub(crate) first_kept_index: usize,
    pub(crate) text_split: Option<Utf8TextSplit>,
    pub(crate) retained_tokens: u32,
    pub(crate) summarized_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SafeCutError {
    NoSafeCut,
}

pub(crate) fn plan_safe_cut(
    candidates: &[SafeCutCandidate<'_>],
    keep_recent_tokens: u32,
    estimate_text_tokens: fn(&str) -> u32,
) -> Result<SafeCutPlan, SafeCutError> {
    if candidates.is_empty() || keep_recent_tokens == 0 {
        return Err(SafeCutError::NoSafeCut);
    }

    let mut first_kept_index = candidates.len();
    let mut retained_tokens = 0_u32;
    let mut text_split = None;

    for (index, candidate) in candidates.iter().enumerate().rev() {
        let candidate_tokens = candidate.tokens(estimate_text_tokens);
        if retained_tokens.saturating_add(candidate_tokens) <= keep_recent_tokens {
            retained_tokens = retained_tokens.saturating_add(candidate_tokens);
            first_kept_index = index;
            continue;
        }

        let available_tokens = keep_recent_tokens.saturating_sub(retained_tokens);
        if let SafeCutContent::Text(text) = candidate.content {
            if let Some(byte_index) =
                suffix_start_for_budget(text, available_tokens, estimate_text_tokens)
            {
                let Some(suffix) = text.get(byte_index..) else {
                    return Err(SafeCutError::NoSafeCut);
                };
                retained_tokens = retained_tokens.saturating_add(estimate_text_tokens(suffix));
                first_kept_index = index;
                text_split = Some(Utf8TextSplit {
                    entry_index: index,
                    byte_index,
                });
            }
        }
        break;
    }

    if first_kept_index == candidates.len() {
        return Err(SafeCutError::NoSafeCut);
    }
    let Some(first_kept) = candidates.get(first_kept_index) else {
        return Err(SafeCutError::NoSafeCut);
    };
    if text_split.is_some() && (first_kept.joins_previous || first_kept.joins_next) {
        return Err(SafeCutError::NoSafeCut);
    }

    while first_kept_index > 0 {
        let Some(current) = candidates.get(first_kept_index) else {
            return Err(SafeCutError::NoSafeCut);
        };
        let Some(previous) = candidates.get(first_kept_index - 1) else {
            return Err(SafeCutError::NoSafeCut);
        };
        if !current.joins_previous && !previous.joins_next {
            break;
        }
        first_kept_index -= 1;
        let Some(joined) = candidates.get(first_kept_index) else {
            return Err(SafeCutError::NoSafeCut);
        };
        retained_tokens = retained_tokens.saturating_add(joined.tokens(estimate_text_tokens));
        if retained_tokens > keep_recent_tokens {
            return Err(SafeCutError::NoSafeCut);
        }
    }

    let mut summarized_tokens = candidates.get(..first_kept_index).map_or(0, |summarized| {
        summarized.iter().fold(0_u32, |total, candidate| {
            total.saturating_add(candidate.tokens(estimate_text_tokens))
        })
    });
    if let Some(split) = text_split {
        let Some(SafeCutCandidate {
            content: SafeCutContent::Text(text),
            ..
        }) = candidates.get(split.entry_index)
        else {
            return Err(SafeCutError::NoSafeCut);
        };
        let Some(prefix) = text.get(..split.byte_index) else {
            return Err(SafeCutError::NoSafeCut);
        };
        summarized_tokens = summarized_tokens.saturating_add(estimate_text_tokens(prefix));
    }

    Ok(SafeCutPlan {
        first_kept_index,
        text_split,
        retained_tokens,
        summarized_tokens,
    })
}

fn suffix_start_for_budget(
    text: &str,
    available_tokens: u32,
    estimate_text_tokens: fn(&str) -> u32,
) -> Option<usize> {
    if available_tokens == 0 {
        return None;
    }
    text.char_indices()
        .map(|(byte_index, _)| byte_index)
        .skip(1)
        .find(|&byte_index| {
            text.get(byte_index..)
                .is_some_and(|suffix| estimate_text_tokens(suffix) <= available_tokens)
        })
}

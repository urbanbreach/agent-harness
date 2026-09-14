#[derive(Debug, Clone, Copy)]
enum SafeCutContent<'a> {
    Text(&'a str),
    Atomic(u32),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SafeCutCandidate<'a> {
    content: SafeCutContent<'a>,
    joins_previous: bool,
}

impl<'a> SafeCutCandidate<'a> {
    pub(crate) const fn text(text: &'a str) -> Self {
        Self {
            content: SafeCutContent::Text(text),
            joins_previous: false,
        }
    }

    pub(crate) const fn atomic(tokens: u32, joins_previous: bool, _joins_next: bool) -> Self {
        Self {
            content: SafeCutContent::Atomic(tokens),
            joins_previous,
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
pub(crate) struct SafeCutPlan {
    pub(crate) first_kept_index: usize,
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

    // Senpi keeps whole messages. The target is approximate: a final tool batch
    // may exceed it, but its assistant call and results must remain together.
    let valid: Vec<_> = candidates
        .iter()
        .enumerate()
        .filter(|(_, item)| !item.joins_previous && item.tokens(estimate_text_tokens) > 0)
        .map(|(index, _)| index)
        .collect();
    let Some(&first) = valid.first() else {
        return Err(SafeCutError::NoSafeCut);
    };
    let mut first_kept_index = first;
    let mut accumulated = 0_u32;
    for (index, item) in candidates.iter().enumerate().rev() {
        accumulated = accumulated.saturating_add(item.tokens(estimate_text_tokens));
        if accumulated >= keep_recent_tokens {
            first_kept_index = valid
                .iter()
                .copied()
                .find(|&cut| cut >= index)
                .or_else(|| valid.last().copied())
                .ok_or(SafeCutError::NoSafeCut)?;
            break;
        }
    }
    let tokens = |items: &[SafeCutCandidate<'_>]| {
        items.iter().fold(0_u32, |total, item| {
            total.saturating_add(item.tokens(estimate_text_tokens))
        })
    };
    Ok(SafeCutPlan {
        first_kept_index,
        retained_tokens: tokens(&candidates[first_kept_index..]),
        summarized_tokens: tokens(&candidates[..first_kept_index]),
    })
}

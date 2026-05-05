#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LimitSummary {
    pub(crate) returned_count: usize,
    pub(crate) truncated_count: usize,
    pub(crate) is_truncated: bool,
}

pub(crate) fn summarize_limit(total_count: usize, limit: usize) -> LimitSummary {
    let returned_count = total_count.min(limit);
    let truncated_count = total_count.saturating_sub(returned_count);
    LimitSummary {
        returned_count,
        truncated_count,
        is_truncated: truncated_count > 0,
    }
}

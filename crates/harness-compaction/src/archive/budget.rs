use super::{capacity, ArchiveShape};

#[derive(Debug)]
pub struct FrameBudgetInput {
    pub window: u32,
    pub reserve: Option<u32>,
    pub nonmessage_tokens: u32,
    pub kept_tokens: u32,
    pub kept_without_reasoning_tokens: u32,
    pub pending_tokens: u32,
    pub threshold: u32,
    pub shape: ArchiveShape,
    pub byte_budget: u64,
    pub image_budget: u32,
}

pub fn byte_frame_cap(shape: &ArchiveShape, byte_budget: u64) -> u32 {
    let canonical = shape.font == "8x13"
        && matches!((shape.cell_width, shape.cell_height), (11, 16) | (8, 22))
        && shape.line_repeat == 1
        && shape.ink == super::Ink::Black
        && shape.columns == 1
        && !shape.stopword_dim
        && !shape.stretch;
    let estimate = if canonical {
        (170_000 * u64::from(shape.edge).pow(2))
            .div_ceil(1932_u64.pow(2))
            .min(170_000)
    } else {
        170_000
    };
    u32::try_from(byte_budget / estimate.max(1))
        .unwrap_or(u32::MAX)
        .max(1)
}

/// Initial archive planning permits a one-frame text-only attempt unless live
/// history already consumes the available window. Fit checks belong to callers.
pub fn plan_frame_budget(input: &FrameBudgetInput) -> u32 {
    let cap = 80
        .min(byte_frame_cap(&input.shape, input.byte_budget))
        .min(input.image_budget);
    if cap == 0 || input.window == 0 {
        return cap;
    }
    let reserve = crate::policy::effective_reserve(input.window, input.reserve);
    let available = i64::from(input.window) - i64::from(reserve);
    let base = i64::from(input.nonmessage_tokens) + i64::from(input.kept_tokens);
    if base >= available {
        return 0;
    }
    let edges = i64::try_from((u64::from(capacity(&input.shape)) * 23).div_ceil(40))
        .unwrap_or(i64::MAX)
        + 2000;
    let fixed = i64::from(input.nonmessage_tokens)
        + i64::from(input.kept_without_reasoning_tokens)
        + i64::from(input.pending_tokens)
        + edges;
    let threshold = i64::from(input.threshold);
    let room = ((threshold - fixed).div_euclid(2)).min(threshold * 3 / 5 - fixed);
    let budget = (available - base - edges).min(room);
    let price = i64::from(input.shape.frame_token_estimate.max(1));
    if budget < price {
        return 1.min(cap);
    }
    u32::try_from(budget / price).unwrap_or(u32::MAX).min(cap)
}

/// Frame budget for re-rendering an existing archive at a maintenance dead end.
///
/// The frames must fit the recovery band beside the fixed prompt, the kept tail and the
/// text edges. Unlike initial planning there is no one-frame fallback; zero means no
/// rescue fits. An unknown window (`threshold == None`) uses the hard caps.
pub fn rescue_frame_budget(
    shape: &ArchiveShape,
    threshold: Option<u32>,
    nonmessage_tokens: u32,
    tail_tokens: u32,
    byte_budget: u64,
    image_budget: u32,
) -> u32 {
    let cap = 80.min(byte_frame_cap(shape, byte_budget)).min(image_budget);
    let Some(threshold) = threshold else {
        return cap;
    };
    let edges =
        i64::try_from((u64::from(capacity(shape)) * 23).div_ceil(40)).unwrap_or(i64::MAX) + 2000;
    let room = i64::from(crate::policy::recovery_target(threshold))
        - i64::from(nonmessage_tokens)
        - i64::from(tail_tokens)
        - edges;
    let price = i64::from(shape.frame_token_estimate.max(1));
    if room < price {
        return 0;
    }
    u32::try_from(room / price).unwrap_or(u32::MAX).min(cap)
}

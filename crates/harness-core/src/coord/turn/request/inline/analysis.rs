//! Candidate gathering, planning and application on the final request arrays.
use super::cache::{fingerprint, frames_of, Renderer, SYSTEM_FRAME_CAP};
use super::estimate::SystemReason;
use super::planner::{saves, Item, Plan, Retraction, Selection, SystemCandidate};
use super::system::{self, Target};
use super::tools::{self, is_image, Candidates};
use super::{Attachments, Content, Env, Fact};
use crate::attachment_transport::AttachmentMetadata;
use crate::coord::context::archive::inline_cache::{InlineCache, Rendered};
use harness_providers::{CompletionMessage, MessageRole};
use std::{collections::HashSet, sync::Arc};

/// A system prompt target that passed every gate.
pub(super) struct SystemSwap {
    pub target: Target,
    pub frames: Arc<[AttachmentMetadata]>,
    pub tokens: u32,
    pub carrier: usize,
}

pub(super) struct Analysis {
    /// Number of leading system messages.
    pub leading: usize,
    pub system: Option<SystemSwap>,
    /// Why there is no system swap.
    pub reason: Option<SystemReason>,
    pub tools: Candidates,
    /// The request's selection after the total cap backstop.
    pub selection: Selection,
    pub retractions: Vec<Retraction>,
}

fn image_count(attachments: &Attachments, index: usize) -> u32 {
    let count = attachments
        .get(&index)
        .map_or(0, |list| list.iter().filter(|a| is_image(a)).count());
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Plans the swaps of one request. `assume_carrier` lets an empty live history count as
/// if the next request supplied a first user message.
pub(super) fn analyze(
    env: &Env,
    messages: &[CompletionMessage],
    attachments: &Attachments,
    facts: &[Fact],
    cache: &mut InlineCache,
    assume_carrier: bool,
) -> Analysis {
    let sink = cache.sink.clone();
    let renderer = Renderer {
        shape: &env.shape,
        sink: sink.as_deref(),
    };
    let leading = messages
        .iter()
        .take_while(|message| message.role == MessageRole::System)
        .count();
    let mut images_through = Vec::with_capacity(messages.len() + 1);
    let mut running = 0u32;
    for index in 0..messages.len() {
        running = running.saturating_add(image_count(attachments, index));
        images_through.push(running);
    }
    let mut carrier = messages.iter().position(|m| m.role == MessageRole::User);
    if carrier.is_none() && assume_carrier && messages.len() == leading {
        carrier = Some(messages.len());
        images_through.push(running);
    }
    let (system, reason) = system_swap(
        env,
        messages,
        leading,
        carrier,
        &images_through,
        cache,
        &renderer,
    );
    let tools = if env.tool_results {
        let built = tools::candidates(env, messages, attachments, facts, cache, &renderer);
        cache.tools.retain(|id, _| built.live.contains(id));
        built
    } else {
        cache.tools.clear();
        Candidates {
            built: Vec::new(),
            live: HashSet::new(),
        }
    };
    plan_and_finish(env, leading, system, reason, tools, images_through)
}

fn plan_and_finish(
    env: &Env,
    leading: usize,
    system: Option<SystemSwap>,
    reason: Option<SystemReason>,
    tools: Candidates,
    images_through: Vec<u32>,
) -> Analysis {
    let candidates = tools.planner();
    let plan = Plan {
        limit: env.limit,
        images_through: &images_through,
        system: system.as_ref().map(|swap| SystemCandidate {
            carrier: swap.carrier,
            frames: swap.frames.len(),
        }),
        tools: &candidates,
    };
    let natural = plan.natural();
    let (selection, retractions) = plan.replay(&natural);
    Analysis {
        leading,
        system,
        reason,
        tools,
        selection,
        retractions,
    }
}

/// The system target with its rendered frames when it passes the carrier, budget and
/// margin gates; otherwise the reason it does not.
fn system_swap(
    env: &Env,
    messages: &[CompletionMessage],
    leading: usize,
    carrier: Option<usize>,
    images_through: &[u32],
    cache: &mut InlineCache,
    renderer: &Renderer,
) -> (Option<SystemSwap>, Option<SystemReason>) {
    let Some(target) = system::extract(env.mode, &messages[..leading]) else {
        cache.system = None;
        cache.system_tokens = None;
        return (None, Some(SystemReason::Empty));
    };
    let Some(carrier) = carrier else {
        return (None, Some(SystemReason::NoCarrier));
    };
    let key = fingerprint(&[target.text.as_bytes()]);
    let domain = env.counter.domain_id();
    let tokens = match cache.system_tokens {
        Some((cached, cached_domain, tokens)) if cached == key && cached_domain == domain => tokens,
        _ => {
            let tokens = env.counter.count_text(&target.text);
            cache.system_tokens = Some((key, domain, tokens));
            tokens
        }
    };
    let raw = env.shape.raw_frame_tokens();
    if !saves(tokens, 1, raw) {
        return (None, Some(SystemReason::Margin));
    }
    let used = images_through.get(carrier).copied().unwrap_or(0);
    let room = env.limit.saturating_sub(used).min(SYSTEM_FRAME_CAP);
    if room == 0 {
        return (None, Some(SystemReason::Budget));
    }
    let rendered = renderer.render(
        &mut cache.system,
        key,
        || target.text.clone(),
        SYSTEM_FRAME_CAP,
        "system",
    );
    let Some(frames) = frames_of(&rendered).map(Arc::clone) else {
        let reason = if matches!(rendered, Rendered::TooMany) {
            SystemReason::Budget
        } else {
            SystemReason::Empty
        };
        return (None, Some(reason));
    };
    if frames.len() > usize::try_from(room).unwrap_or(0) {
        return (None, Some(SystemReason::Budget));
    }
    if !saves(tokens, frames.len(), raw) {
        return (None, Some(SystemReason::Margin));
    }
    let swap = SystemSwap {
        target,
        frames,
        tokens,
        carrier,
    };
    (Some(swap), None)
}

/// Applies the final selection to the outgoing copy and drops the provider continuations
/// of assistant messages bound to a prefix a retraction rewrote.
pub(super) fn apply(
    analysis: &Analysis,
    messages: &mut Vec<CompletionMessage>,
    attachments: &mut Attachments,
    content: &mut Content,
) {
    for tool in &analysis.tools.built {
        if analysis.selection.tools.contains(&tool.index) {
            tools::apply(
                &mut messages[tool.index],
                tool.index,
                &tool.frames,
                attachments,
                content,
            );
        }
    }
    drop_stale_continuations(analysis, messages);
    if let (true, Some(swap)) = (analysis.selection.system, &analysis.system) {
        system::carry(
            messages,
            swap.carrier,
            swap.target.note,
            &swap.frames,
            attachments,
            content,
        );
        system::replace_leading(
            messages,
            analysis.leading,
            swap.target.replacement.clone(),
            attachments,
            content,
        );
    }
}

fn item_index(analysis: &Analysis, item: Item) -> Option<usize> {
    match item {
        Item::System => analysis.system.as_ref().map(|swap| swap.carrier),
        Item::Tool(index) => Some(index),
    }
}

/// Retracted swaps rewrote bytes the provider already saw as frames: assistant messages
/// produced after them and before the retractor were bound to the superseded prefix.
fn drop_stale_continuations(analysis: &Analysis, messages: &mut [CompletionMessage]) {
    for retraction in &analysis.retractions {
        let Some(index) = item_index(analysis, retraction.item) else {
            continue;
        };
        for message in messages
            .iter_mut()
            .take(retraction.at)
            .skip(index + 1)
            .filter(|message| message.role == MessageRole::Assistant)
        {
            message.continuation = None;
        }
    }
}

/// Records the rewrite time of each retraction that has an assistant message between the
/// swapped message and its retractor: the retractor's time, never earlier than a stamp,
/// reducer time or carrier rewrite time already on the entry.
pub(super) fn stamp_rewrites(
    analysis: &Analysis,
    messages: &[CompletionMessage],
    facts: &[Fact],
    cache: &mut InlineCache,
) {
    for retraction in &analysis.retractions {
        let Some(index) = item_index(analysis, retraction.item) else {
            continue;
        };
        let assistant_between = messages
            .get(index + 1..retraction.at)
            .is_some_and(|between| between.iter().any(|m| m.role == MessageRole::Assistant));
        let at = facts.get(retraction.at).and_then(|fact| fact.at_ms);
        let (Some(fact), true, Some(at)) = (facts.get(index), assistant_between, at) else {
            continue;
        };
        let Some(seq) = fact.seq else {
            continue;
        };
        let call_id = match retraction.item {
            Item::System => None,
            Item::Tool(_) => messages[index].tool_call_id.as_deref(),
        };
        let existing = fact
            .pruned_at_ms
            .unwrap_or(0)
            .max(fact.history_rewrite_at_ms.unwrap_or(0));
        cache.stamp(seq, call_id, at.max(existing));
    }
}

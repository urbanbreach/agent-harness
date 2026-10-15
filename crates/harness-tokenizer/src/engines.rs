//! Native ordinary-content token counters, one per [`Encoding`].
//!
//! Every vocabulary is bundled as zstd data under `data/` and decoded once on first use, so no
//! counter touches the network. Counts exclude chat templates, special-token interpretation and
//! the fixed message frames providers add around content. Fragments are counted independently by
//! the caller.

mod bpe;
mod ctok;
mod hf;
mod pack;
mod split;

#[cfg(test)]
mod tests;

use std::sync::LazyLock;

use crate::Encoding;

static O200K: LazyLock<Option<bpe::Bpe>> = LazyLock::new(|| {
    bpe::Bpe::load(
        include_bytes!("../data/o200k_base.bin.zst"),
        split::Dialect::O200k,
    )
});
static KIMI: LazyLock<Option<bpe::Bpe>> = LazyLock::new(|| {
    bpe::Bpe::load(
        include_bytes!("../data/kimi_k2.bin.zst"),
        split::Dialect::Kimi,
    )
});
static QWEN3: LazyLock<Option<hf::Hf>> =
    LazyLock::new(|| hf::Hf::load(include_bytes!("../data/qwen3.bin.zst")));
static DEEPSEEK3: LazyLock<Option<hf::Hf>> =
    LazyLock::new(|| hf::Hf::load(include_bytes!("../data/deepseek3.bin.zst")));
static GLM5: LazyLock<Option<hf::Hf>> =
    LazyLock::new(|| hf::Hf::load(include_bytes!("../data/glm5.bin.zst")));
static CLAUDE_V3: LazyLock<Option<ctok::Model>> =
    LazyLock::new(|| ctok::Model::load(include_bytes!("../data/ctok_v3.json.zst")));
/// Shared by v4.7, v5 and the v5 Sonnet variant, which differ only in framing.
static CLAUDE_V4_7: LazyLock<Option<ctok::Model>> =
    LazyLock::new(|| ctok::Model::load(include_bytes!("../data/ctok_v4_7.json.zst")));

/// Exact ordinary-content token count of `text` under `encoding`, or `None` when the bundled
/// vocabulary failed to initialise (corrupt asset). Claude counts exclude the fixed message frame:
/// empty content costs 1 on v3 and v4.7 and 0 on the v5 families.
pub(crate) fn count(encoding: Encoding, text: &str) -> Option<u32> {
    match encoding {
        Encoding::O200k => O200K.as_ref()?.count(text),
        Encoding::KimiK2 => KIMI.as_ref()?.count(text),
        Encoding::Qwen3 => QWEN3.as_ref()?.count(text),
        Encoding::DeepSeekV3 => DEEPSEEK3.as_ref()?.count(text),
        Encoding::Glm5 => GLM5.as_ref()?.count(text),
        Encoding::ClaudeV3 => Some(CLAUDE_V3.as_ref()?.count(text, &ctok::Frame::V3)),
        Encoding::ClaudeV47 => Some(CLAUDE_V4_7.as_ref()?.count(text, &ctok::Frame::V4_7)),
        Encoding::ClaudeV5 => Some(CLAUDE_V4_7.as_ref()?.count(text, &ctok::Frame::V5)),
        Encoding::ClaudeV5Sonnet => {
            Some(CLAUDE_V4_7.as_ref()?.count(text, &ctok::Frame::V5_SONNET))
        }
    }
}

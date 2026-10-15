use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use rustc_hash::FxHasher;

use crate::identity::{classify, encoding_for, Family};
use crate::image::{resolve, Rule};

mod cache;
#[cfg(test)]
mod tests;

use cache::FragmentCache;

/// Fragment arrays below this length use individual cacheable counts; longer arrays and empty
/// arrays count as one batch that bypasses the cache.
const BATCH_THRESHOLD: usize = 16;

/// Whether unknown models count with o200k instead of bytes. Read once per process.
static ACCURATE: LazyLock<bool> =
    LazyLock::new(|| std::env::var("HARNESS_TOKENIZER_ACCURATE").is_ok_and(|value| value == "1"));

/// Native vocabulary selected for a model identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Encoding {
    O200k,
    ClaudeV3,
    ClaudeV47,
    ClaudeV5,
    ClaudeV5Sonnet,
    Qwen3,
    DeepSeekV3,
    KimiK2,
    Glm5,
}

/// Requested counting mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CountMode {
    Strict,
    Approximate,
    UpperBound,
}

/// Wire API of the reader that will see the counted content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReaderApi {
    OpenAiResponses,
    OpenAiChat,
    Anthropic,
    Other,
}

/// Identity of the model that reads the counted content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reader {
    pub api: ReaderApi,
    pub provider: String,
    pub model_id: String,
    pub official_endpoint: bool,
    pub prefix_binding: bool,
    pub filter_reasoning_history: bool,
}

/// How opaque reasoning is priced for the reader's lineage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReasoningPricing {
    /// Signature and redacted data count as text fragments.
    Signature,
    /// Signed reasoning costs at least the output tokens the provider reported.
    OutputSize,
}

/// Result of a budget probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    pub fits: bool,
    pub tokens: u64,
    pub exact: bool,
}

/// Immutable pricing domain: one counter per reader identity.
#[derive(Debug)]
pub struct Counter {
    reader: Option<Reader>,
    encoding: Option<Encoding>,
    pub(crate) image_rule: Option<Rule>,
    /// False for a counter that estimates from bytes unless strict counting is requested.
    native_default: bool,
    history_replay: bool,
    cache: Mutex<FragmentCache>,
}

impl Counter {
    pub fn new(reader: Option<Reader>) -> Self {
        Self::build(reader, true)
    }

    /// A counter for a model reached over `api`, with no endpoint facts.
    pub fn for_model(provider: &str, model_id: &str, api: ReaderApi) -> Self {
        Self::new(Some(Reader {
            api,
            provider: provider.to_owned(),
            model_id: model_id.to_owned(),
            official_endpoint: false,
            prefix_binding: false,
            filter_reasoning_history: false,
        }))
    }

    /// A counter whose default and upper-bound counts never use a native vocabulary: bytes/4
    /// and bytes. Strict counts are unchanged. Used for deterministic offline estimates.
    pub fn byte_estimating(reader: Option<Reader>) -> Self {
        Self::build(reader, false)
    }

    fn build(reader: Option<Reader>, native_default: bool) -> Self {
        let encoding = reader
            .as_ref()
            .and_then(|reader| encoding_for(&reader.provider, &reader.model_id));
        let image_rule = resolve(reader.as_ref());
        Self {
            reader,
            encoding,
            image_rule,
            native_default,
            history_replay: true,
            cache: Mutex::default(),
        }
    }

    /// Declares whether native history replays to the provider (a warm session). Defaults to
    /// true.
    #[must_use]
    pub fn with_history_replay(mut self, replay: bool) -> Self {
        self.history_replay = replay;
        self
    }

    pub fn history_replay(&self) -> bool {
        self.history_replay
    }

    pub fn encoding(&self) -> Option<Encoding> {
        self.encoding
    }

    pub fn reader(&self) -> Option<&Reader> {
        self.reader.as_ref()
    }

    /// The reader that prices reasoning: it needs a nonempty provider and model and must not
    /// filter reasoning history.
    pub fn reasoning_reader(&self) -> Option<&Reader> {
        self.reader.as_ref().filter(|reader| {
            !reader.provider.is_empty()
                && !reader.model_id.is_empty()
                && !reader.filter_reasoning_history
        })
    }

    /// Anthropic lineage prices signed reasoning by output size on every host; other lineages
    /// price the opaque signature text.
    pub fn reasoning_pricing(&self) -> ReasoningPricing {
        let anthropic = self.reader.as_ref().is_some_and(|reader| {
            matches!(
                classify(&reader.provider, &reader.model_id).family,
                Family::Anthropic { .. }
            )
        });
        if anthropic {
            ReasoningPricing::OutputSize
        } else {
            ReasoningPricing::Signature
        }
    }

    /// Another model on the same api and provider may replay signed reasoning: an Anthropic
    /// reader on an official endpoint, or one whose thinking binds to the prefix.
    pub fn deployment_signatures(&self) -> bool {
        self.reasoning_reader().is_some_and(|reader| {
            reader.api == ReaderApi::Anthropic
                && (reader.official_endpoint || reader.prefix_binding)
        })
    }

    /// `api NUL provider NUL model` of the reasoning reader.
    pub fn reader_key(&self) -> Option<String> {
        self.reasoning_reader()
            .map(|reader| format!("{:?}\0{}\0{}", reader.api, reader.provider, reader.model_id))
    }

    /// Default (approximate) count.
    pub fn count_text(&self, text: &str) -> u32 {
        self.count_text_mode(text, CountMode::Approximate)
    }

    pub fn count_text_mode(&self, text: &str, mode: CountMode) -> u32 {
        match self.native_encoding(mode) {
            Some(encoding) => self
                .count_native(encoding, text, true)
                .unwrap_or_else(|| byte_count(text, mode)),
            None => byte_count(text, mode),
        }
    }

    /// Sum of independently rounded fragments in the default mode; the sum is never rounded
    /// again.
    pub fn count_fragments<'a>(&self, fragments: impl IntoIterator<Item = &'a str>) -> u32 {
        self.count_fragments_mode(fragments, CountMode::Approximate)
    }

    pub fn count_fragments_mode<'a>(
        &self,
        fragments: impl IntoIterator<Item = &'a str>,
        mode: CountMode,
    ) -> u32 {
        let fragments: Vec<&str> = fragments.into_iter().collect();
        let cached = (1..BATCH_THRESHOLD).contains(&fragments.len());
        let Some(encoding) = self.native_encoding(mode) else {
            return fragments.iter().fold(0u32, |total, text| {
                total.saturating_add(byte_count(text, mode))
            });
        };
        fragments.iter().fold(0u32, |total, text| {
            let count = self
                .count_native(encoding, text, cached)
                .unwrap_or_else(|| byte_count(text, mode));
            total.saturating_add(count)
        })
    }

    /// Budget probe: raw UTF-8 bytes first, one strict count only when the bytes do not prove
    /// the fit. A negative budget rejects even empty text.
    pub fn probe_budget<'a>(
        &self,
        fragments: impl IntoIterator<Item = &'a str>,
        budget: i64,
    ) -> Probe {
        let fragments: Vec<&str> = fragments.into_iter().collect();
        let bytes = fragments.iter().fold(0u64, |total, fragment| {
            total.saturating_add(fragment.len() as u64)
        });
        if i64::try_from(bytes).is_ok_and(|bytes| bytes <= budget) {
            return Probe {
                fits: true,
                tokens: bytes,
                exact: false,
            };
        }
        let encoding = self.encoding.unwrap_or(Encoding::O200k);
        let cached = (1..BATCH_THRESHOLD).contains(&fragments.len());
        let mut tokens = 0u64;
        let mut exact = true;
        for fragment in fragments {
            match self.count_native(encoding, fragment, cached) {
                Some(count) => tokens = tokens.saturating_add(u64::from(count)),
                None => {
                    tokens = tokens.saturating_add(fragment.len() as u64);
                    exact = false;
                }
            }
        }
        Probe {
            fits: i64::try_from(tokens).is_ok_and(|tokens| tokens <= budget),
            tokens,
            exact,
        }
    }

    /// Two counters are interchangeable when they price content identically: same native
    /// encoding, image and frame billing, reasoning reader, reasoning pricing and replay state.
    pub fn equivalent(&self, other: &Self) -> bool {
        self.domain_key() == other.domain_key()
    }

    /// Stable identity of the pricing domain, for caches keyed by counter.
    pub fn domain_id(&self) -> u64 {
        let mut hasher = FxHasher::default();
        self.domain_key().hash(&mut hasher);
        hasher.finish()
    }

    fn domain_key(&self) -> impl Eq + Hash + std::fmt::Debug {
        (
            self.encoding,
            crate::image::rule_key(self.image_rule),
            self.frame_key(),
            self.reader_key(),
            self.reasoning_pricing(),
            self.deployment_signatures(),
            self.history_replay,
            self.native_default,
        )
    }

    /// Native encoding that serves `mode`, or `None` when the mode estimates from bytes.
    fn native_encoding(&self, mode: CountMode) -> Option<Encoding> {
        if mode == CountMode::Strict {
            return Some(self.encoding.unwrap_or(Encoding::O200k));
        }
        if !self.native_default {
            return None;
        }
        self.encoding
            .or_else(|| (*ACCURATE).then_some(Encoding::O200k))
    }

    /// Exact native count, `None` when the vocabulary failed to initialise. Only exact counts
    /// enter the cache.
    fn count_native(&self, encoding: Encoding, text: &str, cached: bool) -> Option<u32> {
        if text.is_empty() {
            return crate::engines::count(encoding, text);
        }
        if cached
            && let Ok(mut cache) = self.cache.lock()
            && let Some(count) = cache.get(text)
        {
            return Some(count);
        }
        let count = crate::engines::count(encoding, text)?;
        if cached && let Ok(mut cache) = self.cache.lock() {
            cache.insert(text, count);
        }
        Some(count)
    }
}

/// The byte estimate for a mode: bytes/4 rounded up for approximate, raw bytes otherwise.
fn byte_count(text: &str, mode: CountMode) -> u32 {
    match mode {
        CountMode::Approximate => saturating_u32(text.len().div_ceil(4)),
        CountMode::UpperBound | CountMode::Strict => saturating_u32(text.len()),
    }
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

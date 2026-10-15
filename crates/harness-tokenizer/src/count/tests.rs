use super::*;

fn reader(api: ReaderApi, provider: &str, model: &str) -> Reader {
    Reader {
        api,
        provider: provider.into(),
        model_id: model.into(),
        official_endpoint: true,
        prefix_binding: false,
        filter_reasoning_history: false,
    }
}

fn bytes_counter() -> Counter {
    Counter::byte_estimating(None)
}

#[test]
fn modes_follow_the_documented_table() {
    let counter = bytes_counter();
    assert_eq!(counter.count_text("hello world"), 3);
    assert_eq!(
        counter.count_text_mode("hello world", CountMode::UpperBound),
        11
    );
    assert_eq!(counter.count_text_mode("hello world", CountMode::Strict), 2);
    assert_eq!(counter.count_text(""), 0);
    // A known family counts natively in the default mode and the upper-bound mode.
    let known = Counter::new(Some(reader(
        ReaderApi::OpenAiResponses,
        "openai",
        "gpt-5.5",
    )));
    assert_eq!(known.encoding(), Some(Encoding::O200k));
    for mode in [
        CountMode::Approximate,
        CountMode::UpperBound,
        CountMode::Strict,
    ] {
        assert_eq!(known.count_text_mode("hello world", mode), 2);
    }
}

#[test]
fn fragments_round_up_independently() {
    let counter = bytes_counter();
    assert_eq!(counter.count_fragments(["a", "a", "a", "a"]), 4);
    assert_eq!(counter.count_fragments(std::iter::empty()), 0);
    assert_eq!(
        counter.count_fragments_mode(["a", "bb"], CountMode::UpperBound),
        3
    );
}

#[test]
fn probe_follows_the_budget_rules() {
    let counter = bytes_counter();
    let text = "x".repeat(40);
    let cases = [
        (vec![text.as_str()], 40, true, 40, false),
        (vec![text.as_str()], 20, true, 5, true),
        (vec![text.as_str()], 3, false, 5, true),
        (vec![], -1, false, 0, true),
        (vec![""], 0, true, 0, false),
    ];
    for (fragments, budget, fits, tokens, exact) in cases {
        assert_eq!(
            counter.probe_budget(fragments, budget),
            Probe {
                fits,
                tokens,
                exact
            }
        );
    }
}

#[test]
fn equivalence_compares_every_pricing_input() {
    let opus = Counter::new(Some(reader(
        ReaderApi::Anthropic,
        "anthropic",
        "claude-opus-5",
    )));
    assert_eq!(opus.encoding(), Some(Encoding::ClaudeV5));
    assert_eq!(opus.reasoning_pricing(), ReasoningPricing::OutputSize);
    assert!(opus.equivalent(&Counter::new(Some(reader(
        ReaderApi::Anthropic,
        "anthropic",
        "claude-opus-5"
    )))));
    let rebuilt =
        |model: &str| Counter::new(Some(reader(ReaderApi::Anthropic, "anthropic", model)));
    assert!(!opus.equivalent(&rebuilt("claude-sonnet-5")));
    assert!(!opus.equivalent(&bytes_counter()));
    let cold = Counter::new(Some(reader(
        ReaderApi::Anthropic,
        "anthropic",
        "claude-opus-5",
    )))
    .with_history_replay(false);
    assert!(!opus.equivalent(&cold));
    assert_ne!(opus.domain_id(), cold.domain_id());
    let mut filtered = reader(ReaderApi::Anthropic, "anthropic", "claude-opus-5");
    filtered.filter_reasoning_history = true;
    let filtered = Counter::new(Some(filtered));
    assert_eq!(filtered.reader_key(), None);
    assert!(!opus.equivalent(&filtered));
}

#[test]
fn fragment_cache_is_bounded_and_skips_long_text() {
    let mut cache = FragmentCache::default();
    for n in 0..300 {
        cache.insert(&format!("fragment {n}"), n);
    }
    assert_eq!(cache.len(), 256);
    assert_eq!(cache.get("fragment 299"), Some(299));
    assert_eq!(cache.get("fragment 0"), None);
    cache.insert(&"é".repeat(16385), 1);
    assert_eq!(cache.get(&"é".repeat(16385)), None);
    let edge = "é".repeat(16384);
    cache.insert(&edge, 7);
    assert_eq!(cache.get(&edge), Some(7));
}

#[test]
fn deployment_signatures_need_an_official_or_prefix_bound_anthropic_reader() {
    let build = |official: bool, prefix: bool, api: ReaderApi| {
        let mut reader = reader(api, "anthropic", "claude-opus-5");
        reader.official_endpoint = official;
        reader.prefix_binding = prefix;
        Counter::new(Some(reader)).deployment_signatures()
    };
    assert!(build(true, false, ReaderApi::Anthropic));
    assert!(build(false, true, ReaderApi::Anthropic));
    assert!(!build(false, false, ReaderApi::Anthropic));
    assert!(!build(true, true, ReaderApi::OpenAiChat));
}

#[test]
fn every_bundled_encoding_counts_exactly() {
    use Encoding::*;
    for encoding in [
        O200k,
        ClaudeV3,
        ClaudeV47,
        ClaudeV5,
        ClaudeV5Sonnet,
        Qwen3,
        DeepSeekV3,
        KimiK2,
        Glm5,
    ] {
        assert!(
            crate::engines::count(encoding, "hello world").is_some(),
            "{encoding:?} must load: a corrupt bundled vocabulary is a build defect, not a runtime fallback"
        );
    }
}

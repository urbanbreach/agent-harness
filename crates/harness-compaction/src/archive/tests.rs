use super::*;

static HIGH_320: ArchiveShape = ArchiveShape {
    id: "11on16-bw",
    font: "8x13",
    cell_width: 11,
    cell_height: 16,
    line_repeat: 1,
    edge: 320,
    frame_token_estimate: 4761,
    verified: true,
    ink: Ink::Black,
    columns: 1,
    stopword_dim: false,
    stretch: false,
    detail: None,
    pricing: shapes::Pricing::AnthropicHigh,
};
static HIGH_1932: ArchiveShape = ArchiveShape {
    edge: 1932,
    ..HIGH_320
};
static SILVER_64: ArchiveShape = ArchiveShape {
    id: "silver16-bw",
    font: "silver",
    cell_width: 16,
    cell_height: 16,
    edge: 64,
    stretch: true,
    pricing: shapes::Pricing::Unknown,
    ..HIGH_320
};

#[test]
fn model_geometry_and_billing_follow_lineage_not_gateway() {
    for (model, api, explicit, edge, price, repeat) in [
        (
            "anthropic/claude-fable-5",
            "openai-completions",
            None,
            1932,
            4761,
            1,
        ),
        ("claude-mythos-5", "google-vertex", None, 1932, 4761, 1),
        (
            "claude-opus-4-10",
            "anthropic-messages",
            None,
            1932,
            4761,
            1,
        ),
        ("CLAUDE-OPUS-5", "anthropic-messages", None, 1932, 4761, 1),
        ("claude-opus-4-6", "anthropic-messages", None, 1568, 1521, 1),
        (
            "claude-fable",
            "anthropic-messages",
            Some("8x8r-bw"),
            1568,
            3136,
            2,
        ),
        ("claude-fable-5", "cursor", None, 1024, 1369, 1),
        ("gemini-3", "google-generative-ai", None, 2048, 1120, 1),
        (
            "gpt-6.1-codex",
            "openai-codex-responses",
            None,
            1568,
            2882,
            1,
        ),
        (
            "gpt-5.5",
            "openai-responses",
            Some("5x8-sent"),
            2576,
            7874,
            1,
        ),
    ] {
        let shape = resolve_shape(model, api, explicit);
        assert_eq!(
            (
                shape.edge,
                frame_tokens(shape, shape.edge, shape.edge, api),
                shape.line_repeat
            ),
            (edge, price, repeat),
            "{model}"
        );
    }
    for (model, expected) in [("gpt-6.1", 2882), ("gpt-6.2", 5024)] {
        assert_eq!(
            resolve_shape(model, "openai-responses", None).frame_token_estimate,
            expected
        );
    }
    for (model, width, height) in [
        ("kimi-k2", 8, 22),
        ("glm-5", 8, 16),
        ("claude-sonnet-4.7", 11, 16),
    ] {
        let shape = resolve_shape(model, "anthropic-messages", None);
        assert_eq!(
            (shape.cell_width, shape.cell_height, shape.edge),
            (width, height, 1568)
        );
    }
}

#[test]
fn capacities_and_pagination_preserve_cell_geometry() {
    let legacy = ArchiveShape {
        edge: 320,
        ..*resolve_shape("", "", Some("5x8-sent"))
    };
    let doc = ArchiveShape {
        edge: 160,
        ..*resolve_shape("", "", Some("doc-8on16-bw"))
    };
    for (shape, expected) in [(legacy, 2560), (HIGH_320, 580), (doc, 160)] {
        assert_eq!(capacity(&shape), expected);
    }
    assert_eq!(capacity(resolve_shape("", "", Some("doc-8on16-bw"))), 18816);
    for (text, width, expected) in [
        ("aa bb cc dd", 5, vec!["aa bb", "cc dd"]),
        ("abcdefghij", 4, vec!["abcd", "efgh", "ij"]),
    ] {
        assert_eq!(wrap_doc(text, width, "8x13"), expected);
    }
    let narrow = ArchiveShape {
        edge: 64,
        ..*resolve_shape("", "", Some("8on16-bw"))
    };
    for (count, pages) in [(16, 1), (17, 2)] {
        assert_eq!(paginate_grid(&"界".repeat(count), &narrow).len(), pages);
    }
    assert_eq!(
        paginate_grid(&format!("\u{e}{}\u{f}", "x".repeat(32)), &narrow).len(),
        1
    );
}

#[test]
fn threshold_room_matches_worked_examples_and_byte_caps() {
    for (window, threshold, kept, expected) in [
        (1_000_000, 160_000, 20_000, 11),
        (300_000, 160_000, 20_000, 11),
        (1_000_000, 80_000, 20_000, 1),
        (1_000_000, 60_000, 20_000, 1),
        (200_000, 160_000, 164_000, 1),
        (200_000, 160_000, 165_000, 0),
    ] {
        let input = FrameBudgetInput {
            window,
            reserve: None,
            nonmessage_tokens: 5000,
            kept_tokens: kept,
            kept_without_reasoning_tokens: kept,
            pending_tokens: 0,
            threshold,
            shape: HIGH_1932,
            byte_budget: 12_000_000,
            image_budget: 80,
        };
        assert_eq!(
            plan_frame_budget(&input),
            expected,
            "W={window},T={threshold},K={kept}"
        );
    }
    for (model, expected) in [("gpt-6.1", 15), ("gpt-6.2", 8)] {
        let input = FrameBudgetInput {
            window: 1_000_000,
            reserve: None,
            nonmessage_tokens: 0,
            kept_tokens: 17_998,
            kept_without_reasoning_tokens: 17_998,
            pending_tokens: 0,
            threshold: 120_000,
            shape: *resolve_shape(model, "openai-responses", None),
            byte_budget: 3_000_000,
            image_budget: 80,
        };
        assert_eq!(plan_frame_budget(&input), expected);
    }
    for (model, api, bytes, expected) in [
        ("claude-fable-5", "anthropic-messages", 3_000_000, 17),
        ("gpt-6.1", "openai-responses", 3_000_000, 26),
        ("gemini-3", "google-generative-ai", 3_000_000, 17),
        ("claude-fable-5", "anthropic-messages", 12_000_000, 70),
    ] {
        assert_eq!(
            byte_frame_cap(resolve_shape(model, api, None), bytes),
            expected
        );
    }
}

#[test]
fn unlisted_providers_resolve_from_the_reported_wire_not_their_name() {
    use harness_tokenizer::ReaderApi;
    let on = |provider, api| {
        let shape = resolve_shape_on("acme-9", provider, None, ShapeHost { api });
        (shape.id, shape.frame_token_estimate)
    };
    // No substring guessing: a name that merely contains a vendor is unknown without a wire.
    assert_eq!(on("my-anthropic-proxy", None), ("8on22-bw", 5024));
    assert_eq!(
        on("gateway", Some(ReaderApi::Anthropic)),
        ("11on16-bw", 5024)
    );
    assert_eq!(on("gateway", Some(ReaderApi::OpenAiChat)).0, "8on22-bw");
    // A listed provider keeps its catalog family whatever wire it reports.
    assert_eq!(on("anthropic", Some(ReaderApi::OpenAiChat)).0, "11on16-bw");
    // The host edge limit comes from the deployment table.
    assert_eq!(resolve_shape("claude-opus-5", "cursor", None).edge, 1024);
    assert_eq!(
        resolve_shape("claude-opus-5", "cursor-proxy", None).edge,
        1932
    );
}

#[test]
fn frame_size_override_reprices_the_square_by_lineage() {
    for (model, api, edge, raw) in [
        ("claude-fable-5", "anthropic-messages", 1568, 3136),
        ("claude-fable-5", "anthropic-messages", 2576, 4761),
        ("gpt-6.1", "openai-responses", 2576, 7874),
    ] {
        let resized = resolve_shape(model, api, None).with_frame_size(edge);
        assert_eq!(
            (
                resized.edge,
                resized.raw_frame_tokens(),
                resized.frame_token_estimate
            ),
            (edge, raw, raw.max(if resized.verified { 0 } else { 5024 })),
            "{model}"
        );
    }
}

#[test]
fn rescue_budget_has_no_single_frame_fallback() {
    // A tail that leaves no room for one full frame yields zero, not one.
    assert_eq!(
        rescue_frame_budget(&HIGH_1932, Some(40_000), 5000, 30_000, 12_000_000, 80),
        0
    );
    assert!(rescue_frame_budget(&HIGH_1932, Some(160_000), 5000, 20_000, 12_000_000, 80) > 1);
    assert_eq!(
        rescue_frame_budget(&HIGH_1932, None, 5000, 20_000, 3_000_000, 80),
        byte_frame_cap(&HIGH_1932, 3_000_000).min(80)
    );
}

#[test]
fn general_render_limits_keep_oldest_pages_and_short_frame_geometry() -> Result<(), ArchiveError> {
    let shape = ArchiveShape {
        edge: 64,
        ..*resolve_shape("", "", Some("8on16-bw"))
    };
    for (text, limit, expected) in [
        (" \n\t".to_owned(), None, 0),
        ("x".repeat(65), Some(0), 0),
        ("x".repeat(65), Some(2), 2),
        ("x".repeat(65), None, 3),
    ] {
        assert_eq!(render_many(&text, &shape, limit)?.len(), expected);
    }
    let shape = resolve_shape("", "", Some("8on22-bw"));
    for (text, height) in [
        ("x".repeat(83), 64),
        (
            format!("{}{}{}", "x".repeat(83), " ".repeat(113), "y".repeat(400)),
            88,
        ),
    ] {
        let frame = render_frame(&text, shape)?;
        assert_eq!((frame.width, frame.height), (1568, height));
    }
    assert_eq!(
        super::summary::short_summary(12_000, 1, 1000),
        "Archived 12,000 chars of history onto 1 archive frame (+1,000 chars as text)"
    );
    Ok(())
}

#[test]
fn foveated_source_can_be_reshrunk_without_losing_text_edges() -> Result<(), ArchiveError> {
    let text = format!(
        "HEAD SENTINEL {} TAIL SENTINEL",
        "Important fact number one. ".repeat(1000)
    );
    let initial = build_archive(&ArchivePlanInput {
        previous: None,
        new_text: text,
        frame_budget: 7,
        shape: HIGH_320,
        max_frames: 7,
    })?;
    assert_eq!(initial.frames.len(), 7);
    assert!(initial.lead_in.contains("29 or 40"));
    assert!(initial.source.truncated_chars > 0);
    assert!(initial.text_head.starts_with("HEAD SENTINEL"));
    assert!(initial.text_tail.ends_with("TAIL SENTINEL"));
    let shrunk = build_archive(&ArchivePlanInput {
        previous: Some(initial.source),
        new_text: String::new(),
        frame_budget: 3,
        shape: HIGH_320,
        max_frames: 3,
    })?;
    assert!(shrunk.frames.len() <= 3);
    assert!(shrunk.text_head.starts_with("HEAD SENTINEL"));
    assert!(shrunk.source.text.len() < 20_000);
    Ok(())
}

#[test]
fn text_only_guide_and_short_summary_have_no_compulsory_images() -> Result<(), ArchiveError> {
    for (text, summary) in [
        ("", "Archived 0 chars of history onto 0 archive frames"),
        (
            "hello",
            "Archived 5 chars of history onto 0 archive frames (+5 chars as text)",
        ),
    ] {
        let out = build_archive(&ArchivePlanInput {
            previous: None,
            new_text: text.into(),
            frame_budget: 1,
            shape: HIGH_320,
            max_frames: 0,
        })?;
        assert!(out.frames.is_empty());
        assert_eq!(out.short_summary, summary);
        assert_eq!(out.source.text, text);
    }
    let out = build_archive(&ArchivePlanInput {
        previous: None,
        new_text: "hello".into(),
        frame_budget: 1,
        shape: HIGH_320,
        max_frames: 1,
    })?;
    let mut files = crate::files::FileOps::default();
    files.read.insert("src/main.rs".into());
    assert!(lead_in_with_files(&out, &files).contains("FILES\n==================="));
    assert!(lead_in_with_files(&out, &files).ends_with("HISTORY\n==================="));
    Ok(())
}

#[test]
fn silver_cells_pagination_and_foveation_stay_in_silver() -> Result<(), Box<dyn std::error::Error>>
{
    let silver = &SILVER_64;
    let bitmap = ArchiveShape {
        edge: 64,
        ..*resolve_shape("", "", Some("8on16-bw"))
    };
    assert_eq!((capacity(silver), capacity(&bitmap)), (16, 32));
    for (shape, count, frames) in [
        (silver, 16, 1),
        (silver, 17, 2),
        (&bitmap, 16, 1),
        (&bitmap, 17, 2),
    ] {
        let text = "界".repeat(count);
        assert_eq!(paginate_grid(&text, shape).len(), frames);
        assert_eq!(render_many(&text, shape, None)?.len(), frames);
    }
    let archive = build_archive(&ArchivePlanInput {
        previous: None,
        new_text: "界".repeat(800),
        frame_budget: 1,
        shape: *silver,
        max_frames: 1,
    })?;
    assert_eq!(archive.frames.len(), 1);
    assert!(archive.source.truncated_chars > 0);
    for frame in &archive.frames {
        let reader = png::Decoder::new(std::io::Cursor::new(&frame.png)).read_info()?;
        assert_eq!(reader.info().color_type, png::ColorType::Rgb);
    }
    Ok(())
}

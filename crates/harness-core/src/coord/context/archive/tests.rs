use super::*;
use crate::attachment_transport::AttachmentMetadata;
use crate::coord::compaction::archive::reader::{fail_closed_view, reader_key, render_reader_view};
use crate::event::{ArchiveFrameRecord, ArchiveRecord};
use harness_compaction::{archive::has_reasoning_legend, CompactionMethod};
use harness_providers::MessageContentPart;
use std::{collections::HashMap, sync::Mutex};

/// Frame source that knows which artifacts exist and records every load.
struct Counting {
    present: HashMap<String, u64>,
    loads: Mutex<Vec<String>>,
}

impl Counting {
    fn new(frames: &[AttachmentMetadata], missing: &[usize]) -> Self {
        Self {
            present: frames
                .iter()
                .enumerate()
                .filter(|(index, _)| !missing.contains(index))
                .map(|(_, frame)| (frame.id.clone(), frame.size))
                .collect(),
            loads: Mutex::new(Vec::new()),
        }
    }

    fn loaded(&self) -> Vec<String> {
        self.loads.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

impl ArchiveFrameSource for Counting {
    fn size(&self, frame: &AttachmentMetadata) -> Option<u64> {
        self.present.get(&frame.id).copied()
    }

    fn load(&self, frame: &AttachmentMetadata) -> Option<AttachmentMetadata> {
        self.loads.lock().ok()?.push(frame.id.clone());
        Some(frame.clone())
    }
}

fn frames(sizes: &[u64]) -> Vec<AttachmentMetadata> {
    sizes
        .iter()
        .enumerate()
        .map(|(index, size)| {
            AttachmentMetadata::new(
                format!("f{index}"),
                "image/png",
                *size,
                None,
                "attachment:blake3:x",
            )
        })
        .collect()
}

fn text(head: &'static str, tail: &'static str, truncated: u64) -> ArchiveText<'static> {
    ArchiveText {
        guide: "guide",
        head,
        tail,
        truncated,
    }
}

fn ids(projection: &crate::coord::context::ArchiveProjection) -> Vec<&str> {
    projection
        .attachments
        .iter()
        .map(|a| a.id.as_str())
        .collect()
}

#[test]
fn frames_are_selected_newest_first_and_only_kept_frames_load() {
    // Costs in base64 bytes: 4, 8, 4.
    for (name, sizes, missing, limits, kept, loaded, notice) in [
        (
            "budget skips the oversized middle",
            vec![3, 6, 3],
            vec![],
            FrameLimits { bytes: 8, images: 80 },
            vec!["f0", "f2"],
            vec!["f2", "f0"],
            "1 archived image frame could not be included. 8 B of base64 exceeded the per-request archive payload budget.",
        ),
        (
            "missing artifacts coalesce into one chronological slot",
            vec![3, 3, 3],
            vec![1, 2],
            FrameLimits::transcript(),
            vec!["f0"],
            vec!["f0"],
            "2 archived image frames unavailable here",
        ),
        (
            "the image cap keeps the newest",
            vec![3, 3, 3],
            vec![],
            FrameLimits { bytes: u64::MAX, images: 1 },
            vec!["f2"],
            vec!["f2"],
            "2 archived image frames could not be included. The compacted summary",
        ),
    ] {
        let list = frames(&sizes);
        let source = Counting::new(&list, &missing);
        let projection = archive_projection(&text("head", "tail", 0), &list, list.len(), Some(&source), limits);
        assert_eq!(ids(&projection), kept, "{name}");
        assert_eq!(source.loaded(), loaded, "{name}");
        assert!(projection.text.contains(notice), "{name}: {}", projection.text);
        assert!(projection.text.contains("imaged middle below"), "{name}");
    }
}

#[test]
fn without_a_source_unloaded_frames_are_unavailable() {
    let list = frames(&[3]);
    let projection =
        archive_projection(&text("", "", 0), &list, 1, None, FrameLimits::transcript());
    assert!(projection.attachments.is_empty());
    assert!(projection
        .text
        .contains("1 archived image frame unavailable here"));
    assert!(projection.text.contains("middle history omitted above"));
}

fn frame_record(modern: bool, bytes: u64) -> ArchiveFrameRecord {
    ArchiveFrameRecord {
        path: "artifacts/a.attachment".into(),
        digest: "a".repeat(64),
        bytes,
        width: 8,
        height: 8,
        shape: modern.then(|| "shape".to_owned()),
        ..Default::default()
    }
}

fn record(frames: usize, modern: bool, bytes: u64, truncated_chars: u64) -> ArchiveRecord {
    ArchiveRecord {
        text_head: "head".into(),
        truncated_chars,
        frames: vec![frame_record(modern, bytes); frames],
        ..Default::default()
    }
}

#[test]
fn legacy_guard_omits_provider_frames_but_not_modern_or_transcript() {
    for (name, record, risky) in [
        ("16 legacy frames", record(16, false, 10, 0), true),
        ("16 modern frames", record(16, true, 10, 0), false),
        ("15 small legacy frames", record(15, false, 10, 0), false),
        (
            "large legacy bytes, huge truncation",
            record(3, false, 2_500_000, 1_000_000),
            true,
        ),
        (
            "large legacy bytes, small source",
            record(3, false, 2_500_000, 0),
            false,
        ),
    ] {
        let provider = FrameLimits::provider(&record, 3_000_000, 90);
        assert_eq!(provider.bytes == 0, risky, "{name}");
        let list = frames(&vec![10; record.frames.len()]);
        let source = Counting::new(&list, &[]);
        let sent = archive_projection(
            &text("h", "t", 0),
            &list,
            list.len(),
            Some(&source),
            provider,
        );
        let shown = archive_projection(
            &text("h", "t", 0),
            &list,
            list.len(),
            Some(&source),
            FrameLimits::transcript(),
        );
        assert_eq!(
            sent.attachments.is_empty(),
            risky && !record.frames.is_empty(),
            "{name}"
        );
        assert_eq!(shown.attachments.len(), record.frames.len(), "{name}");
    }
}

#[test]
fn tolerant_import_drops_malformed_frames_and_keeps_text() {
    let good = frame_record(true, 10);
    for (name, input, expected) in [
        (
            "malformed frames dropped, counters defaulted",
            ArchiveRecord {
                text_head: "kept".into(),
                frames: vec![
                    good.clone(),
                    ArchiveFrameRecord {
                        digest: String::new(),
                        ..good.clone()
                    },
                    ArchiveFrameRecord {
                        path: String::new(),
                        ..good.clone()
                    },
                    ArchiveFrameRecord {
                        bytes: 0,
                        ..good.clone()
                    },
                    ArchiveFrameRecord {
                        width: 0,
                        ..good.clone()
                    },
                ],
                ..Default::default()
            },
            Some((1, 4)),
        ),
        (
            "text only survives",
            ArchiveRecord {
                text_tail: "tail".into(),
                ..Default::default()
            },
            Some((0, 4)),
        ),
        (
            "frames only survive",
            ArchiveRecord {
                frames: vec![good.clone()],
                ..Default::default()
            },
            Some((1, 0)),
        ),
        (
            "nothing usable is invalid",
            ArchiveRecord {
                frames: vec![ArchiveFrameRecord::default()],
                ..Default::default()
            },
            None,
        ),
    ] {
        let imported = import_archive(input).map(|r| (r.frames.len(), r.chars));
        assert_eq!(imported, expected, "{name}");
    }
}

fn state(summary: &str, archive: ArchiveRecord) -> CompactionState {
    CompactionState {
        summary: summary.to_owned(),
        short_summary: None,
        method: CompactionMethod::Archive,
        tokens_before: 0,
        tokens_after: None,
        read_files: Vec::new(),
        modified_files: Vec::new(),
        warning: None,
        archive: Some(archive),
        native: None,
        from_hook: false,
        details: None,
        preserve_data: Default::default(),
        commit_seq: 0,
        history_rewrite_at_ms: None,
    }
}

#[test]
fn collapsed_summary_has_no_blocks_while_full_keeps_order() {
    let mut archive = record(1, true, 3, 0);
    archive.text_tail = "tail".into();
    let state = state("guide", archive.clone());
    assert_eq!(collapsed_text(&state), "guide");
    let full = summary_text(&state);
    let head = full.find("head").expect("head");
    let marker = full.find("imaged middle below").expect("marker");
    let tail = full.find("tail").expect("tail");
    assert!(head < marker && marker < tail);
    let list = frames(&[3]);
    let source = Counting::new(&list, &[]);
    let projection = archive_projection(
        &ArchiveText {
            guide: "guide",
            head: "head",
            tail: "tail",
            truncated: 0,
        },
        &list,
        1,
        Some(&source),
        FrameLimits::transcript(),
    );
    let order: Vec<_> = projection
        .parts
        .iter()
        .map(|p| matches!(p, MessageContentPart::Text(_)))
        .collect();
    assert_eq!(order, [true, false, true]);
}

fn reasoning_state() -> CompactionState {
    let mut archive = record(1, true, 3, 0);
    archive.text = "¶user:Public request█¶think:secret plan█¶ai:Public answer".into();
    archive.text_head = archive.text.clone();
    state(
        "Scopes: ¶user: input.\n¶think: assistant reasoning.\n",
        archive,
    )
}

#[test]
fn reader_views_scrub_reasoning_and_never_reuse_old_pixels() {
    let state = reasoning_state();
    let snapshot = (state.summary.clone(), state.archive.clone());
    assert!(has_reasoning_legend(&state.summary));
    let shape = harness_compaction::archive::resolve_shape("claude-opus-4-5", "anthropic", None);
    let rendered = render_reader_view(&state, shape);
    assert!(!rendered.fail_closed);
    assert!(!has_reasoning_legend(&rendered.summary));
    assert!(!rendered.archive.text_head.contains("secret"));
    assert!(rendered.archive.text_head.contains("Public answer"));

    // A shape that cannot render fails closed: scrubbed text, no frames, no invented variant.
    let mut broken = *shape;
    broken.edge = 0;
    let closed = render_reader_view(&state, &broken);
    assert!(closed.fail_closed && closed.frames.is_empty() && closed.archive.frames.is_empty());
    let all = format!(
        "{}{}{}",
        closed.summary, closed.archive.text_head, closed.archive.text_tail
    );
    assert!(!all.contains("secret") && !has_reasoning_legend(&closed.summary));
    assert!(all.contains("Public request"));

    // The original is never mutated; the reader key tracks the whole priced shape.
    assert_eq!(snapshot, (state.summary.clone(), state.archive.clone()));
    let mut repriced = *shape;
    repriced.frame_token_estimate += 1;
    assert_ne!(reader_key(shape), reader_key(&repriced));
    let direct = fail_closed_view(&state, state.archive.as_ref().expect("archive"), "x".into());
    assert!(direct.fail_closed);
}

#[test]
fn seeded_reader_views_resolve_frames_lazily_and_newest_patch_wins() {
    let caches = RenderCaches::default();
    let mut persisted = record(1, true, 3, 0);
    persisted.text_head = "scrubbed".into();
    caches.seed_reader_view(7, "key", "old".into(), Some(persisted.clone()));
    caches.seed_reader_view(7, "key", "new".into(), Some(persisted));
    let view = caches.reader_views().ready(7, "key").expect("seeded");
    assert_eq!(view.summary, "new");
    assert_eq!(view.frames.len(), 1);
    assert!(view.frames[0].content_ref.starts_with("attachment:blake3:"));
    assert!(caches.reader_views().ready(7, "other").is_none());
    assert!(caches.reader_views().ready(8, "key").is_none());
}

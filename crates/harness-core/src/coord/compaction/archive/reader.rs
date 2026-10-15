//! Reasoning-free reader views: rendering, once-per-key preparation and durable patches.
use super::*;
use crate::coord::context::archive::{compaction_seq, needs_reader_view, ReaderView};
use crate::coord::runtime::Runtime;
use crate::event::{CompactionPatch, CompactionPatchedEvent, EventV1};
use harness_compaction::archive::ReaderPolicy;
use std::collections::BTreeMap;

/// Stable key of the complete reader policy and the resolved shape, including the model's
/// frame price, so a shape id shared by differently priced models never reuses a view.
pub(in crate::coord) fn reader_key(shape: &archive::ArchiveShape) -> String {
    format!(
        "{}:{}:{}:{}:no-reasoning",
        shape.id, shape.edge, shape.frame_token_estimate, shape.verified
    )
}

fn units(text: &str) -> u64 {
    text.encode_utf16().count() as u64
}

fn file_ops(state: &CompactionState) -> harness_compaction::files::FileOps {
    harness_compaction::files::FileOps {
        read: state.read_files.iter().cloned().collect(),
        edited: state.modified_files.iter().cloned().collect(),
        ..Default::default()
    }
}

/// Renders the reasoning-free view of an archive summary. Old pixels cannot be scrubbed,
/// so the source is cleaned first and re-rendered with the original frame count; when
/// there are no frames to render or rendering fails, the view is text only.
pub(in crate::coord) fn render_reader_view(
    state: &CompactionState,
    shape: &archive::ArchiveShape,
) -> ReaderView {
    let empty;
    let record = match &state.archive {
        Some(record) => record,
        None => {
            empty = ArchiveRecord::default();
            &empty
        }
    };
    let clean = archive_source(record, true);
    let count = u32::try_from(record.frames.len()).unwrap_or(u32::MAX);
    if count > 0 {
        let input = ArchivePlanInput {
            previous: Some(ArchiveSource {
                text: clean.clone(),
                truncated_chars: record.truncated_chars,
            }),
            new_text: String::new(),
            frame_budget: count,
            max_frames: count,
            shape: *shape,
        };
        if let Ok(output) = archive::build_archive(&input) {
            let (frames, archive) = materialize(&output, shape);
            return ReaderView {
                summary: archive::lead_in_with_files(&output, &file_ops(state)),
                archive,
                frames,
                fail_closed: false,
            };
        }
    }
    fail_closed_view(state, record, clean)
}

/// Text-only view: no frames, source scrubbed before choosing the text edges with the
/// original edge budgets, the dropped middle counted as extra truncated characters, and a
/// guide without the reasoning legend.
pub(in crate::coord) fn fail_closed_view(
    state: &CompactionState,
    record: &ArchiveRecord,
    clean: String,
) -> ReaderView {
    let source: Vec<u16> = clean.encode_utf16().collect();
    let head_budget = record.text_head.encode_utf16().count();
    let tail_budget = record.text_tail.encode_utf16().count();
    let (head, tail, dropped) = if source.len() <= head_budget.saturating_add(tail_budget) {
        (clean, String::new(), 0)
    } else {
        (
            String::from_utf16_lossy(&source[..head_budget]),
            String::from_utf16_lossy(&source[source.len() - tail_budget..]),
            (source.len() - head_budget - tail_budget) as u64,
        )
    };
    let truncated_chars = record.truncated_chars.saturating_add(dropped);
    let output = archive::ArchiveOutput::text_only(head, tail, truncated_chars);
    ReaderView {
        summary: archive::lead_in_with_files(&output, &file_ops(state)),
        archive: ArchiveRecord {
            chars: units(&output.text_head).saturating_add(units(&output.text_tail)),
            text: String::new(),
            text_head: output.text_head,
            text_tail: output.text_tail,
            truncated_chars,
            shape: record.shape.clone(),
            frames: Vec::new(),
        },
        frames: Vec::new(),
        fail_closed: true,
    }
}

impl Worker {
    /// The reader's reasoning policy, the shape it renders with and the key of its views.
    pub(in crate::coord) fn reader_identity(
        &self,
        model: &crate::agent::AgentModelRef,
    ) -> (ReaderPolicy, &'static archive::ArchiveShape, String) {
        let shape = self.shape_for(&model.provider_id, &model.model_id);
        (
            ReaderPolicy::for_model(&model.provider_id, &model.model_id),
            shape,
            reader_key(shape),
        )
    }

    /// Renders and persists, once per compaction and reader key, the reasoning-free view of
    /// every reasoning-bearing archive summary the active reader must not see. Rendering
    /// runs off the request thread; concurrent callers share one render. `Worker::request`
    /// only reads the result.
    pub(in crate::coord) async fn prepare_reader_views(
        &self,
        context: &Context,
    ) -> Result<(), CoordinatorError> {
        let model = crate::agent::AgentModelRef::parse(&self.turn.model);
        let (policy, shape, key) = self.reader_identity(&model);
        if !policy.excludes_archived_reasoning {
            return Ok(());
        }
        for entry in &context.entries {
            let Some(state) = entry
                .compaction
                .as_ref()
                .filter(|state| needs_reader_view(state, policy))
            else {
                continue;
            };
            let seq = compaction_seq(entry, state);
            if seq == 0 {
                continue;
            }
            let cell = context.render_caches.reader_views().cell(seq, &key);
            cell.get_or_init(|| self.reader_view(Arc::clone(state), seq, &key, *shape))
                .await;
        }
        Ok(())
    }

    async fn reader_view(
        &self,
        state: Arc<CompactionState>,
        seq: u64,
        key: &str,
        shape: archive::ArchiveShape,
    ) -> Arc<ReaderView> {
        let rendering = Arc::clone(&state);
        let view = match tokio::task::spawn_blocking(move || render_reader_view(&rendering, &shape))
            .await
        {
            Ok(view) => view,
            Err(_) => {
                let clean = state
                    .archive
                    .as_ref()
                    .map(|record| archive_source(record, true))
                    .unwrap_or_default();
                let record = state.archive.clone().unwrap_or_default();
                fail_closed_view(&state, &record, clean)
            }
        };
        if !view.fail_closed {
            let (actor, key) = (self.actor.clone(), key.to_owned());
            let (summary, archive, frames) = (
                view.summary.clone(),
                view.archive.clone(),
                view.frames.clone(),
            );
            let persisted = self
                .handle
                .call(move |s| s.persist_reader_view(&actor, seq, key, summary, archive, &frames))
                .await;
            if let Err(error) = persisted {
                // The view stays usable in memory; a resume retries the render.
                let _ = self
                    .warn(format!(
                        "Persisting the archive without reasoning failed: {error}"
                    ))
                    .await;
            }
        }
        Arc::new(view)
    }
}

impl Runtime {
    /// Appends the reasoning-free view as an amendment of its compaction. Frame PNGs are
    /// written first as artifacts, then the patch event is appended; the original summary,
    /// archive and frames are untouched.
    pub(in crate::coord) fn persist_reader_view(
        &mut self,
        actor: &EventActor,
        compaction_seq: u64,
        reader_key: String,
        summary: String,
        mut archive: ArchiveRecord,
        frames: &[AttachmentMetadata],
    ) -> Result<(), CoordinatorError> {
        let agent_id = actor
            .agent_id
            .clone()
            .ok_or_else(|| CoordinatorError::Invalid("reader view needs an agent".into()))?;
        if frames.len() != archive.frames.len() {
            return Err(CoordinatorError::Invalid(
                "reader view frames do not match its archive".into(),
            ));
        }
        let mut records = Vec::with_capacity(frames.len());
        for (frame, record) in frames.iter().zip(&archive.frames) {
            let bytes = frame
                .bytes()
                .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
            let artifact = self.write_blob(actor, None, "attachment", bytes, BTreeMap::new())?;
            records.push(ArchiveFrameRecord {
                path: artifact.path,
                digest: artifact.digest,
                ..record.clone()
            });
        }
        archive.frames = records;
        self.emit(
            actor.clone(),
            None,
            EventV1::CompactionPatched(CompactionPatchedEvent {
                agent_id,
                compaction_seq,
                patch: CompactionPatch::ReaderView {
                    reader_key,
                    summary,
                    archive: Some(archive),
                },
            }),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coord::context::archive::{ArchiveFrameSource, RenderCaches, RunDirFrames};
    use crate::{clock::FakeClock, redact::DefaultRedactor};

    const PNG: [u8; 67] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0,
        0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0x0d, 0x49, 0x44,
        0x41, 0x54, 0x78, 0x9c, 0x63, 0, 1, 0, 0, 5, 0, 1, 0x0d, 0x0a, 0x2d, 0xb4, 0, 0, 0, 0,
        0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    #[tokio::test]
    async fn persisted_view_is_a_durable_patch_that_replays_and_resolves_lazily(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let coordinator = spawn_coordinator(
            CoordinatorConfig::new(temp.path().join("sessions")),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator.start_run("reader", temp.path()).await?;
        let frame = AttachmentMetadata::from_bytes(
            "view-frame",
            "image/png",
            None,
            &PNG,
            Some(crate::attachment_transport::AttachmentDimensions::new(1, 1)),
        );
        let record = ArchiveRecord {
            text_head: "scrubbed".into(),
            frames: vec![ArchiveFrameRecord {
                bytes: PNG.len() as u64,
                width: 1,
                height: 1,
                shape: Some("shape".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let actor = EventActor::new(ActorKind::Worker, Some("agent".into()));
        let frames = vec![frame];
        coordinator
            .call(move |s| {
                s.persist_reader_view(&actor, 9, "key".into(), "view".into(), record, &frames)
            })
            .await?;
        coordinator.stop_run().await?;
        let events = crate::store::read_events(&run.events_path)?;
        let artifact = events
            .iter()
            .position(|e| matches!(e.payload, EventV1::ArtifactWritten(_)));
        let patch = events
            .iter()
            .position(|e| matches!(e.payload, EventV1::CompactionPatched(_)));
        assert!(
            artifact < patch,
            "frames are written before the patch is appended"
        );
        let Some(EventV1::CompactionPatched(patched)) = patch.map(|i| &events[i].payload) else {
            return Err("no reader view patch".into());
        };
        assert_eq!(patched.compaction_seq, 9);
        let CompactionPatch::ReaderView {
            reader_key,
            summary,
            archive,
        } = &patched.patch
        else {
            return Err("not a reader view".into());
        };
        let archive = archive.clone().ok_or("patch without archive")?;
        assert!(!archive.frames[0].path.is_empty() && !archive.frames[0].digest.is_empty());

        let caches = RenderCaches::default();
        caches.seed_reader_view(9, reader_key, summary.clone(), Some(archive));
        let view = caches
            .reader_views()
            .ready(9, reader_key)
            .ok_or("not seeded")?;
        assert!(
            view.frames[0].bytes().is_err(),
            "replayed frames carry no bytes"
        );
        let source = RunDirFrames::new(run.run_dir.clone());
        let loaded = source.load(&view.frames[0]).ok_or("frame did not load")?;
        assert_eq!(loaded.bytes()?, &PNG);
        Ok(())
    }
}

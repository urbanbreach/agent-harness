use harness_core::{
    event::{EventEnvelopeV1, EventV1},
    redact::Redactor,
    store::JournalReader,
};
use std::{collections::HashMap, io::Write, path::Path};

/// Exports settled JSONL with contiguous sequences and equivalent history boundaries.
pub(crate) fn export(source: &Path, output: &Path, redactor: &dyn Redactor) -> Result<(), String> {
    let output = super::destination(output, source.parent().ok_or("journal parent missing")?)?;
    let parent = output.parent().ok_or("output parent missing")?;
    let length = harness_core::store::open_private_file(source)
        .and_then(|f| f.metadata())
        .map_err(|e| e.to_string())?
        .len();
    if length > 64 * 1024 * 1024 {
        return Err("session journal exceeds the 64 MiB export limit".into());
    }
    let events = JournalReader::open(source, length).map_err(|e| e.to_string())?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    let mut pending = HashMap::new();
    let mut kept = Vec::new();
    for event in events {
        let mut event = event.map_err(|e| e.to_string())?;
        if !super::clean_event(&mut event, &mut pending) {
            continue;
        }
        kept.push(event.seq);
        remap(&mut event, &kept);
        let mut value = serde_json::to_value(event).map_err(|e| e.to_string())?;
        super::check_value(&mut value, redactor)?;
        serde_json::to_writer(&mut temporary, &value).map_err(|e| e.to_string())?;
        writeln!(temporary).map_err(|e| e.to_string())?;
    }
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary.persist(&output).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    std::fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn remap(event: &mut EventEnvelopeV1, kept: &[u64]) {
    let through = |seq| kept.partition_point(|old| *old <= seq) as u64;
    let from = |seq| kept.partition_point(|old| *old < seq) as u64 + 1;
    event.seq = kept.len() as u64;
    match &mut event.payload {
        EventV1::ConversationRewound(e) => e.target_seq = from(e.target_seq),
        EventV1::SessionCompaction(e) => e.first_kept_event_seq = from(e.first_kept_event_seq),
        EventV1::BranchSummary(e) => e.from_event_seq = through(e.from_event_seq),
        EventV1::CompactionRequested(e) => e.through_seq = through(e.through_seq),
        EventV1::CompactionWritten(e) => e.through_seq = through(e.through_seq),
        EventV1::CompactionApplied(e) => e.through_seq = through(e.through_seq),
        EventV1::CompactionFailed(e) => e.through_seq = e.through_seq.map(through),
        _ => {}
    }
}

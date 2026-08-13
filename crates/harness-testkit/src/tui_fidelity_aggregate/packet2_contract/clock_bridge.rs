use super::{External, Native, PresentationLink, TypeWindow};

pub(super) fn linked_type_observations(
    external: &External,
    links: &[PresentationLink],
    native: &Native,
    windows: &[TypeWindow],
) -> Vec<u64> {
    let read_starts = external
        .raw_reads
        .iter()
        .scan(0_u64, |offset, read| {
            let start = *offset;
            *offset = offset.saturating_add(read.byte_len);
            Some(start)
        })
        .collect::<Vec<_>>();
    external
        .observations
        .iter()
        .filter(|observation| {
            observation.raw_read_ordinals.iter().any(|ordinal| {
                external.raw_reads.get(*ordinal).is_some_and(|read| {
                    let start = read_starts.get(*ordinal).copied().unwrap_or(u64::MAX);
                    let end = start.saturating_add(read.byte_len);
                    links
                        .iter()
                        .filter(|link| {
                            link.byte_sha256 == read.sha256
                                && link.stream_offset >= start
                                && link.stream_offset < end
                        })
                        .any(|link| frame_is_typed(link.frame_sequence, native, windows))
                })
            })
        })
        .map(|observation| observation.observed_at)
        .collect()
}

fn frame_is_typed(sequence: u64, native: &Native, windows: &[TypeWindow]) -> bool {
    native
        .frames
        .iter()
        .find(|frame| frame.sequence == sequence)
        .is_some_and(|frame| {
            frame.cause_ids.iter().any(|id| {
                native.causes.iter().any(|cause| {
                    cause.cause_id == *id
                        && cause.kind == "terminal_input"
                        && windows.iter().any(|window| {
                            cause.received_at >= window.start && cause.received_at < window.end
                        })
                })
            })
        })
}

use super::*;

pub(super) fn apply_compaction_policy(spec: &mut TranscriptBlockSpec) {
    if !matches!(spec.content, TranscriptBlockContent::Compaction { .. }) {
        return;
    }
    spec.chrome = TranscriptBlockChrome {
        accent: true,
        rail: false,
    };
    spec.fold = TranscriptBlockFold {
        foldable: true,
        expanded: false,
    };
    spec.interaction = TranscriptBlockInteraction {
        selectable: false,
        selected: false,
        hoverable: false,
        focusable: false,
    };
    spec.disclosure = TranscriptBlockDisclosure {
        available: true,
        expanded: false,
    };
    spec.compact = TranscriptBlockCompactPolicy::ElideDetails;
    spec.motion = TranscriptBlockMotionDemand::None;
}

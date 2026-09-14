use super::*;

pub(super) fn apply_compaction_policy(spec: &mut TranscriptBlockSpec) {
    let TranscriptBlockContent::Compaction { expanded, .. } = spec.content else {
        return;
    };
    spec.chrome = TranscriptBlockChrome {
        accent: true,
        rail: false,
    };
    spec.fold = TranscriptBlockFold {
        foldable: true,
        expanded,
    };
    spec.interaction = TranscriptBlockInteraction {
        selectable: false,
        selected: false,
        hoverable: false,
        focusable: false,
    };
    spec.disclosure = TranscriptBlockDisclosure {
        available: true,
        expanded,
    };
    spec.compact = TranscriptBlockCompactPolicy::ElideDetails;
    spec.motion = TranscriptBlockMotionDemand::None;
}

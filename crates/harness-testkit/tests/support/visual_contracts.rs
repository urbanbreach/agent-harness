#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct OfflineVisualEvidenceContract {
    pub(crate) family: &'static str,
    pub(crate) state: &'static str,
    pub(crate) png: &'static str,
    pub(crate) snapshot: &'static str,
}

pub(crate) const OFFLINE_VISUAL_EVIDENCE_CONTRACTS: &[OfflineVisualEvidenceContract] = &[
    OfflineVisualEvidenceContract {
        family: "startup_shell",
        state: "happy_path",
        png: "pty_startup_home_primary.png",
        snapshot: "pty_startup_home_primary",
    },
    OfflineVisualEvidenceContract {
        family: "startup_shell",
        state: "happy_path",
        png: "pty_startup_home_dense.png",
        snapshot: "pty_startup_home_dense",
    },
    OfflineVisualEvidenceContract {
        family: "startup_command_palette",
        state: "happy_path",
        png: "pty_startup_command_palette.png",
        snapshot: "pty_startup_command_palette",
    },
    OfflineVisualEvidenceContract {
        family: "startup_session_history",
        state: "continue_history",
        png: "pty_startup_continue_history.png",
        snapshot: "pty_startup_continue_history",
    },
    OfflineVisualEvidenceContract {
        family: "startup_session_history",
        state: "replay_history",
        png: "pty_startup_replay_history.png",
        snapshot: "pty_startup_replay_history",
    },
    OfflineVisualEvidenceContract {
        family: "continue_session",
        state: "happy_path",
        png: "pty_continue_quiescent_session.png",
        snapshot: "pty_continue_quiescent_session",
    },
    OfflineVisualEvidenceContract {
        family: "continue_session",
        state: "failure_path",
        png: "pty_continue_rejected_active.png",
        snapshot: "pty_continue_rejected_active",
    },
    OfflineVisualEvidenceContract {
        family: "continue_session",
        state: "failure_path",
        png: "pty_continue_rejected_unrestorable.png",
        snapshot: "pty_continue_rejected_unrestorable",
    },
    OfflineVisualEvidenceContract {
        family: "permission",
        state: "happy_path",
        png: "pty_permission_overlay_parity.png",
        snapshot: "pty_permission_overlay_parity",
    },
    OfflineVisualEvidenceContract {
        family: "live_shell",
        state: "happy_path",
        png: "pty_interactive_type_first_startup.png",
        snapshot: "pty_interactive_type_first_startup",
    },
    OfflineVisualEvidenceContract {
        family: "live_shell",
        state: "happy_path",
        png: "pty_interactive_prompt_stream.png",
        snapshot: "pty_interactive_prompt_stream",
    },
    OfflineVisualEvidenceContract {
        family: "live_shell",
        state: "happy_path",
        png: "pty_session_shell_primary_live.png",
        snapshot: "pty_session_shell_primary_live",
    },
    OfflineVisualEvidenceContract {
        family: "live_shell",
        state: "inline_completion",
        png: "pty_inline_completion_shell.png",
        snapshot: "pty_inline_completion_shell",
    },
    OfflineVisualEvidenceContract {
        family: "replay_shell",
        state: "happy_path",
        png: "pty_session_shell_primary_replay.png",
        snapshot: "pty_session_shell_primary_replay",
    },
    OfflineVisualEvidenceContract {
        family: "transcript_shell",
        state: "happy_path",
        png: "pty_session_transcript_rich_shell.png",
        snapshot: "pty_session_transcript_rich_shell",
    },
    OfflineVisualEvidenceContract {
        family: "operator_sidebar",
        state: "happy_path",
        png: "pty_operator_sidebar_primary.png",
        snapshot: "pty_operator_sidebar_primary",
    },
    OfflineVisualEvidenceContract {
        family: "replay",
        state: "failure_path",
        png: "pty_replay_read_only.png",
        snapshot: "pty_replay_read_only",
    },
];

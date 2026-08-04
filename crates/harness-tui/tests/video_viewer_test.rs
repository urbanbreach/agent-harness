#[path = "../src/video_viewer/mod.rs"]
mod video_viewer;

use video_viewer::{
    lifecycle::{ViewerError, ViewerPhase, ViewerState},
    progress::{FramePacing, PlaybackProgress},
    subprocess::{SubprocessDescriptor, SubprocessReceipt, SubprocessSupervisor, sanitize_args},
};

fn descriptor(binary: &str) -> SubprocessDescriptor {
    SubprocessDescriptor {
        binary: binary.to_owned(),
        args: vec!["-i".to_owned(), "clip.mp4".to_owned()],
        max_duration_ms: 10_000,
        max_width: 1920,
        max_height: 1080,
    }
}

#[test]
fn progress_tracks_fraction_and_eta() {
    let mut progress = PlaybackProgress::new(10, 1_000);
    assert_eq!(progress.fraction_complete(), 0);
    assert_eq!(progress.eta_ms(), 1_000);
    progress.advance(4, 300);
    assert_eq!(progress.fraction_complete(), 40);
    assert_eq!(progress.eta_ms(), 700);
    progress.advance(99, 999);
    assert_eq!(progress.fraction_complete(), 100);
    assert!(progress.is_complete());
}

#[test]
fn zero_frame_progress_is_complete_without_fraction() {
    let progress = PlaybackProgress::new(0, 500);
    assert_eq!(progress.fraction_complete(), 0);
    assert!(progress.is_complete());
}

#[test]
fn frame_pacing_uses_safe_intervals_and_width_breakpoints() {
    assert_eq!(FramePacing::from_fps(30).frame_interval_ms, 33);
    assert_eq!(FramePacing::from_fps(0).frame_interval_ms, 1000);
    assert_eq!(FramePacing::for_width(80).target_fps, 30);
    assert_eq!(FramePacing::for_width(132).target_fps, 24);
    assert_eq!(FramePacing::for_width(200).target_fps, 15);
}

#[test]
fn sanitize_args_rejects_shell_metacharacters_and_empty_values() {
    for value in [";", "|", "&", "$HOME", "`id`", "\n", "\r", ">", "<", ""] {
        assert!(sanitize_args(&[value.to_owned()]).is_err());
    }
    assert_eq!(
        sanitize_args(&["-i".to_owned(), "clip.mp4".to_owned()]).ok(),
        Some(vec!["-i".to_owned(), "clip.mp4".to_owned()])
    );
}

#[test]
fn descriptor_validation_enforces_binary_and_media_bounds() {
    assert!(descriptor("ffmpeg").validate().is_ok());
    assert!(descriptor("ffprobe").validate().is_ok());
    assert_eq!(descriptor("sh").validate(), Err(ViewerError::UnknownBinary));
    let mut oversized = descriptor("ffmpeg");
    oversized.max_duration_ms = 600_001;
    assert_eq!(oversized.validate(), Err(ViewerError::OversizedMedia));
    oversized.max_duration_ms = 10_000;
    oversized.max_width = 7681;
    assert_eq!(oversized.validate(), Err(ViewerError::OversizedMedia));
}

#[test]
fn receipt_cleanup_requires_all_resources_to_be_reclaimed() {
    let descriptor = descriptor("ffmpeg");
    let clean = SubprocessReceipt {
        descriptor: descriptor.clone(),
        exit_code: Some(0),
        completed_normally: true,
        temp_files_created: vec!["a".to_owned()],
        temp_files_removed: vec!["a".to_owned()],
        child_pids_observed: vec![7],
        child_pids_reaped: vec![7],
    };
    assert!(clean.cleanup_complete());
    let dirty = SubprocessReceipt {
        temp_files_removed: Vec::new(),
        child_pids_reaped: Vec::new(),
        ..clean
    };
    assert!(!dirty.cleanup_complete());
}

#[test]
fn supervisor_validates_and_simulates_cleanup() {
    let mut supervisor = SubprocessSupervisor::new();
    assert!(supervisor.is_empty());
    assert_eq!(supervisor.submit(descriptor("ffmpeg")).ok(), Some(0));
    assert_eq!(supervisor.len(), 1);
    assert!(
        supervisor
            .simulate_run(0, Some(0))
            .ok()
            .is_some_and(|receipt| receipt.cleanup_complete())
    );
    assert!(
        supervisor
            .simulate_run(0, Some(1))
            .ok()
            .is_some_and(|receipt| !receipt.cleanup_complete())
    );
    assert_eq!(
        supervisor.simulate_run(1, Some(0)),
        Err(ViewerError::UnknownRequest)
    );
}

#[test]
fn viewer_state_follows_normal_lifecycle_and_completion() {
    let mut viewer = ViewerState::default();
    assert_eq!(viewer.phase(), &ViewerPhase::Idle);
    assert!(viewer.open(descriptor("ffmpeg")).is_ok());
    assert!(matches!(viewer.phase(), ViewerPhase::Opening(_)));
    viewer.advance_to_decoding();
    viewer.start_playback(10, 1_000);
    viewer.tick_playback(4, 400);
    assert!(matches!(viewer.phase(), ViewerPhase::Playing { .. }));
    viewer.tick_playback(6, 600);
    assert_eq!(viewer.phase(), &ViewerPhase::Closed);
}

#[test]
fn viewer_cancel_error_and_cleanup_verification_are_observable() {
    let mut viewer = ViewerState::default();
    viewer.open(descriptor("ffmpeg")).ok();
    viewer.cancel();
    assert_eq!(viewer.phase(), &ViewerPhase::Closed);
    assert!(viewer.is_cancelled());
    viewer.report_error("decode failed".to_owned());
    assert_eq!(
        viewer.phase(),
        &ViewerPhase::Error("decode failed".to_owned())
    );
    assert!(viewer.cleanup_verified());
    let mut supervisor = SubprocessSupervisor::new();
    supervisor.submit(descriptor("ffmpeg")).ok();
    let dirty = supervisor.simulate_run(0, Some(1)).ok();
    if let Some(receipt) = dirty {
        viewer.close(receipt);
    }
    assert!(!viewer.cleanup_verified());
}

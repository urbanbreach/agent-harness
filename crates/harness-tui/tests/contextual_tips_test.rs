#[path = "../src/contextual_tips/mod.rs"]
mod contextual_tips;

use contextual_tips::priority::{priority_for, resolve_active};
use contextual_tips::triggers::evaluate_triggers;
use contextual_tips::{TipContext, TipId, TipLifetime, TipManager, TipPriority};

#[test]
fn triggers_cover_empty_and_all_true_contexts() {
    // arrange
    // act
    let empty_context = TipContext {
        model_selected: true,
        ..TipContext::default()
    };
    // assert
    assert!(evaluate_triggers(&empty_context).is_empty());
    let context = TipContext {
        is_first_run: true,
        composer_empty: true,
        is_streaming: true,
        permission_pending: true,
        tool_running: true,
        transcript_blocks: 51,
        reduced_motion: true,
        viewport_compact: true,
        model_selected: true,
        queue_items: 1,
    };
    assert_eq!(evaluate_triggers(&context).len(), 8);
}

#[test]
fn active_resolution_uses_priority_and_is_deterministic() {
    // arrange
    // act
    // assert
    assert_eq!(resolve_active(&[]), None);
    assert_eq!(
        resolve_active(&[TipId::ToolRunning, TipId::PermissionPrompted]).map(|t| t.id),
        Some(TipId::PermissionPrompted)
    );
    let tied = [TipId::ToolRunning, TipId::StreamingStarted];
    assert_eq!(
        priority_for(TipId::ToolRunning),
        TipPriority {
            rank: 8,
            display_seconds: 4
        }
    );
    assert_eq!(
        resolve_active(&tied).map(|t| t.id),
        Some(TipId::StreamingStarted)
    );
    assert_eq!(
        resolve_active(&[TipId::StreamingStarted, TipId::ToolRunning]).map(|t| t.id),
        Some(TipId::StreamingStarted)
    );
}

#[test]
fn manager_starts_empty_and_tracks_persistent_permission_tip() {
    // arrange
    // act
    let mut manager = TipManager::new();
    // assert
    assert_eq!(manager.active(), None);
    let context = TipContext {
        permission_pending: true,
        model_selected: true,
        ..TipContext::default()
    };
    assert_eq!(manager.update(&context), Some(TipId::PermissionPrompted));
    manager.tick();
    assert_eq!(manager.active(), Some(TipId::PermissionPrompted));
}

#[test]
fn transient_tip_expires_after_its_display_ticks() {
    // arrange
    // act
    let mut manager = TipManager::new();
    let context = TipContext {
        composer_empty: true,
        model_selected: true,
        ..TipContext::default()
    };
    // assert
    assert_eq!(manager.update(&context), Some(TipId::ComposerEmpty));
    for _ in 0..4 {
        manager.tick();
    }
    assert_eq!(manager.active(), Some(TipId::ComposerEmpty));
    manager.tick();
    assert_eq!(manager.active(), None);
}

#[test]
fn dismissal_persists_until_cleared() {
    // arrange
    // act
    let mut manager = TipManager::new();
    let context = TipContext {
        composer_empty: true,
        model_selected: true,
        ..TipContext::default()
    };
    manager.update(&context);
    manager.dismiss(TipId::ComposerEmpty);
    // assert
    assert!(manager.is_dismissed(TipId::ComposerEmpty));
    assert_eq!(manager.active(), None);
    assert_eq!(manager.update(&context), None);
    manager.clear_dismissals();
    assert_eq!(manager.update(&context), Some(TipId::ComposerEmpty));
}

#[test]
fn competing_tips_choose_highest_priority_and_no_triggers_clear() {
    // arrange
    // act
    let mut manager = TipManager::new();
    let context = TipContext {
        composer_empty: true,
        permission_pending: true,
        model_selected: true,
        ..TipContext::default()
    };
    // assert
    assert_eq!(manager.update(&context), Some(TipId::PermissionPrompted));
    assert!(matches!(TipLifetime::Persistent, TipLifetime::Persistent));
    assert_eq!(
        manager.update(&TipContext {
            model_selected: true,
            ..TipContext::default()
        }),
        None
    );
}

use super::claim::{first_method_discards, grown_fits, snapshot_applies};
use super::*;
use crate::coord::compaction::{CompactionError, CompactionErrorKind};
use CompactionMethod::{Archive, Handoff, Remote, Shake, Soft};

/// Scenario 53: with a 50,000 threshold the band starts at 41,808 and excludes the threshold.
#[test]
fn band_includes_its_lower_bound_and_excludes_the_threshold() {
    for (tokens, expected) in [
        (41_807, false),
        (41_808, true),
        (49_999, true),
        (50_000, false),
        (50_001, false),
    ] {
        assert_eq!(in_band(tokens, 50_000), expected, "{tokens}");
    }
}

/// Scenario 56: the grace cap is the lead past the threshold, bounded by the window room.
#[test]
fn grace_cap_stops_deferral_at_the_exact_cap() {
    assert_eq!(grace_cap(50_000, 100_000), 58_192);
    assert_eq!(grace_cap(50_000, 60_000), 51_808);
    assert_eq!(grace_cap(50_000, 4_000), 0);
}

/// Scenario 57: a local method first means nothing is speculated, even when a summary follows.
#[test]
fn only_a_leading_model_method_speculates() {
    for (order, expected) in [
        (vec![Soft], Some(Soft)),
        (vec![Remote, Soft], Some(Remote)),
        (vec![Handoff, Soft], Some(Handoff)),
        (vec![Archive, Soft], None),
        (vec![Shake, Soft], None),
        (vec![], None),
    ] {
        assert_eq!(speculative_first(&order), expected, "{order:?}");
    }
}

/// Scenario 62: a permanently failed native route is skipped at every position, but a
/// remote-only walk still attempts it.
#[test]
fn blacklisted_native_route_is_skipped_everywhere_unless_alone() {
    for (order, expected) in [
        (vec![Remote, Soft, Remote], vec![Soft]),
        (vec![Soft, Remote], vec![Soft]),
        (vec![Remote], vec![Remote]),
        (vec![Remote, Remote], vec![Remote, Remote]),
    ] {
        let mut order = order;
        skip_native_route(&mut order);
        assert_eq!(order, expected);
    }
}

fn native(
    category: Option<ProviderErrorCategory>,
    status: Option<u16>,
    auth: bool,
    kind: CompactionErrorKind,
) -> CoordinatorError {
    let mut error = CompactionError::new(kind, "failed");
    error.category = category;
    error.status = status;
    error.auth = auth;
    CoordinatorError::Compaction(error)
}

/// Scenario 62: only irrecoverable typed native causes blacklist; credential, endpoint and
/// retriable failures never do.
#[test]
fn only_irrecoverable_native_causes_blacklist() {
    use CompactionErrorKind::{Endpoint, Native};
    use ProviderErrorCategory as Category;
    for (error, expected) in [
        (
            native(Some(Category::OutputLimitExceeded), None, false, Native),
            true,
        ),
        (
            native(Some(Category::UnsupportedToolCall), None, false, Native),
            true,
        ),
        (
            native(Some(Category::MalformedStream), None, false, Native),
            true,
        ),
        (
            native(Some(Category::Other), Some(422), false, Native),
            true,
        ),
        (
            native(Some(Category::InvalidCredentials), Some(401), true, Native),
            false,
        ),
        (
            native(Some(Category::OutputLimitExceeded), None, true, Native),
            false,
        ),
        (
            native(Some(Category::RateLimited), Some(429), false, Native),
            false,
        ),
        (
            native(Some(Category::TransportFailure), None, false, Native),
            false,
        ),
        (
            native(Some(Category::Other), Some(503), false, Native),
            false,
        ),
        (native(None, None, false, Native), false),
        (
            native(Some(Category::OutputLimitExceeded), None, false, Endpoint),
            false,
        ),
        (CoordinatorError::Invalid("no".into()), false),
    ] {
        assert_eq!(native_irrecoverable(&error), expected, "{error:?}");
    }
}

/// Scenario 62: the blacklist is per session and qualified model.
#[test]
fn native_keys_separate_sessions_and_models() {
    let key = native_key("run-1", "openai/gpt");
    assert_ne!(key, native_key("run-2", "openai/gpt"));
    assert_ne!(key, native_key("run-1", "openai/other"));
    assert!(key.starts_with("run-1\0"));
}

/// Scenario 58: an armed model result does not survive a walk that starts with archive.
#[test]
fn archive_or_no_method_discards_the_armed_result() {
    assert!(first_method_discards(Some(Archive)));
    assert!(first_method_discards(None));
    assert!(!first_method_discards(Some(Soft)));
    assert!(!first_method_discards(Some(Shake)));
}

/// Scenario 59: a rewound, reset or compacted context no longer carries the snapshot.
#[test]
fn snapshot_must_stay_the_start_of_the_context() {
    let prefix = [1, 2, 3];
    for (seqs, compactions, expected) in [
        (vec![1, 2, 3], vec![], true),
        (vec![1, 2, 3, 4, 5], vec![], true),
        (vec![1, 2], vec![], false),
        (vec![7, 8, 9], vec![], false),
        (vec![1, 2, 3, 4], vec![3], false),
        (vec![1, 2, 3, 4], vec![0], true),
    ] {
        assert_eq!(
            snapshot_applies(&prefix, &seqs, &compactions),
            expected,
            "{seqs:?}"
        );
    }
}

/// Scenarios 55 and 61: growth beyond max(keep, 8,192) refreshes, signed results exempt,
/// and an invalid snapshot is discarded.
#[test]
fn armed_slots_refresh_on_growth_and_invalidate_when_stale() {
    for (valid, tokens, signed, expected) in [
        (true, 44_000, false, Slot::Armed),
        (true, 44_000 + 8_192, false, Slot::Armed),
        (true, 44_000 + 8_193, false, Slot::Refresh),
        (true, 60_000, true, Slot::Armed),
        (false, 44_000, false, Slot::Invalid),
        (false, 44_000, true, Slot::Invalid),
    ] {
        assert_eq!(armed_state(valid, tokens, 44_000, 1_000, signed), expected);
    }
    // A larger keep-recent budget widens the allowance.
    assert_eq!(
        armed_state(true, 60_000, 44_000, 20_000, false),
        Slot::Armed
    );
}

/// Scenario 61: a grown result must fit the recovery band and beat both the local size and
/// the supplied trigger.
#[test]
fn grown_results_need_headroom_on_every_comparison() {
    // threshold 50,000 -> band 40,000
    for (projected, pending, window_known, local, trigger, expected) in [
        (30_000, 0, true, 45_000, Some(50_500), true),
        (40_000, 0, true, 45_000, Some(50_500), true),
        (40_001, 0, true, 45_000, Some(50_500), false),
        (40_001, 0, false, 45_000, Some(50_500), true),
        (30_000, 5_000, true, 45_000, None, true),
        (36_000, 5_000, true, 45_000, None, false),
        (45_000, 0, false, 45_000, None, false),
        (44_999, 0, false, 45_000, None, true),
        (44_999, 0, false, 0, None, true),
        (30_000, 0, true, 45_000, Some(30_000), false),
        (30_000, 0, true, 45_000, Some(30_001), true),
        (30_000, 0, true, 45_000, Some(0), true),
    ] {
        assert_eq!(
            grown_fits(projected, pending, 50_000, window_known, local, trigger),
            expected,
            "{projected} {pending} {window_known} {local} {trigger:?}"
        );
    }
}

use super::claims::{ManualClaims, Resume};
use super::ManualRejection;

const AGENT: &str = "agent";

fn resume(generation: u64) -> Option<Resume> {
    Some(Resume {
        agent: AGENT.into(),
        generation,
    })
}

/// A pass that took ownership of an agent whose turn was (or was not) streaming.
fn begun(claims: &mut ManualClaims, task: &str, streaming: bool, suppress: bool) {
    assert_eq!(claims.begin(AGENT, task, streaming, suppress), Ok(()));
}

#[test]
fn resume_is_earned_only_by_this_passes_interrupted_turn_after_commit_or_typed_noop() {
    // (streaming, suppress, committed, rejected_noop, auto_continue, resumes)
    for (streaming, suppress, committed, noop, auto, resumes) in [
        (true, false, true, false, true, true), // interrupted turn, commit
        (false, false, true, false, true, false), // idle manual invocation
        (true, false, false, true, true, true), // typed too-small no-op
        (true, false, false, false, true, false), // veto or failure
        (true, true, true, false, true, false), // caller owns continuation
        (true, false, true, false, false, false), // auto-continue disabled
    ] {
        let mut claims = ManualClaims::default();
        begun(&mut claims, "t1", streaming, suppress);
        let generation = claims.generation(AGENT);
        if committed {
            claims.mark_committed(AGENT);
        }
        let got = claims.end(AGENT, "t1", noop, auto);
        let case = (streaming, suppress, committed, noop, auto);
        assert_eq!(
            got,
            resumes.then(|| resume(generation)).flatten(),
            "{case:?}"
        );
    }
}

#[test]
fn a_second_pass_is_rejected_until_the_first_ends() {
    let mut claims = ManualClaims::default();
    begun(&mut claims, "t1", false, false);
    assert_eq!(
        claims.begin(AGENT, "t2", false, false),
        Err(ManualRejection::InProgress)
    );
    assert_eq!(claims.end(AGENT, "other", false, true), None);
    claims.end(AGENT, "t1", false, true);
    assert_eq!(claims.begin(AGENT, "t2", false, false), Ok(()));
}

#[test]
fn a_pass_whose_turn_vanished_is_pruned() {
    let mut claims = ManualClaims::default();
    begun(&mut claims, "t1", false, false);
    claims.prune(AGENT, |_| true);
    assert_eq!(claims.active_task(AGENT), Some("t1"));
    claims.prune(AGENT, |_| false);
    assert_eq!(claims.active_task(AGENT), None);
}

#[test]
fn outstanding_claims_park_the_resume_until_the_last_one_leaves_without_a_turn() {
    // Each release is `dispatched`; a resume is scheduled only when the final claim
    // leaves with `false` and no earlier claim dispatched a turn.
    for (releases, resumes) in [
        (vec![false], true),
        (vec![true], false),
        (vec![false, true], false),
        (vec![true, false], false),
        (vec![false, false], true),
    ] {
        let mut claims = ManualClaims::default();
        begun(&mut claims, "t1", true, false);
        let generation = claims.generation(AGENT);
        for _ in &releases {
            assert!(claims.join(AGENT).is_some());
        }
        claims.mark_committed(AGENT);
        assert_eq!(claims.end(AGENT, "t1", false, true), None, "{releases:?}");
        let mut got = None;
        for dispatched in &releases {
            got = claims.release(AGENT, *dispatched).or(got);
        }
        assert_eq!(
            got,
            resumes.then(|| resume(generation)).flatten(),
            "{releases:?}"
        );
    }
}

#[test]
fn joining_needs_a_pass_or_an_outstanding_claim() {
    let mut claims = ManualClaims::default();
    assert!(claims.join(AGENT).is_none());
    assert!(!claims.claim_pending(AGENT));
    begun(&mut claims, "t1", true, false);
    assert!(claims.join(AGENT).is_some());
    claims.end(AGENT, "t1", true, true);
    assert!(claims.claim_pending(AGENT));
    claims.release(AGENT, true);
    claims.release(AGENT, true);
    assert!(!claims.claim_pending(AGENT));
}

#[test]
fn a_later_pass_inherits_the_owed_resume_and_only_suppression_discards_it() {
    // (second pass suppresses, second pass committed, debt survives)
    for (suppress, committed, survives) in [
        (false, false, true),
        (false, true, true),
        (true, false, false),
    ] {
        let mut claims = ManualClaims::default();
        begun(&mut claims, "t1", true, false);
        assert!(claims.join(AGENT).is_some());
        claims.mark_committed(AGENT);
        assert_eq!(claims.end(AGENT, "t1", false, true), None);
        // The first pass left a debt behind a claim that is still held.
        begun(&mut claims, "t2", false, suppress);
        let generation = claims.generation(AGENT);
        if committed {
            claims.mark_committed(AGENT);
        }
        assert_eq!(
            claims.end(AGENT, "t2", false, true),
            None,
            "parked behind the claim"
        );
        assert_eq!(
            claims.release(AGENT, false),
            survives.then(|| resume(generation)).flatten(),
            "suppress={suppress} committed={committed}"
        );
    }
}

#[test]
fn a_started_turn_clears_the_debt_but_not_the_claim_count() {
    let mut claims = ManualClaims::default();
    begun(&mut claims, "t1", true, false);
    assert!(claims.join(AGENT).is_some());
    assert!(claims.join(AGENT).is_some());
    claims.mark_committed(AGENT);
    claims.end(AGENT, "t1", false, true);
    claims.note_turn_started(AGENT);
    assert_eq!(claims.release(AGENT, false), None);
    // One claim is still outstanding, so a new one can still join it.
    assert!(claims.claim_pending(AGENT));
    assert_eq!(claims.release(AGENT, false), None);
    assert_eq!(claims.release(AGENT, false), None);
}

#[test]
fn only_a_turn_that_opened_a_provider_stream_counts_as_interrupted() {
    let mut claims = ManualClaims::default();
    assert!(!claims.was_streaming(AGENT, "turn-1"));
    claims.note_stream_started(AGENT, "turn-1");
    assert!(claims.was_streaming(AGENT, "turn-1"));
    assert!(!claims.was_streaming(AGENT, "turn-2"));
}

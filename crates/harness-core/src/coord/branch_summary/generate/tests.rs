use super::*;
use crate::config::{ModelLimitProvenance, ResolvedModelLimits};

#[test]
fn a_truthy_custom_instruction_replaces_the_branch_template() {
    let template = prompts::branch_summary_prompt();
    for (custom, expected_tail) in [
        (None, template),
        (Some(""), template),
        (Some("Only list the open bugs."), "Only list the open bugs."),
    ] {
        let request = branch_request_text("T", custom);
        assert_eq!(
            request,
            format!("<conversation>\nT\n</conversation>\n\n{expected_tail}")
        );
    }
}

#[test]
fn a_missing_or_zero_window_falls_back_to_the_branch_default() {
    let limits = |window| {
        ResolvedModelLimits::from_values(window, None, None, ModelLimitProvenance::default())
    };
    for (window, expected) in [(None, 128_000), (Some(0), 128_000), (Some(32_000), 32_000)] {
        assert_eq!(super::window(&limits(window)), expected);
    }
}

#[test]
fn outcomes_commit_a_summary_and_stay_silent_otherwise() {
    let event = |outcome: BranchSummaryOutcome| outcome.into_event("agent".into(), 7, "task");
    let generated = BranchSummaryOutcome {
        summary: Some("generated".into()),
        read_files: vec!["a".into()],
        modified_files: vec!["b".into()],
        ..BranchSummaryOutcome::default()
    };
    let committed = event(generated).ok();
    assert!(committed.is_some_and(|e| {
        e.summary == "generated"
            && e.from_event_seq == 7
            && (e.read_files.as_slice(), e.modified_files.as_slice())
                == (["a".to_owned()].as_slice(), ["b".to_owned()].as_slice())
    }));
    // An abort (or backoff cancel), an empty selection and an error commit no summary; only
    // the error is a failure the caller reports.
    for (outcome, failure) in [
        (BranchSummaryOutcome::aborted(), false),
        (BranchSummaryOutcome::summary_only(NO_CONTENT), false),
        (
            BranchSummaryOutcome::error(CompactionError::new(
                CompactionErrorKind::Branch,
                "provider said no",
            )),
            true,
        ),
    ] {
        let error = event(outcome).err();
        assert_eq!(
            error.is_some_and(|e| !matches!(e, CoordinatorError::Cancelled(_))),
            failure
        );
    }
}

use harness_core::cron_execute::{CronCivilTime, CronExecutor};
use harness_core::cron_schedule::{CronSchedule, CronScheduleRegistry, ScheduleId};
use harness_core::team_mailbox_journal::DurableTeamRegistry;
use harness_core::team_registry::TeamRegistryError;

#[test]
fn recurring_schedule_deduplicates_after_restart() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let mut schedules = CronScheduleRegistry::new();
    schedules
        .register(CronSchedule {
            id: ScheduleId::parse("hourly").unwrap_or_abort(),
            expression: "0 * * * *".to_string(),
            label: Some("hourly child".to_string()),
            payload_hint: "run child".to_string(),
        })
        .unwrap_or_abort();
    let now = CronCivilTime::new(0, 9, 15, 7, 3).unwrap_or_abort();
    // act
    let mut executor = CronExecutor::with_journal_dir(temp.path());
    // assert
    assert_eq!(
        executor
            .fire_due(&schedules, now)
            .unwrap_or_abort()
            .fired
            .len(),
        1
    );
    assert_eq!(
        executor.fire_due(&schedules, now).unwrap_or_abort().skipped,
        1
    );
    let mut restarted = CronExecutor::with_journal_dir(temp.path());
    assert_eq!(restarted.restart_from_journal().unwrap_or_abort(), 1);
    assert!(restarted
        .fire_due(&schedules, now)
        .unwrap_or_abort()
        .fired
        .is_empty());
}

#[test]
fn durable_team_child_progress_completion_and_mailbox_survive_restart() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let mut teams = DurableTeamRegistry::open(temp.path()).unwrap_or_abort();
    let team = teams.create_team("delivery").unwrap_or_abort();
    teams
        .add_member(&team.team_id, "lead", "lead")
        .unwrap_or_abort();
    teams
        .add_member(&team.team_id, "child", "worker")
        .unwrap_or_abort();
    teams
        .send_message(
            &team.team_id,
            "child",
            Some("lead".to_string()),
            "progress: complete",
        )
        .unwrap_or_abort();
    let mut restarted = DurableTeamRegistry::open(temp.path()).unwrap_or_abort();
    // act
    let receipts = restarted
        .deliver_messages(&team.team_id, "lead")
        .unwrap_or_abort();
    // assert
    assert_eq!(receipts[0].body, "progress: complete");
    restarted.cancel_team(&team.team_id).unwrap_or_abort();
    assert!(matches!(
        restarted.send_message(&team.team_id, "child", None, "late"),
        Err(
            harness_core::team_mailbox_journal::TeamMailboxJournalError::Registry(
                TeamRegistryError::Cancelled { .. }
            )
        )
    ));
}

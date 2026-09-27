use harness_core::{cron_schedule::*, team_registry::*};

#[test]
fn team_delivery_is_member_scoped_consumable_and_cancelled_as_a_unit(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut registry = TeamRegistry::new();
    let team = registry.create_team("review")?.team_id;
    registry.add_member(&team, "author", "lead")?;
    registry.add_member(&team, "reviewer", "review")?;
    assert!(registry.add_member(&team, "author", "other").is_err());
    for (sender, recipient) in [("outsider", None), ("author", Some("outsider"))] {
        assert!(registry
            .send_message(&team, sender, recipient.map(String::from), "hello")
            .is_err());
    }
    assert_eq!(registry.mailbox_len(&team)?, 0);
    registry.send_message(&team, "author", None, "ready")?;
    registry.send_message(
        &team,
        "author",
        Some("reviewer".into()),
        "api_key=hidden-key",
    )?;
    assert_eq!(registry.receive_messages(&team, "author")?.len(), 1);
    let mut restored = TeamRegistry::from_parts(registry.to_parts());
    let messages = restored.receive_messages(&team, "reviewer")?;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].body, "ready");
    assert!(!messages[1].body.contains("hidden-key"));
    assert!(restored.receive_messages(&team, "reviewer")?.is_empty());
    restored.send_message(&team, "author", Some("reviewer".into()), "pending")?;
    restored.remove_member(&team, "reviewer")?;
    restored.add_member(&team, "reviewer", "review")?;
    assert!(restored.peek_inbox(&team, "reviewer")?.is_empty());
    restored.cancel_team(&team)?;
    assert!(restored
        .send_message(&team, "author", None, "late")
        .is_err());
    assert!(restored.receive_messages(&team, "author").is_err());
    assert_eq!(restored.summary().cancelled, 1);
    assert_ne!(restored.create_team("next")?.team_id, team);
    Ok(())
}

#[test]
fn durable_teams_merge_writers_and_do_not_consume_messages_on_storage_failure(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::team_mailbox_journal::DurableTeamRegistry;
    let temp = tempfile::tempdir()?;
    let mut first = DurableTeamRegistry::open(temp.path())?;
    let mut second = DurableTeamRegistry::open(temp.path())?;
    assert_eq!(std::fs::read_dir(temp.path())?.count(), 0);
    let team = first.create_team("first")?.team_id;
    second.create_team("second")?;
    first.add_member(&team, "a", "lead")?;
    second.add_member(&team, "b", "worker")?;
    first.send_message(&team, "a", Some("b".into()), "password=hidden-value")?;
    assert_eq!(second.peek_inbox(&team, "b")?.len(), 1);
    let path = first.journal_path().to_owned();
    let bytes = std::fs::read(&path)?;
    assert!(!String::from_utf8_lossy(&bytes).contains("hidden-value"));
    let mut reopened = DurableTeamRegistry::open(temp.path())?;
    assert_eq!(reopened.summary().teams, 2);
    std::fs::write(&path, "{broken")?;
    assert!(reopened.deliver_messages(&team, "b").is_err());
    assert_eq!(reopened.registry().peek_inbox(&team, "b")?.len(), 1);
    assert_eq!(std::fs::read_to_string(&path)?, "{broken");
    std::fs::write(&path, bytes)?;
    assert_eq!(reopened.deliver_messages(&team, "b")?.len(), 1);
    assert!(DurableTeamRegistry::open(temp.path())?
        .peek_inbox(&team, "b")?
        .is_empty());
    Ok(())
}

#[test]
fn schedules_validate_field_ranges_and_registration_never_claims_a_running_timer(
) -> Result<(), Box<dyn std::error::Error>> {
    assert!(ScheduleId::parse("api_key=private-schedule-token").is_err());
    for expression in ["0 9 * * 1-5", "*/15 0,12 1-31 1-12 0-7", "1-20/3 * * * *"] {
        validate_cron_expression(expression)?;
    }
    for expression in [
        "",
        "* * * *",
        "60 * * * *",
        "* 24 * * *",
        "* * 0 * *",
        "* * * 13 *",
        "* * * * 8",
        "*/0 * * * *",
        "5-1 * * * *",
        ",1 * * * *",
        "1,,2 * * * *",
        "-1 * * * *",
        "* * * * *; touch x",
    ] {
        assert!(
            validate_cron_expression(expression).is_err(),
            "{expression}"
        );
    }
    let mut registry = CronScheduleRegistry::new();
    let schedule = CronSchedule {
        id: ScheduleId::parse("nightly")?,
        expression: " 0  2 * * * ".into(),
        label: Some("nightly".into()),
        payload_hint: "compact".into(),
    };
    registry.register(schedule.clone())?;
    assert!(registry.register(schedule.clone()).is_err());
    assert_eq!(
        registry
            .get(&schedule.id)
            .ok_or("missing schedule")?
            .expression,
        "0 2 * * *"
    );
    assert!(!registry.executor_available());
    assert!(register_cron_schedule(&mut registry, schedule.clone())
        .one_line()
        .contains("failed"));
    registry.remove(&schedule.id)?;
    assert!(registry.is_empty());
    Ok(())
}

#[test]
fn cron_due_receipts_survive_restart_without_duplicate_fires_or_partial_commits(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::cron_execute::*;
    let temp = tempfile::tempdir()?;
    let mut registry = CronScheduleRegistry::new();
    let id = ScheduleId::parse("due")?;
    let mut schedule = CronSchedule {
        id: id.clone(),
        expression: "*/15 9 13 * 1".into(),
        label: None,
        payload_hint: "compact".into(),
    };
    for (day, weekday, expected) in [(12, 1, true), (13, 3, true), (12, 3, false)] {
        assert_eq!(
            schedule_is_due(&schedule, CronCivilTime::new(30, 9, day, 5, weekday)?)?,
            expected
        );
    }
    schedule.expression = "30 9 */2 */2 *".into();
    assert!(schedule_is_due(
        &schedule,
        CronCivilTime::new(30, 9, 13, 5, 1)?
    )?);
    assert!(!schedule_is_due(
        &schedule,
        CronCivilTime::new(30, 9, 12, 5, 1)?
    )?);
    schedule.expression = "30 9 * * 7".into();
    let now = CronCivilTime::new(30, 9, 12, 5, 0)?;
    assert!(schedule_is_due(&schedule, now)?);
    registry.register(schedule)?;
    let mut first = CronExecutor::with_journal_dir(temp.path());
    assert_eq!(first.restart_from_journal()?, 0);
    assert_eq!(std::fs::read_dir(temp.path())?.count(), 0);
    assert_eq!(first.fire_due(&registry, now)?.fired.len(), 1);
    let path = temp.path().join("cron-fires.jsonl");
    let before = std::fs::read(&path)?;
    let mut second = CronExecutor::with_journal_dir(temp.path());
    assert_eq!(second.fire_due(&registry, now)?.skipped, 1);
    assert_eq!(std::fs::read(&path)?, before);
    assert!(second.fire_one_if_due(&registry, &id, now).is_err());
    std::fs::write(&path, "{corrupt\n")?;
    assert!(first
        .fire_due(&registry, CronCivilTime::new(30, 9, 19, 5, 0)?)
        .is_err());
    assert_eq!(first.fire_count(), 1);
    assert_eq!(std::fs::read_to_string(&path)?, "{corrupt\n");
    Ok(())
}

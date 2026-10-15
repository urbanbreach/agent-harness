use super::p0_04_composer_tests::{
    active_turn, capturing_live_app, last_queued_submission, maintenance,
};
use super::*;
use crate::app::prompt_queue::PendingQueueDisplay;
use crate::attachment_lifecycle::{AttachmentIngestor, AttachmentPolicy, CancellationToken};
use crate::composer_atoms::AttachmentId;
use crate::UnwrapOrAbort;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Images<'a> = Vec<(u64, &'a str)>;

fn attach(app: &mut AppState, id: u64, bytes: &str) -> TestResult {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("pasted.txt");
    std::fs::write(&path, bytes)?;
    let ingestor = AttachmentIngestor::new(AttachmentPolicy::new(temp.path())?);
    app.composer_attach(
        AttachmentId::new(id),
        ingestor.ingest_file(&path, &CancellationToken::new())?,
    )?;
    Ok(())
}

/// Makes `text` the draft with `images` attached, whatever markers `text` carries itself.
fn set_draft(app: &mut AppState, text: &str, images: &Images) -> TestResult {
    for (id, bytes) in images {
        attach(app, *id, bytes)?;
    }
    app.replace_prompt_input(text.to_owned());
    Ok(())
}

fn queue_steer(app: &mut AppState, text: &str, images: &Images) -> TestResult {
    set_draft(app, text, images)?;
    app.submit_prompt();
    Ok(())
}

fn queue_follow_up(app: &mut AppState, text: &str, images: &Images) -> TestResult {
    set_draft(app, text, images)?;
    app.submit_follow_up_prompt();
    app.clear_prompt_input();
    Ok(())
}

fn images_of(app: &AppState) -> Vec<(u64, Vec<u8>)> {
    app.composer
        .slice
        .attachments()
        .iter()
        .map(|entry| (entry.id.get(), entry.attachment.bytes().to_vec()))
        .collect()
}

fn sent(intents: &Arc<Mutex<Vec<UiIntent>>>) -> Vec<UiIntent> {
    intents.lock().unwrap_or_abort().clone()
}

#[test]
fn queueing_moves_images_to_the_queue_and_follow_up_keeps_the_draft() -> TestResult {
    // Scenarios 17, 18: images leave the draft, absent when there are none.
    for (follow_up, images) in [
        (false, vec![(1, "png")]),
        (true, vec![(1, "png")]),
        (false, vec![]),
    ] {
        let (mut app, _) = capturing_live_app();
        maintenance(&mut app, "manual", true);
        set_draft(&mut app, "keep [Image #1]", &images)?;
        if follow_up {
            app.submit_follow_up_prompt();
        } else {
            app.submit_prompt();
        }
        let item = app
            .composer
            .maintenance_queue
            .items
            .back()
            .ok_or("missing queued item")?;
        assert_eq!(
            item.images.as_ref().map(Vec::len),
            (!images.is_empty()).then_some(1)
        );
        assert!(app.composer.slice.attachments().is_empty());
        assert_eq!(
            app.composer.prompt_history.last().map(String::as_str),
            Some("keep [Image #1]")
        );
        if follow_up {
            // The draft stays, without markers of images that moved to the queue.
            assert!(app.composer.prompt_buffer.starts_with("keep"));
            assert_eq!(
                app.composer.prompt_buffer.contains("[Image #1]"),
                images.is_empty()
            );
        } else {
            assert!(app.composer.prompt_buffer.is_empty());
        }
    }
    Ok(())
}

#[test]
fn flush_delivers_each_item_into_its_lane_with_images() -> TestResult {
    // Scenarios 19, 20: a plain flush starts the first prompt, a retry uses the lanes.
    for (retry, follow_up) in [(false, false), (true, true), (true, false)] {
        let (mut app, intents) = capturing_live_app();
        maintenance(&mut app, "manual", true);
        let images: Images = vec![(1, "pixels")];
        if follow_up {
            queue_follow_up(&mut app, "and this one", &images)?;
        } else {
            queue_steer(&mut app, "describe this", &images)?;
        }
        maintenance(&mut app, if retry { "overflow" } else { "manual" }, false);
        let (_, delivered) = last_queued_submission(&intents);
        let (text, attachments) = match (retry, follow_up, delivered) {
            (
                false,
                false,
                UiIntent::SubmitPrompt {
                    text, attachments, ..
                },
            )
            | (
                true,
                false,
                UiIntent::SteerPrompt {
                    text, attachments, ..
                },
            )
            | (
                true,
                true,
                UiIntent::FollowUpPrompt {
                    text, attachments, ..
                },
            ) => (text, attachments),
            (_, _, other) => return Err(format!("wrong lane: {other:?}").into()),
        };
        assert!(text == "and this one" || text == "describe this");
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].bytes, b"pixels");
    }
    Ok(())
}

#[test]
fn dequeue_drains_both_queues_in_band_order() -> TestResult {
    // Scenario 23 and the pending bands: the agent's queue leads each band.
    let (mut app, _) = capturing_live_app();
    active_turn(&mut app);
    app.handle_paste("session steer");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_paste("session followup");
    app.submit_follow_up_prompt();
    app.clear_prompt_input();
    maintenance(&mut app, "manual", true);
    queue_steer(&mut app, "compaction steer", &Images::new())?;
    queue_follow_up(&mut app, "compaction followup", &Images::new())?;
    assert_eq!(
        app.pending_queue_display(),
        PendingQueueDisplay {
            steering: vec!["session steer".into(), "compaction steer".into()],
            after_yield: vec!["session followup".into(), "compaction followup".into()],
        }
    );

    app.execute_action(Action::DequeuePrompt);
    app.restore_queued_input(vec!["session steer".into(), "session followup".into()]);

    assert_eq!(
        app.composer.prompt_buffer,
        "session steer\n\ncompaction steer\n\nsession followup\n\ncompaction followup"
    );
    assert_eq!(app.maintenance_queue_lanes(), (0, 0));
    assert_eq!(
        app.toast().ok_or("missing toast")?.message,
        "Restored 4 queued messages to the editor"
    );
    Ok(())
}

#[test]
fn dequeue_renumbers_image_markers_after_the_draft_images() -> TestResult {
    // Scenarios 21, 22, 24-27.
    struct Case<'a> {
        draft: Option<(&'a str, Images<'a>)>,
        queued: Vec<(&'a str, Images<'a>)>,
        text: &'a str,
        images: Vec<(u64, &'a str)>,
    }
    let cases = [
        Case {
            draft: Some(("[Image #1] draft text", vec![(1, "D")])),
            queued: vec![("[Image #1] queued text", vec![(1, "Q")])],
            text: "[Image #2] queued text\n\n[Image #1] draft text",
            images: vec![(1, "D"), (2, "Q")],
        },
        Case {
            draft: Some(("[Image #1, 100x100]", vec![(1, "D")])),
            queued: vec![("look [Image #1, 800x600] now", vec![(1, "Q")])],
            text: "look [Image #2, 800x600] now\n\n[Image #1, 100x100]",
            images: vec![(1, "D"), (2, "Q")],
        },
        Case {
            draft: Some(("[Image #1] draft", vec![(1, "D")])),
            queued: vec![
                ("a [Image #1]", vec![(1, "a")]),
                (
                    "b [Image #1] [Image #2] [Image #1]",
                    vec![(1, "b1"), (2, "b2")],
                ),
            ],
            text: "a [Image #2]\n\nb [Image #3] [Image #4] [Image #3]\n\n[Image #1] draft",
            images: vec![(1, "D"), (2, "a"), (3, "b1"), (4, "b2")],
        },
        Case {
            draft: None,
            queued: vec![("[Image #1] queued", vec![(1, "Q")])],
            text: "[Image #1] queued",
            images: vec![(1, "Q")],
        },
        Case {
            draft: None,
            queued: vec![("[attachment:1] and [attachment:1]", vec![(1, "Q")])],
            text: "[attachment:1] and [attachment:1]",
            images: vec![(1, "Q")],
        },
        Case {
            draft: Some(("draft[attachment:1]", vec![(1, "D")])),
            queued: vec![("see [attachment:1]", vec![(1, "Q")])],
            text: "see [attachment:2]\n\ndraft[attachment:1]",
            images: vec![(1, "D"), (2, "Q")],
        },
        Case {
            draft: None,
            queued: vec![("/skill:foo bar", vec![])],
            text: "/skill:foo bar",
            images: vec![],
        },
    ];
    for case in cases {
        let (mut app, _) = capturing_live_app();
        maintenance(&mut app, "manual", true);
        for (text, images) in &case.queued {
            queue_steer(&mut app, text, images)?;
        }
        if let Some((text, images)) = &case.draft {
            set_draft(&mut app, text, images)?;
        }

        app.execute_action(Action::DequeuePrompt);

        assert_eq!(app.composer.prompt_buffer, case.text);
        let expected: Vec<_> = case
            .images
            .iter()
            .map(|(id, bytes)| (*id, bytes.as_bytes().to_vec()))
            .collect();
        assert_eq!(images_of(&app), expected);
        assert_eq!(app.maintenance_queue_lanes(), (0, 0));
    }
    Ok(())
}

#[test]
fn queued_new_session_launches_the_next_text_in_a_fresh_session() -> TestResult {
    // A queued /new then text goes through NewSession, never to the old live target.
    let (mut app, intents) = capturing_live_app();
    maintenance(&mut app, "manual", true);
    queue_steer(&mut app, "/new", &Images::new())?;
    queue_steer(&mut app, "fresh start", &Images::new())?;
    maintenance(&mut app, "manual", false);

    assert_eq!(sent(&intents), vec![UiIntent::NewSession]);
    let launched = crate::app::pending_live::take_pending_live_prompt()
        .map(|prompt| (prompt.text, prompt.auto_submit));
    assert_eq!(launched, Some(("fresh start".to_owned(), true)));
    assert!(app.should_quit);

    // Only commands queued: the session resets and no model prompt is sent.
    let (mut app, intents) = capturing_live_app();
    maintenance(&mut app, "manual", true);
    queue_steer(&mut app, "/new", &Images::new())?;
    maintenance(&mut app, "manual", false);
    assert!(sent(&intents).is_empty());
    assert!(app.startup_mode);
    assert!(crate::app::pending_live::take_pending_live_prompt().is_none());
    Ok(())
}

#[test]
fn failed_flush_restores_the_whole_snapshot_and_withdraws_accepted_input() -> TestResult {
    for (accepted_before_failure, queued) in [(0, 1), (2, 3)] {
        let (mut app, intents) = capturing_live_app();
        maintenance(&mut app, "manual", true);
        queue_steer(&mut app, "one", &Images::new())?;
        if queued > 1 {
            queue_steer(&mut app, "two", &Images::new())?;
            queue_follow_up(&mut app, "three", &Images::new())?;
        }
        maintenance(&mut app, "manual", false);
        for _ in 0..accepted_before_failure {
            let (id, _) = last_queued_submission(&intents);
            app.maintenance_prompt_receipt(id, Ok(()));
        }
        let (id, _) = last_queued_submission(&intents);
        app.maintenance_prompt_receipt(id, Err("boom".into()));

        let (expected, plural) = if queued > 1 {
            ("one\n\ntwo\n\nthree", "s")
        } else {
            ("one", "")
        };
        assert_eq!(app.composer.prompt_buffer, expected);
        assert_eq!(
            app.toast().ok_or("missing toast")?.message,
            format!("Failed to send queued message{plural}: boom")
        );
        let withdrawals = sent(&intents)
            .iter()
            .filter(|intent| matches!(intent, UiIntent::WithdrawAcceptedInput))
            .count();
        assert_eq!(withdrawals, usize::from(accepted_before_failure > 0));
        // The withdrawal returns what the agent had accepted; it is already restored.
        if accepted_before_failure > 0 {
            app.restore_queued_input(vec!["one".into(), "two".into()]);
            assert_eq!(app.composer.prompt_buffer, expected);
        }
    }
    Ok(())
}

#[test]
fn the_pending_band_lists_steering_then_after_yield_and_hides_for_a_child_or_replay() -> TestResult
{
    let (mut app, _) = capturing_live_app();
    maintenance(&mut app, "manual", true);
    queue_follow_up(&mut app, "later work", &vec![])?;
    queue_steer(&mut app, "change course", &vec![])?;
    let screen = render_text(&app, 80, 24);
    let rows: Vec<&str> = screen.lines().collect();
    let at = |needle: &str| rows.iter().position(|row| row.contains(needle));
    let (steering, steer_item, after, after_item) = (
        at("Steering"),
        at("change course"),
        at("After yield"),
        at("later work"),
    );
    assert!(
        steering < steer_item && steer_item < after && after < after_item,
        "{screen}"
    );
    assert!(!screen.contains("QUEUED") && !screen.contains("Steering 1"));
    app.replay_mode = true;
    assert_eq!(app.pending_queue_display(), PendingQueueDisplay::default());
    Ok(())
}

#[test]
fn failed_flush_restores_accepted_image_prompts_once() -> TestResult {
    // The agent accepted the image-bearing prompt before the next delivery failed: the
    // rollback withdraws everything accepted, and the restored draft carries it once.
    let (mut app, intents) = capturing_live_app();
    maintenance(&mut app, "manual", true);
    queue_steer(&mut app, "look at this", &vec![(1, "png")])?;
    queue_steer(&mut app, "then this", &Images::new())?;
    maintenance(&mut app, "manual", false);
    let (id, accepted) = last_queued_submission(&intents);
    let UiIntent::SubmitPrompt {
        text, attachments, ..
    } = &accepted
    else {
        return Err(format!("unexpected first delivery: {accepted:?}").into());
    };
    assert_eq!(attachments.len(), 1);
    let accepted_text = text.clone();
    app.maintenance_prompt_receipt(id, Ok(()));
    let (id, _) = last_queued_submission(&intents);
    app.maintenance_prompt_receipt(id, Err("boom".into()));
    assert_eq!(
        sent(&intents)
            .iter()
            .filter(|intent| matches!(intent, UiIntent::WithdrawAcceptedInput))
            .count(),
        1
    );
    let restored = app.composer.prompt_buffer.clone();
    assert!(restored.contains("then this") && restored.contains("look at this"));
    assert_eq!(images_of(&app).len(), 1);
    // What the coordinator hands back is what is already in the editor.
    app.restore_queued_input(vec![accepted_text]);
    assert_eq!(app.composer.prompt_buffer, restored);
    assert_eq!(images_of(&app).len(), 1);
    Ok(())
}

#[test]
fn queued_new_session_carries_the_images_of_the_launch_prompt() -> TestResult {
    let (mut app, intents) = capturing_live_app();
    maintenance(&mut app, "manual", true);
    queue_steer(&mut app, "/new", &Images::new())?;
    queue_steer(&mut app, "look at this", &vec![(1, "png")])?;
    queue_steer(&mut app, "and this", &vec![(2, "gif")])?;
    maintenance(&mut app, "manual", false);
    // The flush neither fails nor restores anything: the session launches with both images.
    assert_eq!(sent(&intents), vec![UiIntent::NewSession]);
    assert!(app
        .toast()
        .is_none_or(|toast| !toast.message.starts_with("Failed")));
    let launched = crate::app::pending_live::take_pending_live_prompt().ok_or("no launch")?;
    assert!(launched.auto_submit);
    assert_eq!(launched.text.matches("\n\n").count(), 1);
    let mut ids: Vec<u64> = launched.images.iter().map(|entry| entry.id.get()).collect();
    ids.sort_unstable();
    assert_eq!(ids, [1, 2]);
    Ok(())
}

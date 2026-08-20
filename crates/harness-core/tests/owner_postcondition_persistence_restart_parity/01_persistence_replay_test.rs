#[test]
fn p2_session_persistence_survives_writer_drop_and_reopen() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-persist-restart";

    // act
    let first_seq = {
        let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
        store
            .append(make_envelope_without_seq(
                run_id,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "first-life".into(),
                    workspace_root: "/ws".to_string(),
                }),
            ))
            .unwrap_or_abort()
            .seq
    };
    // assert
    assert_eq!(first_seq, 1, "first append must yield seq=1");

    // Restart: open a fresh store on the same run dir; existing events must
    // be visible and the next seq must continue monotonically.
    let reopened = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
    assert_eq!(
        reopened.next_seq().unwrap_or_abort(),
        2,
        "reopened store must resume seq counter"
    );

    // Mutation: drop this check and the test no longer proves persistence
    // across restart — a regression that silently wipes events.jsonl on
    // reopen would slip through.
    let events_on_disk = read_events_from_jsonl(&root.path().join(run_id).join("events.jsonl"));
    assert_eq!(
        events_on_disk.len(),
        1,
        "exactly one event must persist after writer drop"
    );
    assert_eq!(events_on_disk[0].seq, 1);
}

// ===========================================================================
// P7 REPLAY PURITY: three passes produce identical projections; no fs delta.
// ===========================================================================

#[tokio::test]
async fn p7_replay_three_passes_produce_identical_projections_with_no_fs_delta() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-replay-purity";
    let events = full_run_events(run_id);

    {
        let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
        for event in &events {
            store
                .append(EventEnvelopeWithoutSeqV1::from(event.clone()))
                .unwrap_or_abort();
        }
    }

    let events_path = root.path().join(run_id).join("events.jsonl");
    let digest_seed = event_log_digest(&events_path).unwrap_or_abort();

    let mut projections = Vec::new();
    for pass in 0..3 {
        let store = JsonlFileEventStore::open_existing(root.path(), run_id, true).unwrap_or_abort();
        let replayed: Vec<EventEnvelopeV1> = {
            use tokio_stream::StreamExt;
            let mut stream = store.replay(1).unwrap_or_abort();
            let mut out = Vec::new();
            while let Some(result) = stream.next().await {
                out.push(result.unwrap_or_abort());
            }
            out
        };
        drop(store);

        let conv = project_conversation(&replayed, &[]).unwrap_or_abort();
        projections.push((replayed, conv));

        // act
        // Every pass must leave events.jsonl byte-identical.
        // Mutation: revert the append-only guarantee and this digest drifts.
        let digest_after = event_log_digest(&events_path).unwrap_or_abort();
        // assert
        assert_eq!(
            digest_seed, digest_after,
            "pass {pass} mutated events.jsonl — replay must be read-only"
        );
    }

    // All three projections must be equal — proves determinism.
    assert_eq!(
        projections[0], projections[1],
        "replay passes 0 and 1 diverged"
    );
    assert_eq!(
        projections[1], projections[2],
        "replay passes 1 and 2 diverged"
    );
}

// ===========================================================================
// P7 REPLAY PURITY: transcript projection is also deterministic and pure.
// ===========================================================================

#[tokio::test]
async fn p7_transcript_projection_is_deterministic_across_replays() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-transcript-purity";
    let events = full_run_events(run_id);

    {
        let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
        for event in &events {
            store
                .append(EventEnvelopeWithoutSeqV1::from(event.clone()))
                .unwrap_or_abort();
        }
    }

    let proj_a = project_transcript(&replay_all_async(root.path(), run_id).await).unwrap_or_abort();
    let proj_b = project_transcript(&replay_all_async(root.path(), run_id).await).unwrap_or_abort();

    // act
    // Mutation: introduce nondeterminism into the transcript projection
    // (e.g. a clock-derived sort key) and these two will diverge.
    // assert
    assert_eq!(
        format!("{proj_a:?}"),
        format!("{proj_b:?}"),
        "transcript projection must be deterministic across replays"
    );
}


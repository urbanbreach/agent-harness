use super::*;

pub(super) async fn malformed_question_answer_does_not_resolve_permission() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let config = test_config(temp_dir.path());
    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("question_validation", temp_dir.path())
        .await
        .unwrap_or_abort();

    let question_handle = handle.clone();
    let request = tokio::spawn(async move {
        question_handle
            .request_question(
                EventActor::new(ActorKind::Worker, Some("agent-worker".to_string())),
                "toolcall_question_validation",
                json!({
                    "questions": [{
                        "question": "Pick one",
                        "header": "Choice",
                        "options": [{"label": "A", "description": "Option A"}],
                    }]
                }),
            )
            .await
    });

    let before = wait_for_events(
        &handle,
        &run.events_path,
        "question permission request",
        |event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data) if data.kind == "question"
            )
        },
    )
    .await;
    let permission_id = before
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data) if data.kind == "question" => {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();

    let err = handle
        .resolve_permission(
            permission_id.clone(),
            PermissionDecision::Allow,
            Some("not-json".to_string()),
        )
        .await
        .expect_err("malformed answers must be rejected");
    assert!(err.to_string().contains("invalid question answer payload"));

    assert!(
        read_events(&run.events_path).iter().all(|event| {
            !matches!(
                &event.payload,
                EventV1::PermissionResolved(data) if data.permission_id == permission_id
            )
        }),
        "permission should remain pending when answer payload is invalid"
    );

    request.abort();
    handle.stop_run().await.unwrap_or_abort();
}

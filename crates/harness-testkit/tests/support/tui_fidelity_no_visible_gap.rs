use super::*;

#[test]
fn no_visible_send_does_not_create_input_response_tail() {
    let fixture = AggregateFixture::new_packet2();
    configure_gap(&fixture, "no_visible_change", None);

    aggregate_with_profile(&fixture.roots, AcceptanceProfile::Packet2Scheduling)
        .expect("60ms semantic gap is not relabeled as an input response tail");
}

#[test]
fn visible_send_with_same_timestamps_still_fails_fast_cadence() {
    let fixture = AggregateFixture::new_packet2();
    configure_gap(&fixture, "visible_change", Some(1));

    let error = aggregate_with_profile(&fixture.roots, AcceptanceProfile::Packet2Scheduling)
        .expect_err("visible input with 60ms response tail rejected");

    assert!(error.to_string().contains("16 ms cadence"));
}

#[test]
fn missing_native_interaction_linkage_fails_closed() {
    let fixture = AggregateFixture::new_packet2();
    configure_gap(&fixture, "no_visible_change", None);
    mutate_json(&fixture.roots[0].join("receipt.json"), |receipt| {
        receipt["runtimes"][1]["presentation"]["native"]["causes"] = serde_json::json!([]);
    });

    let error = aggregate_with_profile(&fixture.roots, AcceptanceProfile::Packet2Scheduling)
        .expect_err("missing native cause linkage rejected");

    assert!(error.to_string().contains("interaction linkage"));
}

#[test]
fn mixed_native_outcomes_remain_visible() {
    let fixture = AggregateFixture::new_packet2();
    configure_gap(&fixture, "no_visible_change", None);
    for root in &fixture.roots {
        mutate_json(&root.join("receipt.json"), |receipt| {
            receipt["runtimes"][1]["presentation"]["native"]["causes"]
                .as_array_mut()
                .expect("causes")
                .push(serde_json::json!({
                    "interaction_id":"scenario:action:0","resulting_revision":1,
                    "outcome":"visible_change"
                }));
        });
    }

    let error = aggregate_with_profile(&fixture.roots, AcceptanceProfile::Packet2Scheduling)
        .expect_err("mixed outcome send remains visible");

    assert!(error.to_string().contains("16 ms cadence"));
}

#[test]
fn visible_send_without_timestamp_fails_closed() {
    let fixture = AggregateFixture::new_packet2();
    configure_gap(&fixture, "visible_change", Some(1));
    mutate_json(&fixture.roots[0].join("receipt.json"), |receipt| {
        receipt["runtimes"][1]["presentation"]["external"]["actual_input_sends"][0]
            .as_object_mut()
            .expect("send")
            .remove("sent_at");
    });

    let error = aggregate_with_profile(&fixture.roots, AcceptanceProfile::Packet2Scheduling)
        .expect_err("visible send timestamp required");

    assert!(error.to_string().contains("timestamp missing"));
}

fn configure_gap(fixture: &AggregateFixture, first_outcome: &str, first_revision: Option<u64>) {
    for root in &fixture.roots {
        mutate_json(&root.join("receipt.json"), |receipt| {
            receipt["scenario_id"] = serde_json::json!("packet2-sustained-stream");
            for runtime in receipt["runtimes"].as_array_mut().expect("runtimes") {
                runtime["presentation_binding"]["scenario_id"] =
                    serde_json::json!("packet2-sustained-stream");
                let sends = runtime["presentation"]["external"]["actual_input_sends"]
                    .as_array_mut()
                    .expect("sends");
                sends[0]["sent_at"] = serde_json::json!(100);
                sends[1]["sent_at"] = serde_json::json!(100_000);
            }
            receipt["runtimes"][1]["presentation"]["native"]["causes"] = serde_json::json!([
                {"interaction_id":"scenario:action:0","resulting_revision":first_revision,
                 "outcome":first_outcome},
                {"interaction_id":"scenario:action:1","resulting_revision":2,
                 "outcome":"visible_change"}
            ]);
        });
        mutate_json(&root.join("comparison.json"), |comparison| {
            comparison["presentation"]["candidate"]["external_observation_timestamps_micros"] =
                serde_json::json!([100, 60_100, 100_000]);
        });
    }
}

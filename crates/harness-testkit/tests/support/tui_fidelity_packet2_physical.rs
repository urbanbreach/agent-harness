use super::*;

#[test]
fn type_text_gap_uses_linked_native_inter_byte_cadence() {
    let fixture = AggregateFixture::new_packet2();
    configure_linked_type_gap(&fixture);

    aggregate_with_profile(&fixture.roots, AcceptanceProfile::Packet2Scheduling)
        .expect("67ms linked TypeText gap is within twice the declared 45ms cadence");
}

#[test]
fn type_text_gap_without_clock_bridge_fails_closed() {
    let fixture = AggregateFixture::new_packet2();
    configure_linked_type_gap(&fixture);
    mutate_json(&fixture.roots[0].join("receipt.json"), |receipt| {
        receipt["runtimes"][1]["presentation"]["links"] = serde_json::json!([]);
    });

    let error = aggregate_with_profile(&fixture.roots, AcceptanceProfile::Packet2Scheduling)
        .expect_err("missing observation-to-frame bridge rejected");

    assert!(error.to_string().contains("33 ms cadence"));
}

#[test]
fn duplicate_digest_at_different_stream_offset_fails_closed() {
    let fixture = AggregateFixture::new_packet2();
    configure_linked_type_gap(&fixture);
    mutate_json(&fixture.roots[0].join("receipt.json"), |receipt| {
        receipt["runtimes"][1]["presentation"]["links"][1]["stream_offset"] = serde_json::json!(0);
    });

    let error = aggregate_with_profile(&fixture.roots, AcceptanceProfile::Packet2Scheduling)
        .expect_err("same digest at the wrong stream offset rejected");

    assert!(error.to_string().contains("33 ms cadence"));
}

fn configure_linked_type_gap(fixture: &AggregateFixture) {
    for root in &fixture.roots {
        mutate_json(&root.join("receipt.json"), |receipt| {
            receipt["scenario_id"] = serde_json::json!("packet2-sustained-stream");
            for runtime in receipt["runtimes"].as_array_mut().expect("runtimes") {
                runtime["presentation_binding"]["scenario_id"] =
                    serde_json::json!("packet2-sustained-stream");
                let sends = runtime["presentation"]["external"]["actual_input_sends"]
                    .as_array_mut()
                    .expect("sends");
                sends[0]["sent_at"] = serde_json::json!(0);
                sends[1]["sent_at"] = serde_json::json!(100_000);
            }
            let presentation = &mut receipt["runtimes"][1]["presentation"];
            presentation["external"]["observations"] = serde_json::json!([
                {"observed_at":1,"raw_read_ordinals":[0]},
                {"observed_at":67_029,"raw_read_ordinals":[1]},
                {"observed_at":100_000,"raw_read_ordinals":[2]}
            ]);
            presentation["external"]["raw_reads"] = serde_json::json!([
                {"byte_len":10,"sha256":"a"},{"byte_len":10,"sha256":"b"},
                {"byte_len":10,"sha256":"c"}
            ]);
            presentation["links"] = serde_json::json!([
                {"frame_sequence":1,"byte_sha256":"a","stream_offset":0},
                {"frame_sequence":3,"byte_sha256":"b","stream_offset":10},
                {"frame_sequence":2,"byte_sha256":"c","stream_offset":20}
            ]);
            let native = &mut presentation["native"];
            native["causes"][1]["received_at"] = serde_json::json!(100_000);
            native["causes"]
                .as_array_mut()
                .expect("causes")
                .push(serde_json::json!({
                    "cause_id":"cause:3","interaction_id":null,"received_at":45_000,
                    "kind":"terminal_input","resulting_revision":3,"outcome":"visible_change"
                }));
            native["frames"]
                .as_array_mut()
                .expect("frames")
                .push(serde_json::json!({
                    "sequence":3,"revision":3,"cause_ids":["cause:3"]
                }));
            native["acknowledgements"]
                .as_array_mut()
                .expect("acks")
                .push(serde_json::json!({
                    "sequence":3,"acknowledged_at":45_020,"outcome":"completed_write"
                }));
            native["acknowledgements"][1]["acknowledged_at"] = serde_json::json!(100_020);
        });
        mutate_json(&root.join("comparison.json"), |comparison| {
            comparison["presentation"]["candidate"]["external_observation_timestamps_micros"] =
                serde_json::json!([1, 67_029, 100_000]);
        });
    }
}

#[test]
fn validator_rejects_viewport_inconsistent_with_responsive_behavior_id() {
    // arrange
    let mut manifest = checked_in_manifest();
    row_mut(&mut manifest, "RESP-80x24")["viewport"]["cols"] = json!(120);

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "state-viewport-mismatch");
}

#[test]
fn validator_rejects_viewport_inconsistent_with_reference_freeze() {
    // arrange
    let mut manifest = checked_in_manifest();
    row_mut(&mut manifest, "P0-START-01")["viewport"] = json!({ "cols": 80, "rows": 24 });

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "state-viewport-mismatch");
}

#[test]
fn validator_rejects_diverged_without_declared_receipt_note() {
    // arrange — no checked-in row is diverged (Wave 4.7), so synthesize one
    let mut manifest = checked_in_manifest();
    make_p0_start_01_pass(&mut manifest);
    let diverged_row = row_mut(&mut manifest, "P0-START-01");
    diverged_row["status"] = json!("diverged");
    diverged_row["deliberate_divergence_id"] = json!("DIV-AA-SHELL-FAIL");
    manifest["identity_policy"]["approved_divergence_notes"]["DIV-AA-SHELL-FAIL"] =
        json!("User-approved pure AA residual. Receipt marker removed.");

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "missing-divergence-receipt");
}

#[test]
fn validator_rejects_diverged_evidence_backed_status() {
    // arrange
    // The "evidence-backed divergence" category is forbidden; only
    // incomplete/blocked/pass/diverged statuses are allowed.
    let mut manifest = checked_in_manifest();
    row_mut(&mut manifest, "P0-START-01")["status"] = json!("diverged_evidence_backed");

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "invalid-status");
}

#[test]
fn derive_status_demotes_claims_with_evidence_gaps() {
    // arrange
    let mut manifest = checked_in_manifest();
    make_p0_start_01_pass(&mut manifest);
    let pass_row = row_mut(&mut manifest, "P0-START-01").clone();
    // No row in the checked-in manifest is diverged anymore (DIV-AA-PALETTE
    // was rejected in Wave 4.7), so synthesize the diverged and blocked cases
    // from a promoted pass row.
    let diverged_row = {
        let mut row = pass_row.clone();
        row["status"] = json!("incomplete");
        row["deliberate_divergence_id"] = json!("DIV-AA-SHELL-FAIL");
        row
    };
    let gap_row = {
        let mut row = pass_row.clone();
        row["evidence_paths"]["L4"] = json!("");
        row
    };
    let blocked_row = {
        let mut row = pass_row.clone();
        row["status"] = json!("incomplete");
        row["deliberate_divergence_id"] = json!("DIV-NOT-APPROVED");
        row
    };
    let policy = divergence_policy(&manifest);

    // act
    let pass_derived = derive_status(&pass_row, &policy);
    let diverged_derived = derive_status(&diverged_row, &policy);
    let gap_derived = derive_status(&gap_row, &policy);
    let blocked_derived = derive_status(&blocked_row, &policy);

    // assert
    assert_eq!(pass_derived, "pass");
    assert_eq!(diverged_derived, "diverged");
    assert_eq!(
        gap_derived, "incomplete",
        "evidence gaps must derive incomplete"
    );
    assert_eq!(
        blocked_derived, "blocked",
        "unapproved divergences must derive blocked"
    );
}

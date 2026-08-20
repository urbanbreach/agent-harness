use std::collections::BTreeMap;

use crate::tui_fidelity::Scenario;

use super::super::types::AcceptanceProfile;
use super::build_receipt;

const STARTUP: &str = include_str!("../../tui_fidelity_scenarios/baseline/startup.json");

#[test]
fn comparison_receipt_enumerates_every_applied_typed_span() -> Result<(), Box<dyn std::error::Error>>
{
    // arrange
    let scenario = Scenario::from_json(STARTUP)?;

    // act
    let receipt = build_receipt(
        &scenario,
        AcceptanceProfile::FullParity,
        true,
        true,
        BTreeMap::new(),
        None,
        true,
    );

    // assert
    assert_eq!(receipt.applied_substitutions, scenario.substitutions);
    assert!(receipt.applied_substitutions.iter().all(|substitution| {
        !substitution.reference_provenance.is_empty()
            && !substitution.candidate_provenance.is_empty()
            && substitution.reference.text != substitution.candidate.text
            && substitution.canonical_placeholder == substitution.field.placeholder()
            && !substitution.kind.as_str().is_empty()
            && !substitution.field.as_str().is_empty()
    }));
    Ok(())
}

#[test]
fn comparison_receipt_does_not_claim_unverified_spans_were_applied(
) -> Result<(), Box<dyn std::error::Error>> {
    // arrange
    let scenario = Scenario::from_json(STARTUP)?;

    // act
    let receipt = build_receipt(
        &scenario,
        AcceptanceProfile::FullParity,
        true,
        false,
        BTreeMap::new(),
        None,
        false,
    );

    // assert
    assert!(receipt.applied_substitutions.is_empty());
    Ok(())
}

mod boulder;

use std::fmt;

use serde::Deserialize;
use serde_json::Value;

use crate::tui_fidelity_compare::hash_bytes;
use crate::tui_fidelity_matrix::validate_coverage_documents;

pub use boulder::{complete_boulder_atomically, validate_boulder_for_completion, ClosureError};

const CONTRACT_SCHEMA: &str = "harness.tui-fidelity.closure-contract.v1";
const RECEIPT_SCHEMA: &str = "harness.tui-fidelity.closure-receipt.v1";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClosureContract {
    pub schema_version: String,
    pub reviewed_plan_sha256: String,
    pub requirement_inventory_sha256: String,
    pub coverage_manifest_sha256: String,
    pub recovery_run_id: String,
    pub preliminary_reviews: Vec<ReviewReference>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewReference {
    pub review_id: String,
    pub receipt_path: String,
}

#[derive(Clone, Debug)]
pub struct ReviewReceiptInput {
    pub review_id: String,
    pub json: String,
}

#[derive(Clone, Debug)]
pub struct ClosureVerificationInput<'a> {
    pub contract_json: &'a str,
    pub plan_bytes: &'a [u8],
    pub inventory_json: &'a str,
    pub manifest_json: &'a str,
    pub revocation_json: &'a str,
    pub review_receipts: &'a [ReviewReceiptInput],
    pub candidate_sha256: &'a str,
    pub evidence_root: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosureReceipt {
    pub json: String,
    pub receipt_sha256: String,
}

pub fn verify_closure(input: ClosureVerificationInput<'_>) -> Result<ClosureReceipt, ClosureError> {
    let contract: ClosureContract = serde_json::from_str(input.contract_json)
        .map_err(|error| ClosureError::Json(format!("contract: {error}")))?;
    if contract.schema_version != CONTRACT_SCHEMA {
        return Err(ClosureError::Invalid(
            "unsupported closure contract schema".to_owned(),
        ));
    }
    let plan_sha256 = hash_bytes(input.plan_bytes)
        .map_err(|error| ClosureError::Invalid(format!("plan hash: {error}")))?;
    let inventory_sha256 = hash_bytes(input.inventory_json.as_bytes())
        .map_err(|error| ClosureError::Invalid(format!("inventory hash: {error}")))?;
    let manifest_sha256 = hash_bytes(input.manifest_json.as_bytes())
        .map_err(|error| ClosureError::Invalid(format!("manifest hash: {error}")))?;
    if contract.reviewed_plan_sha256 != plan_sha256 {
        return Err(ClosureError::Invalid(
            "contract plan digest differs from live plan".to_owned(),
        ));
    }
    if contract.requirement_inventory_sha256 != inventory_sha256 {
        return Err(ClosureError::Invalid(
            "contract inventory digest differs from inventory".to_owned(),
        ));
    }
    if contract.coverage_manifest_sha256 != manifest_sha256 {
        return Err(ClosureError::Invalid(
            "contract manifest digest differs from manifest".to_owned(),
        ));
    }
    validate_coverage_documents(input.inventory_json, input.manifest_json)
        .map_err(|error| ClosureError::Invalid(error.to_string()))?;
    validate_inventory_plan(input.plan_bytes, input.inventory_json)?;
    validate_revocation(input.revocation_json, &contract.reviewed_plan_sha256)?;
    validate_reviews(
        &contract,
        input.review_receipts,
        &contract.reviewed_plan_sha256,
        &inventory_sha256,
    )?;
    if input.candidate_sha256.len() < 40 || input.evidence_root.trim().is_empty() {
        return Err(ClosureError::Invalid(
            "candidate digest or evidence root is empty".to_owned(),
        ));
    }
    let nonce_seed = format!(
        "{}:{}:{}:{}",
        contract.recovery_run_id,
        contract.reviewed_plan_sha256,
        input.candidate_sha256,
        manifest_sha256
    );
    let nonce = hash_bytes(nonce_seed.as_bytes())
        .map_err(|error| ClosureError::Invalid(format!("nonce hash: {error}")))?;
    let mut receipt = serde_json::json!({
        "schema_version": RECEIPT_SCHEMA,
        "status": "verified",
        "recovery_run_id": contract.recovery_run_id,
        "reviewed_plan_sha256": plan_sha256,
        "requirement_inventory_sha256": inventory_sha256,
        "coverage_manifest_sha256": manifest_sha256,
        "candidate_sha256": input.candidate_sha256,
        "evidence_root": input.evidence_root,
        "nonce": nonce,
    });
    receipt["receipt_path"] =
        Value::String(format!("{}/closure-receipt.json", input.evidence_root));
    let receipt_bytes = serde_json::to_vec(&receipt)
        .map_err(|error| ClosureError::Json(format!("receipt: {error}")))?;
    let receipt_sha256 = hash_bytes(&receipt_bytes)
        .map_err(|error| ClosureError::Invalid(format!("receipt hash: {error}")))?;
    receipt["receipt_sha256"] = Value::String(receipt_sha256.clone());
    let json = serde_json::to_string_pretty(&receipt)
        .map_err(|error| ClosureError::Json(format!("receipt: {error}")))?;
    Ok(ClosureReceipt {
        json,
        receipt_sha256,
    })
}

fn validate_inventory_plan(plan_bytes: &[u8], inventory_json: &str) -> Result<(), ClosureError> {
    let inventory: crate::tui_fidelity_matrix::RequirementInventory =
        serde_json::from_str(inventory_json)
            .map_err(|error| ClosureError::Json(format!("inventory: {error}")))?;
    let plan = std::str::from_utf8(plan_bytes)
        .map_err(|error| ClosureError::Invalid(format!("plan is not UTF-8: {error}")))?;
    let lines = plan.lines().collect::<Vec<_>>();
    for requirement in inventory.requirements {
        let line_index =
            usize::try_from(requirement.source_line.saturating_sub(1)).map_err(|error| {
                ClosureError::Invalid(format!(
                    "{} has invalid source line: {error}",
                    requirement.id
                ))
            })?;
        let line = lines
            .get(line_index)
            .ok_or_else(|| {
                ClosureError::Invalid(format!("{} points outside plan", requirement.id))
            })?
            .trim();
        if requirement.id.starts_with("global.module-tier.")
            || requirement.id.starts_with("global.terminal.")
            || requirement.id.starts_with("todo.56.journey.")
            || requirement.id.ends_with(".acceptance")
        {
            if line.is_empty() {
                return Err(ClosureError::Invalid(format!(
                    "{} source line is empty",
                    requirement.id
                )));
            }
        } else if requirement.id.starts_with("todo.") || requirement.id.starts_with("final.") {
            let expected_label = requirement.id.split('.').nth(1).ok_or_else(|| {
                ClosureError::Invalid(format!("{} has no task label", requirement.id))
            })?;
            let prefix = line
                .strip_prefix("- [x] ")
                .or_else(|| line.strip_prefix("- [ ] "))
                .ok_or_else(|| {
                    ClosureError::Invalid(format!(
                        "{} source line is not a checklist row",
                        requirement.id
                    ))
                })?;
            let title = prefix
                .strip_prefix(expected_label)
                .and_then(|value| value.strip_prefix(". "))
                .ok_or_else(|| {
                    ClosureError::Invalid(format!(
                        "{} source label differs from plan",
                        requirement.id
                    ))
                })?;
            if title != requirement.title {
                return Err(ClosureError::Invalid(format!(
                    "{} title differs from plan",
                    requirement.id
                )));
            }
        } else if !line.contains(&requirement.title) {
            return Err(ClosureError::Invalid(format!(
                "{} title is not traceable",
                requirement.id
            )));
        }
    }
    Ok(())
}

fn validate_revocation(revocation_json: &str, plan_sha256: &str) -> Result<(), ClosureError> {
    let revocation: Value = serde_json::from_str(revocation_json)
        .map_err(|error| ClosureError::Json(format!("revocation: {error}")))?;
    let object = revocation
        .as_object()
        .ok_or_else(|| ClosureError::Invalid("revocation is not an object".to_owned()))?;
    let reviewed_plan = object
        .get("reviewed_plan_sha256")
        .and_then(Value::as_str)
        .or_else(|| {
            object
                .get("completion_guard")
                .and_then(Value::as_object)
                .and_then(|guard| guard.get("reviewed_plan_sha256"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            object
                .get("plan_review")
                .and_then(Value::as_object)
                .and_then(|review| review.get("plan_sha256"))
                .and_then(Value::as_str)
        });
    if object.get("status").and_then(Value::as_str) != Some("REJECTED")
        || reviewed_plan != Some(plan_sha256)
    {
        return Err(ClosureError::Invalid(
            "revocation record is not plan-bound and rejected".to_owned(),
        ));
    }
    Ok(())
}

fn validate_reviews(
    contract: &ClosureContract,
    inputs: &[ReviewReceiptInput],
    plan_sha256: &str,
    inventory_sha256: &str,
) -> Result<(), ClosureError> {
    if contract.preliminary_reviews.len() != 2 || inputs.len() != 2 {
        return Err(ClosureError::Invalid(
            "exactly two preliminary reviews are required".to_owned(),
        ));
    }
    let mut sessions = std::collections::BTreeSet::new();
    for reference in &contract.preliminary_reviews {
        let input = inputs
            .iter()
            .find(|item| item.review_id == reference.review_id)
            .ok_or_else(|| {
                ClosureError::Invalid(format!(
                    "missing preliminary review {}",
                    reference.review_id
                ))
            })?;
        let review: Value = serde_json::from_str(&input.json).map_err(|error| {
            ClosureError::Json(format!("review {}: {error}", reference.review_id))
        })?;
        let object = review
            .as_object()
            .ok_or_else(|| ClosureError::Invalid("review is not an object".to_owned()))?;
        if object.get("verdict").and_then(Value::as_str) != Some("APPROVE")
            || object.get("plan_sha256").and_then(Value::as_str) != Some(plan_sha256)
            || object.get("inventory_sha256").and_then(Value::as_str) != Some(inventory_sha256)
        {
            return Err(ClosureError::Invalid(format!(
                "preliminary review {} is not approved and bound",
                reference.review_id
            )));
        }
        let session = object
            .get("reviewer_session")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ClosureError::Invalid(format!(
                    "preliminary review {} has no reviewer session",
                    reference.review_id
                ))
            })?;
        if !sessions.insert(session.to_owned()) {
            return Err(ClosureError::Invalid(
                "preliminary reviews share a reviewer session".to_owned(),
            ));
        }
    }
    Ok(())
}

impl fmt::Display for ClosureReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "closure receipt {}", self.receipt_sha256)
    }
}

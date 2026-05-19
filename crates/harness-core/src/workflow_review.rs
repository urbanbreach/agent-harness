use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const REVIEW_RECOMMENDATION_METADATA_KEY: &str = "recommendation";
pub const REVIEW_ARCHITECTURAL_STATUS_METADATA_KEY: &str = "architectural_status";
pub const REVIEW_FINDINGS_METADATA_KEY: &str = "findings";
pub const AUTOPILOT_WORKFLOW_ID_METADATA_KEY: &str = "autopilot_workflow_id";
pub const PARENT_WORKFLOW_ID_METADATA_KEY: &str = "parent_workflow_id";
pub const SOURCE_REVIEW_WORKFLOW_ID_METADATA_KEY: &str = "source_review_workflow_id";
pub const RETURN_TO_RALPLAN_REASON_METADATA_KEY: &str = "return_to_ralplan_reason";
pub const REVIEW_VERDICT_METADATA_KEY: &str = "review_verdict";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CodeReviewVerdict {
    pub recommendation: String,
    pub architectural_status: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<String>,
}

impl CodeReviewVerdict {
    pub fn is_clean(&self) -> bool {
        self.recommendation.eq_ignore_ascii_case("APPROVE")
            && self.architectural_status.eq_ignore_ascii_case("CLEAR")
    }

    pub fn normalized_recommendation(&self) -> String {
        normalize_verdict_token(&self.recommendation)
    }

    pub fn normalized_architectural_status(&self) -> String {
        normalize_verdict_token(&self.architectural_status)
    }
}

pub fn code_review_verdict_from_evidence(
    summary: &str,
    metadata: &BTreeMap<String, String>,
) -> Option<CodeReviewVerdict> {
    let summary_json = serde_json::from_str::<Value>(summary).ok();
    let recommendation = metadata_string(metadata, REVIEW_RECOMMENDATION_METADATA_KEY)
        .or_else(|| metadata_string(metadata, "code_review_recommendation"))
        .or_else(|| json_string(summary_json.as_ref(), REVIEW_RECOMMENDATION_METADATA_KEY))
        .or_else(|| json_string(summary_json.as_ref(), "recommendation"));
    let architectural_status = metadata_string(metadata, REVIEW_ARCHITECTURAL_STATUS_METADATA_KEY)
        .or_else(|| metadata_string(metadata, "architect_status"))
        .or_else(|| metadata_string(metadata, "architectStatus"))
        .or_else(|| metadata_string(metadata, "architecture_status"))
        .or_else(|| {
            json_string(
                summary_json.as_ref(),
                REVIEW_ARCHITECTURAL_STATUS_METADATA_KEY,
            )
        })
        .or_else(|| json_string(summary_json.as_ref(), "architectStatus"))
        .or_else(|| json_string(summary_json.as_ref(), "architect_status"));
    let findings = metadata_string(metadata, REVIEW_FINDINGS_METADATA_KEY)
        .map(|value| parse_findings_string(&value))
        .or_else(|| json_findings(summary_json.as_ref()))
        .unwrap_or_default();

    Some(CodeReviewVerdict {
        recommendation: normalize_verdict_token(&recommendation?),
        architectural_status: normalize_verdict_token(&architectural_status?),
        findings,
    })
}

pub fn review_return_to_ralplan_reason(verdict: &CodeReviewVerdict) -> String {
    let recommendation = verdict.normalized_recommendation();
    let architectural_status = verdict.normalized_architectural_status();
    let mut reason = format!(
        "code review not clean: recommendation={recommendation}, architectural_status={architectural_status}"
    );
    if !verdict.findings.is_empty() {
        reason.push_str(&format!(", findings={}", verdict.findings.join("; ")));
    }
    reason
}

fn metadata_string(metadata: &BTreeMap<String, String>, key: &str) -> Option<String> {
    metadata
        .get(key)
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn json_string(value: Option<&Value>, key: &str) -> Option<String> {
    value?
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn json_findings(value: Option<&Value>) -> Option<Vec<String>> {
    let findings = value?.get(REVIEW_FINDINGS_METADATA_KEY)?;
    if let Some(values) = findings.as_array() {
        let findings = values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        return Some(findings);
    }
    findings.as_str().map(parse_findings_string)
}

fn parse_findings_string(value: &str) -> Vec<String> {
    value
        .split(['\n', ';'])
        .map(str::trim)
        .filter(|finding| !finding.is_empty())
        .map(str::to_string)
        .collect()
}

fn normalize_verdict_token(value: &str) -> String {
    value.trim().replace([' ', '-'], "_").to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::{code_review_verdict_from_evidence, review_return_to_ralplan_reason};

    #[test]
    fn parses_code_review_verdict_from_json_summary() {
        let verdict = code_review_verdict_from_evidence(
            r#"{"recommendation":"APPROVE","architectural_status":"CLEAR","findings":[]}"#,
            &Default::default(),
        )
        .expect("review verdict");

        assert!(verdict.is_clean());
        assert_eq!(verdict.recommendation, "APPROVE");
        assert_eq!(verdict.architectural_status, "CLEAR");
    }

    #[test]
    fn parses_code_review_verdict_from_metadata_and_formats_loopback_reason() {
        let verdict = code_review_verdict_from_evidence(
            "review found blockers",
            &std::collections::BTreeMap::from([
                ("recommendation".to_string(), "REQUEST CHANGES".to_string()),
                ("architectStatus".to_string(), "WATCH".to_string()),
                ("findings".to_string(), "fix tests; update docs".to_string()),
            ]),
        )
        .expect("review verdict");

        assert!(!verdict.is_clean());
        assert_eq!(verdict.recommendation, "REQUEST_CHANGES");
        assert_eq!(
            review_return_to_ralplan_reason(&verdict),
            "code review not clean: recommendation=REQUEST_CHANGES, architectural_status=WATCH, findings=fix tests; update docs"
        );
    }
}

use super::error::ComparatorError;

pub fn reject_self_comparison(
    reference_sha256: &str,
    candidate_sha256: &str,
) -> Result<(), ComparatorError> {
    if reference_sha256.is_empty() || candidate_sha256.is_empty() {
        return Err(ComparatorError::Invalid {
            detail: "binary SHA-256 values must not be empty".to_owned(),
        });
    }
    if reference_sha256 == candidate_sha256 {
        Err(ComparatorError::SelfComparison {
            sha256: reference_sha256.to_owned(),
        })
    } else {
        Ok(())
    }
}

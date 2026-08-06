use super::{
    BinaryIdentity, BinaryReceipt, BinaryReceiptError, BuildFingerprint, ReceiptExpectations,
    RepeatBuildReceipt, BINARY_RECEIPT_SCHEMA,
};
use std::path::Path;

#[path = "validation_helpers.rs"]
mod validation_helpers;

use validation_helpers::{
    invalid, require_equal, verify_identity, verify_repeat, IdentityExpectation, RepeatExpectation,
};

impl BinaryReceipt {
    pub fn verify(&self, expected: &ReceiptExpectations) -> Result<(), BinaryReceiptError> {
        require_equal(
            "schema_version".to_owned(),
            BINARY_RECEIPT_SCHEMA,
            &self.schema_version,
        )?;
        verify_identity(
            "reference",
            &self.reference,
            &IdentityExpectation {
                source_revision: &expected.reference_revision,
                clean_pre: expected.reference_clean_pre,
                clean_post: expected.reference_clean_post,
                package: &expected.reference_package,
                executable: &expected.reference_executable,
            },
        )?;
        verify_identity(
            "harness",
            &self.harness,
            &IdentityExpectation {
                source_revision: &expected.harness_revision,
                clean_pre: expected.harness_clean_pre,
                clean_post: expected.harness_clean_post,
                package: &expected.harness_package,
                executable: &expected.harness_executable,
            },
        )?;
        verify_repeat(
            "reference_repeat",
            &self.reference_repeat,
            &RepeatExpectation {
                identity: &self.reference,
                source_revision: &expected.reference_revision,
            },
        )?;
        verify_repeat(
            "harness_repeat",
            &self.harness_repeat,
            &RepeatExpectation {
                identity: &self.harness,
                source_revision: &expected.harness_revision,
            },
        )?;
        for (field, accepted) in [
            (
                "mutation_probe.wrong_revision_rejected",
                self.mutation_probe.wrong_revision_rejected,
            ),
            (
                "mutation_probe.mutated_digest_rejected",
                self.mutation_probe.mutated_digest_rejected,
            ),
        ] {
            if !accepted {
                return Err(invalid(field, "must be true"));
            }
        }
        Ok(())
    }

    pub fn verify_binary_digests(&self) -> Result<(), BinaryReceiptError> {
        macro_rules! verify_digests {
            ($($field:literal => $path:expr, $expected:expr);+ $(;)?) => {
                $(super::digest::verify_binary_digest($field, Path::new($path), $expected)?;)+
            };
        }
        verify_digests! {
            "reference.sha256" => self.reference.binary_path.as_str(), self.reference.sha256.as_str();
            "harness.sha256" => self.harness.binary_path.as_str(), self.harness.sha256.as_str();
            "reference_repeat.first.binary_sha256" => self.reference_repeat.first_binary_path.as_str(), self.reference_repeat.first.binary_sha256.as_str();
            "reference_repeat.second.binary_sha256" => self.reference_repeat.second_binary_path.as_str(), self.reference_repeat.second.binary_sha256.as_str();
            "harness_repeat.first.binary_sha256" => self.harness_repeat.first_binary_path.as_str(), self.harness_repeat.first.binary_sha256.as_str();
            "harness_repeat.second.binary_sha256" => self.harness_repeat.second_binary_path.as_str(), self.harness_repeat.second.binary_sha256.as_str();
        }
        Ok(())
    }
}

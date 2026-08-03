use super::{
    BinaryIdentity, BinaryReceipt, BinaryReceiptError, BuildFingerprint, ReceiptExpectations,
    RepeatBuildReceipt, BINARY_RECEIPT_SCHEMA,
};
use std::path::Path;

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
        if !self.mutation_probe.wrong_revision_rejected {
            return Err(invalid(
                "mutation_probe.wrong_revision_rejected",
                "must be true",
            ));
        }
        if !self.mutation_probe.mutated_digest_rejected {
            return Err(invalid(
                "mutation_probe.mutated_digest_rejected",
                "must be true",
            ));
        }
        Ok(())
    }

    pub fn verify_binary_digests(&self) -> Result<(), BinaryReceiptError> {
        super::digest::verify_binary_digest(
            "reference.sha256",
            Path::new(&self.reference.binary_path),
            &self.reference.sha256,
        )?;
        super::digest::verify_binary_digest(
            "harness.sha256",
            Path::new(&self.harness.binary_path),
            &self.harness.sha256,
        )
    }
}

struct IdentityExpectation<'a> {
    source_revision: &'a str,
    clean_pre: bool,
    clean_post: bool,
    package: &'a str,
    executable: &'a str,
}

struct RepeatExpectation<'a> {
    identity: &'a BinaryIdentity,
    source_revision: &'a str,
}

fn verify_identity(
    label: &str,
    identity: &BinaryIdentity,
    expected: &IdentityExpectation<'_>,
) -> Result<(), BinaryReceiptError> {
    require_equal(
        format!("{label}.source_revision"),
        expected.source_revision,
        &identity.source_revision,
    )?;
    require_equal(
        format!("{label}.package"),
        expected.package,
        &identity.package,
    )?;
    require_equal(
        format!("{label}.executable"),
        expected.executable,
        &identity.executable,
    )?;
    if identity.clean_pre != expected.clean_pre {
        return Err(mismatch(
            format!("{label}.clean_pre"),
            expected.clean_pre,
            identity.clean_pre,
        ));
    }
    if identity.clean_post != expected.clean_post {
        return Err(mismatch(
            format!("{label}.clean_post"),
            expected.clean_post,
            identity.clean_post,
        ));
    }
    require_absolute_path(format!("{label}.target_dir"), &identity.target_dir)?;
    require_absolute_path(format!("{label}.binary_path"), &identity.binary_path)?;
    if identity.version.trim().is_empty() {
        return Err(invalid(format!("{label}.version"), "must be non-empty"));
    }
    for (field, value) in [
        ("source_sha256", &identity.source_sha256),
        ("sha256", &identity.sha256),
        ("cargo_lock_sha256", &identity.cargo_lock_sha256),
        ("toolchain_sha256", &identity.toolchain_sha256),
        ("rustc_sha256", &identity.rustc_sha256),
        ("cargo_sha256", &identity.cargo_sha256),
    ] {
        require_sha256(format!("{label}.{field}"), value)?;
    }
    Ok(())
}

fn verify_repeat(
    label: &str,
    repeat: &RepeatBuildReceipt,
    expected: &RepeatExpectation<'_>,
) -> Result<(), BinaryReceiptError> {
    if !repeat.matching {
        return Err(invalid(format!("{label}.matching"), "must be true"));
    }
    for (field, value) in [
        ("first_target_dir", &repeat.first_target_dir),
        ("second_target_dir", &repeat.second_target_dir),
        ("first_binary_path", &repeat.first_binary_path),
        ("second_binary_path", &repeat.second_binary_path),
    ] {
        require_absolute_path(format!("{label}.{field}"), value)?;
    }
    verify_fingerprint(
        &format!("{label}.first"),
        &repeat.first,
        expected.source_revision,
    )?;
    verify_fingerprint(
        &format!("{label}.second"),
        &repeat.second,
        expected.source_revision,
    )?;
    if repeat.first != repeat.second {
        return Err(invalid(
            format!("{label}.matching"),
            "first and second differ",
        ));
    }
    if repeat.first != fingerprint(expected.identity) {
        return Err(invalid(
            format!("{label}.matching"),
            "repeat identity differs from primary identity",
        ));
    }
    Ok(())
}

fn verify_fingerprint(
    label: &str,
    fingerprint: &BuildFingerprint,
    expected_revision: &str,
) -> Result<(), BinaryReceiptError> {
    require_equal(
        format!("{label}.source_revision"),
        expected_revision,
        &fingerprint.source_revision,
    )?;
    if fingerprint.version.trim().is_empty() {
        return Err(invalid(format!("{label}.version"), "must be non-empty"));
    }
    for (field, value) in [
        ("source_sha256", &fingerprint.source_sha256),
        ("cargo_lock_sha256", &fingerprint.cargo_lock_sha256),
        ("toolchain_sha256", &fingerprint.toolchain_sha256),
        ("rustc_sha256", &fingerprint.rustc_sha256),
        ("cargo_sha256", &fingerprint.cargo_sha256),
        ("binary_sha256", &fingerprint.binary_sha256),
    ] {
        require_sha256(format!("{label}.{field}"), value)?;
    }
    Ok(())
}

fn fingerprint(identity: &BinaryIdentity) -> BuildFingerprint {
    BuildFingerprint {
        source_revision: identity.source_revision.clone(),
        source_sha256: identity.source_sha256.clone(),
        cargo_lock_sha256: identity.cargo_lock_sha256.clone(),
        toolchain_sha256: identity.toolchain_sha256.clone(),
        rustc_sha256: identity.rustc_sha256.clone(),
        cargo_sha256: identity.cargo_sha256.clone(),
        binary_sha256: identity.sha256.clone(),
        version: identity.version.clone(),
    }
}

fn require_equal(field: String, expected: &str, actual: &str) -> Result<(), BinaryReceiptError> {
    if expected == actual {
        Ok(())
    } else {
        Err(BinaryReceiptError::Mismatch {
            field,
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        })
    }
}

fn require_sha256(field: String, value: &str) -> Result<(), BinaryReceiptError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(invalid(
            field,
            "must be a 64-character hexadecimal SHA-256 digest",
        ))
    }
}

fn mismatch(field: String, expected: bool, actual: bool) -> BinaryReceiptError {
    BinaryReceiptError::Mismatch {
        field,
        expected: expected.to_string(),
        actual: actual.to_string(),
    }
}

fn require_absolute_path(field: String, value: &str) -> Result<(), BinaryReceiptError> {
    if Path::new(value).is_absolute() {
        Ok(())
    } else {
        Err(invalid(field, "must be absolute"))
    }
}

fn invalid(field: impl Into<String>, reason: impl Into<String>) -> BinaryReceiptError {
    BinaryReceiptError::InvalidField {
        field: field.into(),
        reason: reason.into(),
    }
}

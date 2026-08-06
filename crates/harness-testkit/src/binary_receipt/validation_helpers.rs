use super::super::{BinaryIdentity, BinaryReceiptError, BuildFingerprint, RepeatBuildReceipt};
use std::path::Path;

pub(super) struct IdentityExpectation<'a> {
    pub(super) source_revision: &'a str,
    pub(super) clean_pre: bool,
    pub(super) clean_post: bool,
    pub(super) package: &'a str,
    pub(super) executable: &'a str,
}

pub(super) struct RepeatExpectation<'a> {
    pub(super) identity: &'a BinaryIdentity,
    pub(super) source_revision: &'a str,
}

pub(super) fn verify_identity(
    label: &str,
    identity: &BinaryIdentity,
    expected: &IdentityExpectation<'_>,
) -> Result<(), BinaryReceiptError> {
    for (field, expected_value, actual_value) in [
        (
            "source_revision",
            expected.source_revision,
            identity.source_revision.as_str(),
        ),
        ("package", expected.package, identity.package.as_str()),
        (
            "executable",
            expected.executable,
            identity.executable.as_str(),
        ),
    ] {
        require_equal(format!("{label}.{field}"), expected_value, actual_value)?;
    }
    for (field, expected_value, actual_value) in [
        ("clean_pre", expected.clean_pre, identity.clean_pre),
        ("clean_post", expected.clean_post, identity.clean_post),
    ] {
        if expected_value != actual_value {
            return Err(mismatch(
                format!("{label}.{field}"),
                expected_value,
                actual_value,
            ));
        }
    }
    require_absolute_path(format!("{label}.target_dir"), &identity.target_dir)?;
    require_absolute_path(format!("{label}.binary_path"), &identity.binary_path)?;
    if identity.version.trim().is_empty() {
        return Err(invalid(format!("{label}.version"), "must be non-empty"));
    }
    verify_sha256_fields(
        label,
        [
            ("source_sha256", &identity.source_sha256),
            ("sha256", &identity.sha256),
            ("cargo_lock_sha256", &identity.cargo_lock_sha256),
            ("toolchain_sha256", &identity.toolchain_sha256),
            ("rustc_sha256", &identity.rustc_sha256),
            ("cargo_sha256", &identity.cargo_sha256),
        ],
    )
}

pub(super) fn verify_repeat(
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
    for (name, fingerprint) in [("first", &repeat.first), ("second", &repeat.second)] {
        verify_fingerprint(
            &format!("{label}.{name}"),
            fingerprint,
            expected.source_revision,
        )?;
    }
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
        fingerprint.source_revision.as_str(),
    )?;
    if fingerprint.version.trim().is_empty() {
        return Err(invalid(format!("{label}.version"), "must be non-empty"));
    }
    verify_sha256_fields(
        label,
        [
            ("source_sha256", &fingerprint.source_sha256),
            ("cargo_lock_sha256", &fingerprint.cargo_lock_sha256),
            ("toolchain_sha256", &fingerprint.toolchain_sha256),
            ("rustc_sha256", &fingerprint.rustc_sha256),
            ("cargo_sha256", &fingerprint.cargo_sha256),
            ("binary_sha256", &fingerprint.binary_sha256),
        ],
    )
}

fn verify_sha256_fields<const N: usize>(
    label: &str,
    fields: [(&str, &String); N],
) -> Result<(), BinaryReceiptError> {
    for (field, value) in fields {
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

pub(super) fn require_equal(
    field: String,
    expected: &str,
    actual: &str,
) -> Result<(), BinaryReceiptError> {
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

pub(super) fn invalid(field: impl Into<String>, reason: impl Into<String>) -> BinaryReceiptError {
    BinaryReceiptError::InvalidField {
        field: field.into(),
        reason: reason.into(),
    }
}

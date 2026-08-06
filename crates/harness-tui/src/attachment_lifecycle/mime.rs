use super::AttachmentError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimeKind {
    Png,
    Jpeg,
    PlainText,
    Binary,
    Zip,
    Unknown,
}

impl MimeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::PlainText => "text/plain",
            Self::Binary => "application/octet-stream",
            Self::Zip => "application/zip",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_allowed(self) -> bool {
        matches!(
            self,
            Self::Png | Self::Jpeg | Self::PlainText | Self::Binary
        )
    }
}

pub fn sniff(bytes: &[u8]) -> MimeKind {
    if bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]) {
        return MimeKind::Png;
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return MimeKind::Jpeg;
    }
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return MimeKind::Zip;
    }
    if std::str::from_utf8(bytes).is_ok() {
        MimeKind::PlainText
    } else {
        MimeKind::Unknown
    }
}

pub fn resolve(bytes: &[u8], hint: Option<MimeKind>) -> Result<MimeKind, AttachmentError> {
    let detected = sniff(bytes);
    if detected.is_allowed() {
        return Ok(detected);
    }
    if detected == MimeKind::Zip {
        return Ok(detected);
    }
    if detected == MimeKind::Unknown {
        if let Some(hint) = hint.filter(|mime| mime.is_allowed()) {
            return Ok(hint);
        }
    }
    Err(AttachmentError::MimeRejected { mime: detected })
}

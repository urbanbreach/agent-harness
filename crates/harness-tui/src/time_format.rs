pub(crate) fn iso_timestamp_minute(timestamp: &str) -> Option<&str> {
    let trimmed = timestamp.trim();
    if trimmed.len() >= 16 && trimmed.as_bytes().get(10) == Some(&b'T') {
        Some(&trimmed[..16])
    } else {
        None
    }
}

pub(crate) fn short_time_or_trimmed(timestamp: &str) -> String {
    if let Some(minute) = iso_timestamp_minute(timestamp) {
        minute[11..].to_string()
    } else {
        timestamp.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_timestamp_minute_extracts_trimmed_prefix() {
        assert_eq!(
            iso_timestamp_minute(" 2026-03-08T12:34:56Z "),
            Some("2026-03-08T12:34")
        );
    }

    #[test]
    fn iso_timestamp_minute_rejects_non_iso_text() {
        assert_eq!(iso_timestamp_minute("2026-03-08 12:34:56"), None);
        assert_eq!(iso_timestamp_minute(""), None);
    }

    #[test]
    fn short_time_or_trimmed_formats_iso_and_fallbacks() {
        assert_eq!(short_time_or_trimmed(" 2026-03-08T12:34:56Z "), "12:34");
        assert_eq!(short_time_or_trimmed(" not iso "), "not iso");
    }
}

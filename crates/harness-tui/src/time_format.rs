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

/// Freeze-matched wall clock on the user row (`9:33 AM`), not 24h `09:33`.
///
/// Identity-masked exact value may differ; geometry/style must match the freeze.
pub(crate) fn wall_clock_12h(timestamp: &str) -> String {
    let Some(minute) = iso_timestamp_minute(timestamp) else {
        return timestamp.trim().to_string();
    };
    let hour_raw = &minute[11..13];
    let mins = &minute[14..16];
    let Ok(hour24) = hour_raw.parse::<u8>() else {
        return short_time_or_trimmed(timestamp);
    };
    let (hour12, meridiem) = match hour24 {
        0 => (12_u8, "AM"),
        1..=11 => (hour24, "AM"),
        12 => (12, "PM"),
        13..=23 => (hour24 - 12, "PM"),
        _ => return short_time_or_trimmed(timestamp),
    };
    format!("{hour12}:{mins} {meridiem}")
}

pub(crate) fn wall_clock_hover_detail(timestamp: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let trimmed = timestamp.trim();
    let Some(date_time) = trimmed.get(..19) else {
        return trimmed.to_string();
    };
    let bytes = date_time.as_bytes();
    if bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return trimmed.to_string();
    }
    let Ok(month) = date_time[5..7].parse::<usize>() else {
        return trimmed.to_string();
    };
    let Some(month_name) = month.checked_sub(1).and_then(|index| MONTHS.get(index)) else {
        return trimmed.to_string();
    };
    format!(
        "{} | {month_name} {}",
        &date_time[11..19],
        &date_time[8..10]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_timestamp_minute_extracts_trimmed_prefix() {
        // arrange
        // act
        // assert
        assert_eq!(
            iso_timestamp_minute(" 2026-03-08T12:34:56Z "),
            Some("2026-03-08T12:34")
        );
    }

    #[test]
    fn iso_timestamp_minute_rejects_non_iso_text() {
        // arrange
        // act
        // assert
        assert_eq!(iso_timestamp_minute("2026-03-08 12:34:56"), None);
        assert_eq!(iso_timestamp_minute(""), None);
    }

    #[test]
    fn short_time_or_trimmed_formats_iso_and_fallbacks() {
        // arrange
        // act
        // assert
        assert_eq!(short_time_or_trimmed(" 2026-03-08T12:34:56Z "), "12:34");
        assert_eq!(short_time_or_trimmed(" not iso "), "not iso");
    }

    #[test]
    fn wall_clock_12h_formats_freeze_style() {
        // arrange
        // act
        // assert
        assert_eq!(wall_clock_12h("2026-03-19T09:33:00Z"), "9:33 AM");
        assert_eq!(wall_clock_12h("2026-03-19T00:05:00Z"), "12:05 AM");
        assert_eq!(wall_clock_12h("2026-03-19T12:00:00Z"), "12:00 PM");
        assert_eq!(wall_clock_12h("2026-03-19T21:45:00Z"), "9:45 PM");
        assert_eq!(wall_clock_12h(" already local "), "already local");
    }
}

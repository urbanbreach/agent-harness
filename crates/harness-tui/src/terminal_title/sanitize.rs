#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitationReport {
    pub control_chars_stripped: usize,
    pub osc_sequences_stripped: usize,
    pub csi_sequences_stripped: usize,
    pub truncated: bool,
    pub original_len: usize,
    pub sanitized_len: usize,
}

pub fn sanitize_title(raw: &str) -> String {
    sanitize_title_traced(raw).0
}

pub fn sanitize_title_traced(raw: &str) -> (String, SanitationReport) {
    let original_len = raw.chars().count();
    let bytes = raw.as_bytes();
    let mut output = String::with_capacity(raw.len());
    let mut control_chars_stripped = 0;
    let mut osc_sequences_stripped = 0;
    let mut csi_sequences_stripped = 0;
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == 0x1b && index + 1 < bytes.len() && bytes[index + 1] == b']' {
            osc_sequences_stripped += 1;
            index += 2;
            while index < bytes.len() {
                if bytes[index] == 0x07 {
                    index += 1;
                    break;
                }
                if bytes[index] == 0x1b && index + 1 < bytes.len() && bytes[index + 1] == b'\\' {
                    index += 2;
                    break;
                }
                index += 1;
            }
            continue;
        }
        if bytes[index] == 0x1b && index + 1 < bytes.len() && bytes[index + 1] == b'[' {
            csi_sequences_stripped += 1;
            index += 2;
            while index < bytes.len() {
                let byte = bytes[index];
                index += 1;
                if (0x40..=0x7e).contains(&byte) {
                    break;
                }
            }
            continue;
        }
        let Some(character) = raw[index..].chars().next() else {
            break;
        };
        index += character.len_utf8();
        if character == '\t' {
            output.push(' ');
        } else if character.is_control() || ('\u{7f}'..='\u{9f}').contains(&character) {
            control_chars_stripped += 1;
        } else {
            output.push(character);
        }
    }

    let truncated = output.chars().count() > 200;
    let sanitized = output.chars().take(200).collect::<String>();
    let sanitized = sanitized.trim().to_string();
    let report = SanitationReport {
        control_chars_stripped,
        osc_sequences_stripped,
        csi_sequences_stripped,
        truncated,
        original_len,
        sanitized_len: sanitized.chars().count(),
    };
    (sanitized, report)
}

use super::{ArchiveOutput, ArchiveShape, Ink};

fn grouped(number: u64) -> String {
    let digits = number.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub(super) fn short_summary(chars: u64, frames: usize, text_chars: u64) -> String {
    let plural = if frames == 1 { "" } else { "s" };
    let mut summary = format!(
        "Archived {} chars of history onto {frames} archive frame{plural}",
        grouped(chars)
    );
    if text_chars > 0 {
        summary.push_str(&format!(" (+{} chars as text)", grouped(text_chars)));
    }
    summary
}

/// The reading guide. `rows` is the largest unique-row capacity across the planned frame
/// shapes; the reasoning legend appears only when the retained source really has a
/// reasoning scope (a literal inside dim tool output does not count).
pub(super) fn lead_in(
    output: &ArchiveOutput,
    shape: &ArchiveShape,
    widths: &[u32],
    rows: u32,
) -> String {
    if output.source.text.is_empty() && output.frames.is_empty() {
        return "No prior history.".to_owned();
    }
    let mut guide=String::from("Read the chronological HISTORY below in full, then continue the live conversation after it.\n\nScope labels: ¶user: user input; ¶ai: assistant reply; ¶call: tool invocation.\n");
    if super::scope::has_reasoning_scope(&output.source.text) {
        guide.push_str("¶think: assistant reasoning.\n");
    }
    guide.push_str("Unprefixed lines continue the current scope. Adjacent equal scopes share a label. Calls use ¶call:name(args)//intent; the intent comment is optional. <out>…</out> encloses tool output.\n\nPlain text is the authoritative transcript. ");
    if !output.frames.is_empty() {
        guide.push_str("Read image pages in order between the marked text edges. A solid FULL BLOCK represents a collapsed newline. ");
        if shape.columns == 2 {
            guide.push_str("Read the left column top to bottom, then the right column. ");
        } else {
            guide.push_str(
                "Read each grid left to right, then top to bottom; there is no word wrapping. ",
            );
        }
        let widths = widths
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(" or ");
        guide.push_str(&format!(
            "Pages have {widths} columns and up to {rows} unique rows. "
        ));
        if shape.ink == Ink::Sentence {
            guide.push_str("Sentence hues cycle after sentence punctuation. ");
        }
        if shape.stopword_dim {
            guide.push_str("Common words are dimmed without changing their text. ");
        }
        if shape.line_repeat > 1 {
            guide.push_str("Repeated bands contain identical lines, not additional history. ");
        }
    }
    if output.source.text.contains("[Summary of earlier history]") {
        guide.push_str(
            "An earlier-history digest summarizes older material rather than quoting it. ",
        );
    }
    if output.source.truncated_chars > 0 {
        guide.push_str(&format!(
            "{} characters of older middle history were omitted. ",
            grouped(output.source.truncated_chars)
        ));
    }
    guide.push_str("Recover unclear exact details from the workspace instead of guessing.\n\nHISTORY\n===================");
    guide
}

/// Add the shared grouped file tree before HISTORY without duplicating path rules.
pub fn lead_in_with_files(output: &ArchiveOutput, files: &crate::files::FileOps) -> String {
    let (read, modified) = crate::files::file_lists(files);
    let block = crate::files::render_files_block(&read, &modified, &files.read);
    if block.is_empty() {
        return output.lead_in.clone();
    }
    let tree = block
        .strip_prefix("<files>")
        .unwrap_or(&block)
        .strip_suffix("</files>")
        .unwrap_or(&block)
        .trim();
    let intro = output
        .lead_in
        .strip_suffix("HISTORY\n===================")
        .unwrap_or("");
    format!("{intro}FILES\n===================\n{tree}\n\nHISTORY\n===================")
}

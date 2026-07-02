use super::ui_secondary::format_detail_payload;

pub(super) struct CollapsibleOutputPreview {
    pub(super) output: String,
    pub(super) expand_hint: Option<&'static str>,
}

pub(super) fn collapsible_output_preview(output: &str) -> CollapsibleOutputPreview {
    let formatted = format_detail_payload(output);
    full_output_preview(&formatted)
}

pub(super) fn collapsible_bash_panel_preview(output: &str) -> CollapsibleOutputPreview {
    full_output_preview(output)
}

fn full_output_preview(output: &str) -> CollapsibleOutputPreview {
    CollapsibleOutputPreview {
        output: output.to_string(),
        expand_hint: None,
    }
}

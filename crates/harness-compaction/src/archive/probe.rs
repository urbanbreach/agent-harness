use super::fonts::cell_width;
use super::shapes::{is_explicit, resolve_shape, resolve_shape_on, resolve_silver, ShapeHost};
use super::{font_coverage, normalize_text, ArchiveShape};

/// Minimum East Asian wide graphics for text to count as CJK-heavy.
const MIN_WIDE: usize = 8;

/// Resolve geometry for the text that will actually be rendered.
///
/// Starts from the model and provider choice. If that shape's font cannot draw the text
/// safely, Silver replaces it when Silver can; if neither is safe the original shape stays
/// so preflight rejects it. A safe original yields to Silver only for CJK-heavy text: at
/// least eight East Asian wide graphics making up a quarter of the non-space graphics,
/// when Silver is also safe. An explicit variant is never changed. `probe` is the prior
/// archive source (or prior summary) and a FULL BLOCK before the fresh serialization.
pub fn resolve_shape_for_text(
    model_id: &str,
    provider_family: &str,
    explicit: Option<&str>,
    probe: &str,
) -> &'static ArchiveShape {
    resolve_shape_for_text_on(
        model_id,
        provider_family,
        explicit,
        probe,
        ShapeHost::default(),
    )
}

/// [`resolve_shape_for_text`] with the deployment facts the provider reported.
pub fn resolve_shape_for_text_on(
    model_id: &str,
    provider_family: &str,
    explicit: Option<&str>,
    probe: &str,
    host: ShapeHost,
) -> &'static ArchiveShape {
    let base = resolve_shape_on(model_id, provider_family, explicit, host);
    if is_explicit(explicit) {
        return base;
    }
    let silver_safe = font_coverage(probe, "silver").is_safe();
    let prefer_silver = if font_coverage(probe, base.font).is_safe() {
        silver_safe && cjk_heavy(probe)
    } else {
        silver_safe
    };
    if prefer_silver {
        resolve_silver(model_id, provider_family, host)
    } else {
        base
    }
}

fn cjk_heavy(probe: &str) -> bool {
    let (mut wide, mut graphics) = (0_usize, 0_usize);
    for ch in normalize_text(probe, "silver").chars() {
        if matches!(ch, ' ' | '█' | '\u{e}' | '\u{f}') {
            continue;
        }
        graphics += 1;
        wide += usize::from(cell_width(ch) == 2);
    }
    wide >= MIN_WIDE && wide.saturating_mul(4) >= graphics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_geometry_switches_to_silver_only_when_it_helps() {
        let wide = "你好世界".repeat(3);
        let mostly_latin = format!("{}你好", "word ".repeat(40));
        let private_use = "\u{e000}".repeat(10);
        for (text, explicit, expected) in [
            (wide.as_str(), None, "silver16-bw"),
            (wide.as_str(), Some("auto"), "silver16-bw"),
            (wide.as_str(), Some("8on22-bw"), "8on22-bw"),
            ("plain ascii text", None, "8on22-bw"),
            (mostly_latin.as_str(), None, "8on22-bw"),
            ("你好世界", None, "8on22-bw"),
            (private_use.as_str(), None, "8on22-bw"),
        ] {
            let shape = resolve_shape_for_text("gpt-5.5", "openai-responses", explicit, text);
            assert_eq!(shape.id, expected, "{text:?} {explicit:?}");
        }
    }

    #[test]
    fn silver_keeps_the_model_edge_and_billing_lineage() {
        let text = "你好世界".repeat(3);
        let shape = resolve_shape_for_text("claude-opus-4-7", "anthropic-messages", None, &text);
        let model = resolve_shape("claude-opus-4-7", "anthropic-messages", None);
        assert_eq!(shape.id, "silver16-bw");
        assert_eq!(
            (shape.edge, shape.frame_token_estimate, shape.verified),
            (model.edge, model.frame_token_estimate, model.verified)
        );
    }
}

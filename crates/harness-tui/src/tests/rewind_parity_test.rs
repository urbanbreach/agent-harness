use super::*;
use crate::UnwrapOrAbort;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

#[test]
fn rewind_panels_match_grok_reference_cells() {
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/grok-rewind-reference.json"
    ))
    .unwrap_or_abort();
    for case in reference["cases"].as_array().unwrap_or_abort() {
        let selected =
            usize::try_from(case["selected"].as_u64().unwrap_or_abort()).unwrap_or_abort();
        let phase = match case["phase"].as_str().unwrap_or_abort() {
            "loading" => RewindPhase::Loading,
            "picker" => RewindPhase::Picker {
                selected,
                points: (0..20)
                    .rev()
                    .map(|index| RewindPointInfo {
                        prompt_index: index,
                        created_at: String::new(),
                        num_file_snapshots: 0,
                        has_file_changes: false,
                        prompt_preview: Some(format!(
                            "Turn {}: inspect 日本語 and a long prompt with spaces",
                            index + 1
                        )),
                    })
                    .collect(),
            },
            "cancel" => RewindPhase::CancelOffer {
                active_idx: selected,
            },
            "confirm" => RewindPhase::Confirm {
                target_prompt_index: 3,
                active_idx: selected,
                prompt_preview: Some("inspect 日本語 and a long prompt with spaces".into()),
            },
            "executing" => RewindPhase::Executing {
                target_prompt_index: 3,
            },
            _ => RewindPhase::Error {
                message: "The session could not be rewound. Try again.".into(),
            },
        };
        let height = rewind_overlay_height(
            &phase,
            u16::try_from(case["screen_height"].as_u64().unwrap_or_abort()).unwrap_or_abort(),
        );
        assert_eq!(u64::from(height), case["height"].as_u64().unwrap_or_abort());
        let area = Rect::new(
            2,
            1,
            u16::try_from(case["width"].as_u64().unwrap_or_abort()).unwrap_or_abort(),
            height,
        );
        let theme = match case["theme"].as_str() {
            Some("light") => Theme::harness_light(),
            Some("terminal") => Theme::terminal_native(),
            _ => Theme::harness_dark(),
        };
        let mut buffer = Buffer::empty(Rect::new(0, 0, area.right() + 2, area.bottom() + 1));
        buffer.set_style(
            buffer.area,
            Style::new().fg(theme.text.primary).bg(theme.surface.canvas),
        );
        render_rewind_overlay(
            &mut buffer,
            area,
            &phase,
            case["focused"].as_bool().unwrap_or_abort(),
            &theme,
        );
        let cells: Vec<_> = buffer
            .content
            .iter()
            .map(|cell| {
                (
                    cell.symbol(),
                    format!("{:?}", cell.fg),
                    format!("{:?}", cell.bg),
                    cell.modifier.bits(),
                )
            })
            .collect();
        let digest = Sha256::digest(serde_json::to_vec(&cells).unwrap_or_abort())
            .iter()
            .fold(String::new(), |mut hex, byte| {
                write!(&mut hex, "{byte:02x}").unwrap_or_abort();
                hex
            });
        assert_eq!(
            digest,
            case["digest"].as_str().unwrap_or_abort(),
            "Grok reference mismatch: {case}\n{buffer:?}"
        );
    }
}

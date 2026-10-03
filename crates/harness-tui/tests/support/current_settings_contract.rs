//! Separate current-registry oracle. Never records or rewrites the original.
use super::*;
use harness_core::config::{setting_editor_kind, settings_registry, SettingSurface};
use ratatui::style::Modifier;
use sha2::{Digest, Sha256};

const FIXTURE: &str = "tests/fixtures/tui-current-settings-registry-20261003.cells.jsonl";
const IDENTITY: &str = "tests/fixtures/tui-current-settings-registry-20261003.identity.json";
const SIZES: [(u16, u16); 4] = [(40, 24), (80, 24), (120, 40), (160, 50)];

pub(super) fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

fn ids() -> Vec<String> {
    SIZES
        .into_iter()
        .flat_map(|(width, height)| {
            ["dialog", "selected"]
                .map(|state| format!("{state}-settings-{width}x{height}-reduced-0ms"))
        })
        .collect()
}

pub(super) fn binding() -> Result<Value> {
    Ok(json!({
        "contract": "Harness current settings registry 2026-10-03; not Grok parity",
        "original_base": BASE,
        "original_fixture_sha256": sha256(&fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(GOLDEN)
        )?),
        "registry": settings_registry(),
        "editor_kinds": settings_registry().iter().map(|entry| {
            (entry.setting_id.as_str(), format!("{:?}", setting_editor_kind(entry.setting_id.as_str())))
        }).collect::<Vec<_>>(),
        "frame_ids": ids(),
    }))
}

pub(super) struct Contract {
    pub(super) frames: Vec<Value>,
}

impl Contract {
    pub(super) fn load() -> Result<Self> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let bytes = fs::read(root.join(FIXTURE))?;
        let identity: Value = serde_json::from_slice(&fs::read(root.join(IDENTITY))?)?;
        validate_identity(&identity, &binding()?, &bytes)?;
        let frames: Vec<Value> = std::str::from_utf8(&bytes)?
            .lines()
            .map(serde_json::from_str)
            .collect::<std::result::Result<_, _>>()?;
        let actual_ids: Vec<_> = frames.iter().map(|frame| frame["id"].clone()).collect();
        if json!(actual_ids) != json!(ids()) {
            return Err("current settings fixture coverage differs".into());
        }
        Ok(Self { frames })
    }

    pub(super) fn expected<'a>(&'a self, original: &'a Value) -> &'a Value {
        self.frames
            .iter()
            .find(|frame| frame["id"] == original["id"])
            .unwrap_or(original)
    }

    pub(super) fn controls(&self) -> Result {
        // Exercise the same complete-frame comparison, including non-scrollbar cells.
        let expected = &self.frames[0];
        for (pointer, value) in [
            ("/id", json!("wrong-frame")),
            ("/cells/0/0", json!(99999)),
            ("/cells/0/1/0", json!("!")),
            ("/cells/0/1/1", json!("Reset")),
            ("/cells/0/1/2", json!("Reset")),
            ("/cells/0/1/3", json!(255)),
            ("/cells/0/1/4", json!("Skip")),
            ("/cells/0/1/5", json!("Red")),
            ("/cursor", json!([0, 0])),
            ("/inputs", json!([])),
            ("/intents", json!(["SubmitPrompt"])),
        ] {
            let mut corrupt = expected.clone();
            *corrupt
                .pointer_mut(pointer)
                .ok_or("missing control field")? = value;
            assert!(compare(expected, &corrupt).is_err(), "accepted {pointer}");
        }
        let bytes = fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE))?;
        let current = binding()?;
        let identity = json!({"binding": current, "fixture_sha256": sha256(&bytes)});
        for pointer in [
            "/binding/editor_kinds/0/1",
            "/binding/registry/0/setting_id",
            "/fixture_sha256",
        ] {
            let mut corrupt = identity.clone();
            *corrupt
                .pointer_mut(pointer)
                .ok_or("missing binding control")? = json!("wrong");
            assert!(validate_identity(&corrupt, &current, &bytes).is_err());
        }
        let mut corrupt_bytes = bytes;
        corrupt_bytes.push(b' ');
        assert!(validate_identity(&identity, &current, &corrupt_bytes).is_err());
        Ok(())
    }
}

fn validate_identity(identity: &Value, current: &Value, bytes: &[u8]) -> Result {
    if identity["binding"] != *current {
        return Err("current settings registry binding differs".into());
    }
    if identity["fixture_sha256"] != sha256(bytes) {
        return Err("current settings complete fixture digest differs".into());
    }
    Ok(())
}

pub(super) fn compare(expected: &Value, actual: &Value) -> Result {
    for field in ["id", "cursor", "cells", "inputs", "intents"] {
        if expected[field] != actual[field] {
            return Err(format!("reference drift in {}: {field}", actual["id"]).into());
        }
    }
    Ok(())
}

// Independent geometry/semantics check before any captured frame can be accepted.
// Do not invoke the renderer's modal model, scrollbar helper, or label formatter.
pub(super) fn check_render(
    name: &str,
    area: Rect,
    journey: &Journey,
    buffer: &Buffer,
    cursor: Option<ratatui::layout::Position>,
) -> Result {
    let selected = match name {
        "dialog-settings" => 0,
        "selected-settings" => 1,
        _ => return Ok(()),
    };
    let registry: Vec<_> = settings_registry()
        .iter()
        .filter(|entry| entry.surface == SettingSurface::Runtime)
        .collect();
    let rows = journey.app.settings_editor_rows();
    assert_eq!(rows.len(), registry.len());
    for (index, (row, definition)) in rows.iter().zip(&registry).enumerate() {
        assert_eq!(row.setting_id, definition.setting_id.as_str());
        assert!(!row.editable && row.effective_value.is_none());
        assert_eq!(row.selected, index == selected);
    }
    assert_eq!(journey.app.settings_editor_selected_index(), selected);
    assert!(cursor.is_none(), "unfocused settings list must hide cursor");
    assert_eq!(
        journey.inputs,
        json!([{"key": if selected == 0 { "Enter" } else { "Down" }, "modifiers": 0}])
            .as_array()
            .ok_or("invalid expected input")?
            .clone()
    );
    assert!(journey
        .intents
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty());
    assert_eq!(buffer.area, area);
    assert_eq!(
        buffer.content.len(),
        usize::from(area.width) * usize::from(area.height)
    );
    let width = area.width.min(88);
    let height = area.height.min(28);
    let x = (area.width - width) / 2;
    let y = (area.height - height) / 2;
    let visible = height - 6;
    let thumb = usize::from(visible).pow(2).div_ceil(registry.len());
    let theme = journey.app.theme();
    for row in 0..visible {
        let cell = &buffer[(x + width - 4, y + 4 + row)];
        let in_thumb = usize::from(row) < thumb;
        assert_eq!(cell.symbol(), if in_thumb { "█" } else { " " });
        assert_eq!(
            cell.fg,
            if in_thumb {
                theme.text.tertiary
            } else {
                theme.scrollbar.track
            }
        );
        assert_eq!(cell.bg, theme.surface.canvas);
        assert_eq!(cell.modifier, Modifier::empty());
        check_row(
            buffer,
            Rect::new(x + 3, y + 4 + row, width - 7, 1),
            registry[usize::from(row)].setting_id.as_str(),
            usize::from(row) == selected,
            theme,
        )?;
    }
    Ok(())
}

fn check_row(
    buffer: &Buffer,
    area: Rect,
    id: &str,
    selected: bool,
    theme: &harness_tui::theme::Theme,
) -> Result {
    let id = id.replace(['.', '_'], " ");
    let mut chars = id.chars();
    let label = chars
        .next()
        .ok_or("empty setting id")?
        .to_uppercase()
        .chain(chars)
        .collect::<String>();
    let width = usize::from(area.width);
    let meta = if width < 32 { "" } else { "read only" };
    assert!(label.len() <= width - meta.len() - 2);
    let pad = width - label.len() - meta.len() - 1;
    let text = format!(" {label}{}{meta}", " ".repeat(pad));
    for (column, symbol) in text.chars().enumerate() {
        let cell = &buffer[(area.x + u16::try_from(column)?, area.y)];
        assert_eq!(cell.symbol(), symbol.to_string());
        let is_meta = column >= width - meta.len();
        assert_eq!(
            cell.fg,
            if selected && !is_meta {
                theme.text.primary
            } else {
                theme.text.tertiary
            }
        );
        assert_eq!(
            cell.bg,
            if selected {
                theme.surface.selected_card
            } else {
                theme.surface.canvas
            }
        );
        assert_eq!(
            cell.modifier,
            if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }
        );
    }
    Ok(())
}

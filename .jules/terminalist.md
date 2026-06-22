## 2024-06-22 - Consistent Overlay Input Placeholders
**Learning:** The `model_switcher` overlay has a separate custom implementation of the input rendering (`render_model_select_input`) that diverges from the standard `render_command_palette_input` in `ui_overlays.rs`.
**Action:** When updating generic placeholder strings in `ui_overlays.rs`, check for specific custom overlay implementations (like `model_switcher.rs`) that might have hardcoded their own placeholder strings.

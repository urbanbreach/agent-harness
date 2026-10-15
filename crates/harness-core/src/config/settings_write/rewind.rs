use super::tui_flags::{flag_settings, write_flag};
use super::*;

pub fn rewind_confirmation_settings(
    explicit_runtime: Option<&Path>,
    context: &ConfigDiscoveryContext,
) -> Result<(bool, PathBuf), SettingWriteError> {
    flag_settings(
        explicit_runtime,
        context,
        |config| config.confirm_before_rewind,
        true,
    )
}
pub fn write_rewind_confirmation(path: &Path, enabled: bool) -> Result<(), SettingWriteError> {
    write_flag(path, "confirm_before_rewind", enabled)
}

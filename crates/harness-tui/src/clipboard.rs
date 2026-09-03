use crate::UnwrapOrAbort;
use std::ffi::OsStr;
use std::io::{self, IsTerminal, Write};
use std::process::{Command, Stdio};

const COPY_ON_SELECT_ENV: &str = "HARNESS_EXPERIMENTAL_DISABLE_COPY_ON_SELECT";

#[cfg(test)]
type CopyHook = Box<dyn Fn(&str) -> io::Result<()>>;

#[cfg(test)]
type CopyOnSelectDisabledOverride = Option<bool>;

#[cfg(test)]
thread_local! {
    static COPY_OVERRIDE: std::cell::RefCell<Option<CopyHook>> = const { std::cell::RefCell::new(None) };

    #[cfg(test)]
    static COPY_ON_SELECT_DISABLED_OVERRIDE: std::cell::Cell<CopyOnSelectDisabledOverride> = const { std::cell::Cell::new(None) };
}

fn truthy(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true")
}

pub(crate) fn copy_on_select_disabled() -> bool {
    #[cfg(test)]
    if let Some(value) = COPY_ON_SELECT_DISABLED_OVERRIDE.with(std::cell::Cell::get) {
        return value;
    }

    copy_on_select_disabled_from_env(
        std::env::var_os(COPY_ON_SELECT_ENV).as_deref(),
        cfg!(windows),
    )
}

fn copy_on_select_disabled_from_env(value: Option<&OsStr>, windows_default: bool) -> bool {
    let Some(value) = value.and_then(OsStr::to_str) else {
        return windows_default;
    };
    truthy(value)
}

fn write_osc52(text: &str) -> io::Result<bool> {
    if !std::io::stdout().is_terminal() {
        return Ok(false);
    }

    let base64 = encode_base64(text.as_bytes());
    let sequence = format!("\x1b]52;c;{base64}\x07");
    let sequence = if std::env::var_os("TMUX").is_some() || std::env::var_os("STY").is_some() {
        format!("\x1bPtmux;\x1b{sequence}\x1b\\")
    } else {
        sequence
    };

    let mut stdout = std::io::stdout().lock();
    stdout.write_all(sequence.as_bytes())?;
    stdout.flush()?;
    Ok(true)
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        let bits = u32::from(first) << 16 | u32::from(second) << 8 | u32::from(third);

        encoded.push(TABLE[((bits >> 18) & 0x3F) as usize] as char);
        encoded.push(TABLE[((bits >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(TABLE[((bits >> 6) & 0x3F) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(TABLE[(bits & 0x3F) as usize] as char);
        } else {
            encoded.push('=');
        }
    }

    encoded
}

fn run_with_stdin(program: &str, args: &[&str], text: &str) -> io::Result<bool> {
    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }

    Ok(child.wait()?.success())
}

fn native_copy(text: &str) -> io::Result<bool> {
    if cfg!(target_os = "macos") {
        let escaped = text
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\r', "\" & return & \"")
            .replace('\n', "\" & linefeed & \"");
        let status = Command::new("osascript")
            .arg("-e")
            .arg(format!("set the clipboard to \"{escaped}\""))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        return match status {
            Ok(status) => Ok(status.success()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err),
        };
    }

    if cfg!(target_os = "linux") {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() && run_with_stdin("wl-copy", &[], text)? {
            return Ok(true);
        }
        if run_with_stdin("xclip", &["-selection", "clipboard"], text)? {
            return Ok(true);
        }
        if run_with_stdin("xsel", &["--clipboard", "--input"], text)? {
            return Ok(true);
        }
        return Ok(false);
    }

    if cfg!(target_os = "windows") {
        return run_with_stdin(
            "powershell.exe",
            &[
                "-NonInteractive",
                "-NoProfile",
                "-Command",
                "[Console]::InputEncoding = [System.Text.Encoding]::UTF8; Set-Clipboard -Value ([Console]::In.ReadToEnd())",
            ],
            text,
        );
    }

    Ok(false)
}

fn copy_impl(
    text: &str,
    write_osc52: impl FnOnce(&str) -> io::Result<bool>,
    native_copy: impl FnOnce(&str) -> io::Result<bool>,
) -> io::Result<()> {
    let osc52 = write_osc52(text);
    let native = native_copy(text);

    if matches!(osc52, Ok(true)) || matches!(native, Ok(true)) {
        return Ok(());
    }

    match (osc52, native) {
        (Err(err), _) => Err(err),
        (_, Err(err)) => Err(err),
        (Ok(false), Ok(false)) => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no clipboard integration available",
        )),
        _ => Ok(()),
    }
}

pub(crate) fn copy(text: &str) -> io::Result<()> {
    #[cfg(test)]
    if let Some(result) = COPY_OVERRIDE.with(|cell| cell.borrow().as_ref().map(|copy| copy(text))) {
        return result;
    }

    copy_impl(text, write_osc52, native_copy)
}

/// Build an OSC 8 hyperlink sequence (open + label + close).
///
/// Terminals that ignore OSC 8 still render `label` as plain text. Empty `uri`
/// or `label` yields plain label text without escape sequences. Control
/// characters in either field are stripped so untrusted provider or model text
/// cannot terminate the sequence and inject terminal controls.
pub(crate) fn format_osc8_hyperlink(uri: &str, label: &str) -> String {
    let sanitized_label: String = label.chars().filter(|c| !c.is_control()).collect();
    if uri.is_empty() || sanitized_label.is_empty() {
        return sanitized_label;
    }
    let sanitized_uri: String = uri.chars().filter(|c| !c.is_control()).collect();
    if sanitized_uri.is_empty() {
        return sanitized_label;
    }
    format!("\x1b]8;;{sanitized_uri}\x1b\\{sanitized_label}\x1b]8;;\x1b\\")
}

#[cfg(test)]
pub(crate) fn set_copy_override(copy: Option<CopyHook>) {
    COPY_OVERRIDE.with(|cell| {
        *cell.borrow_mut() = copy;
    });
}

#[cfg(test)]
pub(crate) fn set_copy_on_select_disabled_override(value: CopyOnSelectDisabledOverride) {
    COPY_ON_SELECT_DISABLED_OVERRIDE.with(|cell| cell.set(value));
}

#[cfg(test)]
mod tests {
    use super::{copy_impl, copy_on_select_disabled_from_env};
    use crate::UnwrapOrAbort;
    use std::ffi::OsStr;
    use std::io;
    use std::sync::{Arc, Mutex};

    #[test]
    fn copy_on_select_env_defaults_to_windows_only() {
        // arrange
        // act
        // assert
        assert!(!copy_on_select_disabled_from_env(None, false));
        assert!(copy_on_select_disabled_from_env(None, true));
    }

    #[test]
    fn copy_on_select_env_treats_truthy_values_as_disabled() {
        // arrange
        // act
        // assert
        assert!(copy_on_select_disabled_from_env(
            Some(OsStr::new("1")),
            false
        ));
        assert!(copy_on_select_disabled_from_env(
            Some(OsStr::new("true")),
            false
        ));
        assert!(copy_on_select_disabled_from_env(
            Some(OsStr::new("TRUE")),
            false
        ));
    }

    #[test]
    fn copy_on_select_env_treats_other_values_as_enabled() {
        // arrange
        // act
        // assert
        assert!(!copy_on_select_disabled_from_env(
            Some(OsStr::new("0")),
            true
        ));
        assert!(!copy_on_select_disabled_from_env(
            Some(OsStr::new("false")),
            true
        ));
        assert!(!copy_on_select_disabled_from_env(
            Some(OsStr::new("nope")),
            true
        ));
    }

    #[test]
    fn copy_falls_back_to_native_after_osc52_error() {
        // arrange
        // act
        // assert
        let calls = Arc::new(Mutex::new(Vec::new()));
        let osc52_calls = Arc::clone(&calls);
        let native_calls = Arc::clone(&calls);

        let result = copy_impl(
            "fallback text",
            move |text| {
                osc52_calls
                    .lock()
                    .unwrap_or_abort()
                    .push(format!("osc52:{text}"));
                Err(io::Error::other("osc52 failed"))
            },
            move |text| {
                native_calls
                    .lock()
                    .unwrap_or_abort()
                    .push(format!("native:{text}"));
                Ok(true)
            },
        );

        assert!(result.is_ok());
        assert_eq!(
            calls.lock().unwrap_or_abort().as_slice(),
            ["osc52:fallback text", "native:fallback text"]
        );
    }

    #[test]
    fn copy_errors_when_no_clipboard_path_is_available() {
        // arrange
        // act
        // assert
        let err = copy_impl("unhandled", |_| Ok(false), |_| Ok(false)).expect_err("copy fails");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert_eq!(err.to_string(), "no clipboard integration available");
    }

    #[test]
    fn osc8_hyperlink_wraps_label_and_falls_back_on_empty() {
        // arrange
        // act
        // assert
        use super::format_osc8_hyperlink;

        let linked = format_osc8_hyperlink("https://example.com/path", "path");
        assert!(linked.contains("https://example.com/path"));
        assert!(linked.contains("path"));
        assert!(linked.starts_with("\x1b]8;;"));
        assert!(linked.ends_with("\x1b]8;;\x1b\\"));

        assert_eq!(format_osc8_hyperlink("", "plain"), "plain");
        assert_eq!(format_osc8_hyperlink("https://x", ""), "");

        let injected = format_osc8_hyperlink(
            "https://example.com/\u{1b}]52;c;exfil",
            "auth\u{1b}]8;;https://evil.example\u{5c}\u{1b}\u{5c}injected\u{7}",
        );
        assert!(!injected.contains('\u{7}'));
        assert_eq!(injected.matches('\u{1b}').count(), 4);
    }
}

use std::io::{self, IsTerminal, Write};

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use harness_core::auth::copilot::CopilotDeployment;
use harness_core::auth::AuthProviderId;

use crate::CliIo;

use super::AuthLoginMethod;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopilotDeploymentChoice {
    Public,
    Enterprise,
}

#[derive(Debug, Clone, Copy)]
struct AuthPromptOption<T> {
    label: &'static str,
    value: T,
    hint: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthPromptKey {
    Up,
    Down,
    Submit,
    Cancel,
    Backspace,
    Char(char),
    Ignored,
}

pub(super) struct RawModeGuard {
    enabled: bool,
}

impl RawModeGuard {
    pub(super) fn new(enabled: bool) -> Self {
        let enabled = enabled && crossterm::terminal::enable_raw_mode().is_ok();
        Self { enabled }
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if self.enabled {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
}

pub(super) enum AuthInteractiveError {
    Cancelled,
    Io(io::Error),
}

pub(super) fn prompt_auth_provider(io: &mut CliIo<'_>) -> io::Result<Option<AuthProviderId>> {
    prompt_pick(
        io,
        "Select provider",
        &[
            AuthPromptOption {
                label: "OpenAI",
                value: AuthProviderId::Codex,
                hint: Some("ChatGPT Plus/Pro or API key"),
            },
            AuthPromptOption {
                label: "GitHub Copilot",
                value: AuthProviderId::GithubCopilot,
                hint: None,
            },
        ],
        true,
    )
}

pub(super) fn prompt_login_method(
    auth_provider: AuthProviderId,
    io: &mut CliIo<'_>,
) -> io::Result<Option<AuthLoginMethod>> {
    if auth_provider == AuthProviderId::GithubCopilot {
        return Ok(Some(AuthLoginMethod::Device));
    }

    prompt_pick(
        io,
        "Login method",
        &[
            AuthPromptOption {
                label: "ChatGPT Pro/Plus (browser)",
                value: AuthLoginMethod::Browser,
                hint: None,
            },
            AuthPromptOption {
                label: "ChatGPT Pro/Plus (headless)",
                value: AuthLoginMethod::Device,
                hint: None,
            },
            AuthPromptOption {
                label: "Manually enter API Key",
                value: AuthLoginMethod::ApiKey,
                hint: None,
            },
        ],
        false,
    )
}

pub(super) fn interactive_enterprise_url(
    auth_provider: AuthProviderId,
    explicit_enterprise_url: Option<String>,
    io: &mut CliIo<'_>,
) -> Result<Option<String>, AuthInteractiveError> {
    if auth_provider != AuthProviderId::GithubCopilot || explicit_enterprise_url.is_some() {
        return Ok(explicit_enterprise_url);
    }

    let deployment = prompt_pick(
        io,
        "Select GitHub deployment type",
        &[
            AuthPromptOption {
                label: "GitHub.com",
                value: CopilotDeploymentChoice::Public,
                hint: Some("Public"),
            },
            AuthPromptOption {
                label: "GitHub Enterprise",
                value: CopilotDeploymentChoice::Enterprise,
                hint: Some("Data residency or self-hosted"),
            },
        ],
        false,
    )
    .map_err(AuthInteractiveError::Io)?;

    match deployment {
        Some(CopilotDeploymentChoice::Public) => Ok(None),
        Some(CopilotDeploymentChoice::Enterprise) => prompt_copilot_enterprise_url(io),
        None => Err(AuthInteractiveError::Cancelled),
    }
}

fn prompt_copilot_enterprise_url(
    io: &mut CliIo<'_>,
) -> Result<Option<String>, AuthInteractiveError> {
    loop {
        let value = prompt_input(
            io,
            "Enter your GitHub Enterprise URL or domain",
            Some("company.ghe.com or https://company.ghe.com"),
            false,
        )
        .map_err(AuthInteractiveError::Io)?;
        let Some(value) = value else {
            return Err(AuthInteractiveError::Cancelled);
        };
        match CopilotDeployment::enterprise(&value) {
            Ok(CopilotDeployment::Enterprise { domain }) => return Ok(Some(domain)),
            Ok(CopilotDeployment::Public) => return Ok(None),
            Err(err) => {
                let _ = clack_log_error(io.stdout, &err.to_string());
            }
        }
    }
}

fn prompt_pick<T: Copy>(
    io: &mut CliIo<'_>,
    message: &str,
    options: &[AuthPromptOption<T>],
    searchable: bool,
) -> io::Result<Option<T>> {
    let terminal = auth_prompt_terminal_events_enabled();
    let _raw_mode = RawModeGuard::new(terminal);
    let mut selected = 0_usize;
    let mut filter = String::new();
    let mut rendered_lines = 0_usize;

    loop {
        let visible = visible_auth_options(options, &filter, searchable);
        if selected >= visible.len() {
            selected = 0;
        }
        rendered_lines = render_auth_picker(
            io.stdout,
            rendered_lines,
            AuthPickerRender {
                message,
                filter: &filter,
                options,
                visible: &visible,
                selected,
                searchable,
            },
        )?;

        match read_auth_prompt_key(io.stdin, terminal)? {
            AuthPromptKey::Up => {
                if !visible.is_empty() {
                    selected = if selected == 0 {
                        visible.len() - 1
                    } else {
                        selected - 1
                    };
                }
            }
            AuthPromptKey::Down => {
                if !visible.is_empty() {
                    selected = (selected + 1) % visible.len();
                }
            }
            AuthPromptKey::Submit => {
                clear_auth_prompt(io.stdout, rendered_lines)?;
                if let Some(option_index) = visible.get(selected) {
                    render_auth_picker_result(io.stdout, message, &options[*option_index])?;
                    return Ok(Some(options[*option_index].value));
                }
            }
            AuthPromptKey::Cancel => {
                clear_auth_prompt(io.stdout, rendered_lines)?;
                return Ok(None);
            }
            AuthPromptKey::Backspace => {
                if searchable {
                    filter.pop();
                    selected = 0;
                }
            }
            AuthPromptKey::Char(ch) => {
                if searchable && !ch.is_control() {
                    filter.push(ch);
                    selected = 0;
                }
            }
            AuthPromptKey::Ignored => {}
        }
    }
}

pub(super) fn prompt_input(
    io: &mut CliIo<'_>,
    message: &str,
    placeholder: Option<&str>,
    secret: bool,
) -> io::Result<Option<String>> {
    let terminal = auth_prompt_terminal_events_enabled();
    let _raw_mode = RawModeGuard::new(terminal);
    let mut value = String::new();
    let mut rendered_lines = 0_usize;

    loop {
        rendered_lines = render_auth_input(
            io.stdout,
            rendered_lines,
            message,
            placeholder,
            &value,
            secret,
        )?;
        match read_auth_prompt_key(io.stdin, terminal)? {
            AuthPromptKey::Submit => {
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() {
                    let _ = clack_log_error(io.stdout, "Required");
                    continue;
                }
                clear_auth_prompt(io.stdout, rendered_lines)?;
                render_auth_input_result(io.stdout, message, &value, secret)?;
                return Ok(Some(trimmed));
            }
            AuthPromptKey::Cancel => {
                clear_auth_prompt(io.stdout, rendered_lines)?;
                return Ok(None);
            }
            AuthPromptKey::Backspace => {
                value.pop();
            }
            AuthPromptKey::Char(ch) => {
                if !ch.is_control() {
                    value.push(ch);
                }
            }
            AuthPromptKey::Up | AuthPromptKey::Down | AuthPromptKey::Ignored => {}
        }
    }
}

fn visible_auth_options<T>(
    options: &[AuthPromptOption<T>],
    filter: &str,
    searchable: bool,
) -> Vec<usize> {
    let needle = filter.trim().to_ascii_lowercase();
    options
        .iter()
        .enumerate()
        .filter_map(|(index, option)| {
            if !searchable || needle.is_empty() {
                return Some(index);
            }
            let haystack = format!("{} {:?}", option.label, option.hint).to_ascii_lowercase();
            haystack.contains(&needle).then_some(index)
        })
        .collect()
}

struct AuthPickerRender<'a, T> {
    message: &'a str,
    filter: &'a str,
    options: &'a [AuthPromptOption<T>],
    visible: &'a [usize],
    selected: usize,
    searchable: bool,
}

fn render_auth_picker<T>(
    stdout: &mut dyn Write,
    previous_lines: usize,
    render: AuthPickerRender<'_, T>,
) -> io::Result<usize> {
    clear_auth_prompt(stdout, previous_lines)?;
    let mut lines = 0;
    write!(stdout, "│\r\n")?;
    lines += 1;
    if render.searchable && !render.filter.is_empty() {
        write!(
            stdout,
            "◆  {} {AUTH_DIM}{}{AUTH_RESET}\r\n",
            render.message, render.filter
        )?;
    } else {
        write!(stdout, "◆  {}\r\n", render.message)?;
    }
    lines += 1;
    if render.visible.is_empty() {
        write!(stdout, "│  {AUTH_DIM}No matches{AUTH_RESET}\r\n")?;
        lines += 1;
    } else {
        for (position, option_index) in render.visible.iter().enumerate() {
            let option = &render.options[*option_index];
            let marker = if position == render.selected {
                "●"
            } else {
                "○"
            };
            let hint = option
                .hint
                .map(|hint| format!(" {AUTH_DIM}{hint}{AUTH_RESET}"))
                .unwrap_or_default();
            write!(stdout, "│  {marker} {}{hint}\r\n", option.label)?;
            lines += 1;
        }
    }
    write!(
        stdout,
        "└  {AUTH_DIM}↑/↓ select, enter confirm, esc cancel{AUTH_RESET}\r\n"
    )?;
    lines += 1;
    stdout.flush()?;
    Ok(lines)
}

fn render_auth_picker_result<T>(
    stdout: &mut dyn Write,
    message: &str,
    option: &AuthPromptOption<T>,
) -> io::Result<()> {
    let hint = option
        .hint
        .map(|hint| format!(" {AUTH_DIM}{hint}{AUTH_RESET}"))
        .unwrap_or_default();
    write!(stdout, "│\r\n")?;
    write!(stdout, "◇  {message}\r\n")?;
    write!(stdout, "│  {}{hint}\r\n", option.label)?;
    stdout.flush()
}

fn render_auth_input(
    stdout: &mut dyn Write,
    previous_lines: usize,
    message: &str,
    placeholder: Option<&str>,
    value: &str,
    secret: bool,
) -> io::Result<usize> {
    clear_auth_prompt(stdout, previous_lines)?;
    let mut lines = 0;
    write!(stdout, "│\r\n")?;
    lines += 1;
    write!(stdout, "◆  {message}\r\n")?;
    lines += 1;
    let rendered_value = if value.is_empty() {
        placeholder
            .map(|value| format!("{AUTH_DIM}{value}{AUTH_RESET}"))
            .unwrap_or_default()
    } else if secret {
        "•".repeat(value.chars().count())
    } else {
        value.to_string()
    };
    write!(stdout, "│  {rendered_value}\r\n")?;
    lines += 1;
    write!(
        stdout,
        "└  {AUTH_DIM}enter confirm, esc cancel{AUTH_RESET}\r\n"
    )?;
    lines += 1;
    stdout.flush()?;
    Ok(lines)
}

fn render_auth_input_result(
    stdout: &mut dyn Write,
    message: &str,
    value: &str,
    secret: bool,
) -> io::Result<()> {
    let rendered_value = if secret {
        "•".repeat(value.chars().count())
    } else {
        value.to_string()
    };
    write!(stdout, "│\r\n")?;
    write!(stdout, "◇  {message}\r\n")?;
    write!(stdout, "│  {rendered_value}\r\n")?;
    stdout.flush()
}

fn clear_auth_prompt(stdout: &mut dyn Write, lines: usize) -> io::Result<()> {
    if lines > 0 {
        write!(stdout, "\x1b[{lines}A\x1b[J")?;
    }
    Ok(())
}

fn read_auth_prompt_key(
    stdin: &mut dyn std::io::BufRead,
    terminal: bool,
) -> io::Result<AuthPromptKey> {
    if terminal {
        return read_terminal_auth_prompt_key();
    }

    let Some(byte) = read_auth_prompt_byte(stdin)? else {
        return Ok(AuthPromptKey::Cancel);
    };
    match byte {
        b'\r' | b'\n' => Ok(AuthPromptKey::Submit),
        0x03 => Ok(AuthPromptKey::Cancel),
        0x7f | 0x08 => Ok(AuthPromptKey::Backspace),
        b'\t' => Ok(AuthPromptKey::Down),
        0x1b => read_auth_escape_key(stdin),
        byte if byte.is_ascii() => Ok(AuthPromptKey::Char(byte as char)),
        _ => Ok(AuthPromptKey::Ignored),
    }
}

fn read_terminal_auth_prompt_key() -> io::Result<AuthPromptKey> {
    loop {
        match crossterm::event::read().map_err(io::Error::other)? {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Up => return Ok(AuthPromptKey::Up),
                KeyCode::Down | KeyCode::Tab => return Ok(AuthPromptKey::Down),
                KeyCode::Enter => return Ok(AuthPromptKey::Submit),
                KeyCode::Esc => return Ok(AuthPromptKey::Cancel),
                KeyCode::Backspace => return Ok(AuthPromptKey::Backspace),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(AuthPromptKey::Cancel)
                }
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(AuthPromptKey::Down)
                }
                KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(AuthPromptKey::Up)
                }
                KeyCode::Char(ch) => return Ok(AuthPromptKey::Char(ch)),
                _ => return Ok(AuthPromptKey::Ignored),
            },
            _ => {}
        }
    }
}

fn read_auth_escape_key(stdin: &mut dyn std::io::BufRead) -> io::Result<AuthPromptKey> {
    let Some(next) = read_auth_prompt_byte(stdin)? else {
        return Ok(AuthPromptKey::Cancel);
    };
    if next != b'[' {
        return Ok(AuthPromptKey::Cancel);
    }
    match read_auth_prompt_byte(stdin)? {
        Some(b'A') => Ok(AuthPromptKey::Up),
        Some(b'B') => Ok(AuthPromptKey::Down),
        _ => Ok(AuthPromptKey::Ignored),
    }
}

fn read_auth_prompt_byte(stdin: &mut dyn std::io::BufRead) -> io::Result<Option<u8>> {
    let mut byte = [0_u8; 1];
    match stdin.read(&mut byte)? {
        0 => Ok(None),
        _ => Ok(Some(byte[0])),
    }
}

pub(super) fn auth_prompt_terminal_events_enabled() -> bool {
    std::io::stdin().is_terminal()
}

pub(super) fn auth_prompt_io_error(err: io::Error, stderr: &mut dyn Write) -> i32 {
    let _ = writeln!(
        stderr,
        "auth login failed: failed to read terminal input: {err}"
    );
    1
}

const AUTH_DIM: &str = "\x1b[90m";
const AUTH_RESET: &str = "\x1b[0m";
const AUTH_GREEN: &str = "\x1b[92m";
const AUTH_RED: &str = "\x1b[91m";

pub(super) fn clack_intro(stdout: &mut dyn Write, message: &str) -> io::Result<()> {
    writeln!(stdout, "◇  {message}")
}

pub(super) fn clack_outro(stdout: &mut dyn Write, message: &str) -> io::Result<()> {
    writeln!(stdout, "└  {message}")
}

pub(super) fn clack_log_info(stdout: &mut dyn Write, message: &str) -> io::Result<()> {
    writeln!(stdout, "│  {message}")
}

pub(super) fn clack_log_success(stdout: &mut dyn Write, message: &str) -> io::Result<()> {
    writeln!(stdout, "│  {AUTH_GREEN}{message}{AUTH_RESET}")
}

pub(super) fn clack_log_error(stdout: &mut dyn Write, message: &str) -> io::Result<()> {
    writeln!(stdout, "│  {AUTH_RED}{message}{AUTH_RESET}")
}

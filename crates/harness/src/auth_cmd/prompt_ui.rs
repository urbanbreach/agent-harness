use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use harness_core::auth::plugin::{
    AuthMethodSpec, AuthPluginRegistry, PromptField, PromptFieldType, PromptOp,
};
use harness_core::auth::ProviderId;
use harness_core::provider_catalog::ProviderCatalog;

use crate::CliIo;

#[derive(Debug, Clone)]
struct AuthPromptOption<T> {
    label: String,
    value: T,
    hint: Option<String>,
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

pub(super) fn prompt_auth_provider(
    catalog: &ProviderCatalog,
    registry: &AuthPluginRegistry,
    io: &mut CliIo<'_>,
) -> io::Result<Option<ProviderId>> {
    let options: Vec<AuthPromptOption<ProviderId>> = catalog
        .sorted_by_priority()
        .into_iter()
        .filter_map(|entry| {
            let provider_id = ProviderId::parse(entry.id.as_str())?;
            let plugin = registry.get(&provider_id).or_else(|| {
                (provider_id.as_str() == "openai")
                    .then(ProviderId::codex)
                    .and_then(|codex| registry.get(&codex))
            });
            Some(AuthPromptOption {
                label: plugin
                    .map(|plugin| plugin.label().to_string())
                    .unwrap_or_else(|| entry.name.clone()),
                value: plugin
                    .map(|plugin| plugin.provider_id().clone())
                    .unwrap_or(provider_id),
                hint: plugin
                    .map(|plugin| plugin.description().to_string())
                    .or_else(|| Some("API key".to_string())),
            })
        })
        .collect();
    prompt_pick(io, "Select provider", &options, true)
}

pub(super) fn prompt_login_method(
    methods: &[AuthMethodSpec],
    io: &mut CliIo<'_>,
) -> io::Result<Option<usize>> {
    let options: Vec<AuthPromptOption<usize>> = methods
        .iter()
        .enumerate()
        .map(|(i, m)| AuthPromptOption {
            label: method_label(m),
            value: i,
            hint: None,
        })
        .collect();
    prompt_pick(io, "Login method", &options, false)
}

fn method_label(method: &AuthMethodSpec) -> String {
    match method {
        AuthMethodSpec::OAuthAuto { label, .. } => label.clone(),
        AuthMethodSpec::OAuthCode { label } => label.clone(),
        AuthMethodSpec::ApiKey { label } => label.clone(),
        AuthMethodSpec::Prompts { label, .. } => label.clone(),
    }
}

pub(super) fn run_prompts(
    prompts: &[PromptField],
    io: &mut CliIo<'_>,
) -> Result<BTreeMap<String, String>, AuthInteractiveError> {
    let mut values = BTreeMap::new();
    for field in prompts {
        if !should_run_prompt(field, &values) {
            continue;
        }
        let value = match field.field_type {
            PromptFieldType::Select => prompt_select_field(field, io)?,
            PromptFieldType::Text => prompt_text_field(field, io)?,
        };
        let Some(value) = value else {
            return Err(AuthInteractiveError::Cancelled);
        };
        values.insert(field.key.clone(), value);
    }
    Ok(values)
}

fn should_run_prompt(field: &PromptField, values: &BTreeMap<String, String>) -> bool {
    let Some(condition) = &field.when else {
        return true;
    };
    let actual = values.get(&condition.key);
    match condition.op {
        PromptOp::Eq => actual.map(|v| v == &condition.value).unwrap_or(false),
        PromptOp::Neq => actual.map(|v| v != &condition.value).unwrap_or(true),
    }
}

fn prompt_select_field(
    field: &PromptField,
    io: &mut CliIo<'_>,
) -> Result<Option<String>, AuthInteractiveError> {
    let options: Vec<AuthPromptOption<String>> = field
        .options
        .iter()
        .map(|opt| AuthPromptOption {
            label: opt.label.clone(),
            value: opt.id.clone(),
            hint: None,
        })
        .collect();
    prompt_pick(io, &field.message, &options, false).map_err(AuthInteractiveError::Io)
}

fn prompt_text_field(
    field: &PromptField,
    io: &mut CliIo<'_>,
) -> Result<Option<String>, AuthInteractiveError> {
    prompt_input(io, &field.message, field.placeholder.as_deref(), false)
        .map_err(AuthInteractiveError::Io)
}

fn prompt_pick<T: Clone>(
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
                    return Ok(Some(options[*option_index].value.clone()));
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
                .as_ref()
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
        .as_ref()
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

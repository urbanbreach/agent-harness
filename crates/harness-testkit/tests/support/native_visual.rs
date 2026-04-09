use image::RgbImage;
use regex::Regex;
use serde_json::{json, Value};
use std::env;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::live_visual::{
    ExternalPngCheckpointSpec, FocusCapture, LiveVisualCheckpoint, LiveVisualRun,
};

const DEFAULT_FONT_FAMILY: &str = "DejaVu Sans Mono";
const DEFAULT_FONT_SIZE: &str = "12";
const CAPTURE_RETRY_ATTEMPTS: usize = 10;
const CAPTURE_RETRY_DELAY: Duration = Duration::from_millis(200);
const WINDOW_LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);
const SCREEN_POLL_INTERVAL: Duration = Duration::from_millis(50);
const NATIVE_VISUAL_CAPTURE_HELPER_ENV: &str = "HARNESS_NATIVE_VISUAL_CAPTURE_HELPER";
const NATIVE_VISUAL_HELPER_DESKTOP_ID: &str = "accela-agent-harness-native-visual-helper.desktop";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeVisualGrid {
    pub(crate) rows: u16,
    pub(crate) cols: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeVisualSummary {
    pub(crate) backend: String,
    pub(crate) display_protocol: String,
    pub(crate) window_id: String,
    pub(crate) font_family: String,
    pub(crate) font_size: String,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) ghostty_version: String,
    pub(crate) tmux_version: String,
    pub(crate) cleanup_verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeVisualAvailability {
    Disabled,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExternalCaptureProvenance {
    helper_path: PathBuf,
    capture_backend: String,
    capture_tool: String,
    captured_window_id: String,
    captured_window_title: Option<String>,
}

#[derive(Debug)]
pub(crate) struct GhosttyNativeHarness {
    ghostty: Child,
    tmux_socket: String,
    session_name: String,
    session_target: String,
    window_title: String,
    window_id: String,
    workspace_root: PathBuf,
    temp_dir: PathBuf,
    grid: NativeVisualGrid,
    font_family: String,
    font_size: String,
    ghostty_version: String,
    tmux_version: String,
    ghostty_stderr_path: PathBuf,
}

impl GhosttyNativeHarness {
    pub(crate) fn spawn_with_args(
        harness_bin: &Path,
        workspace_root: &Path,
        session_dir: &Path,
        grid: NativeVisualGrid,
        extra_args: &[&str],
    ) -> Result<Self, String> {
        ensure_native_visual_prereqs()?;

        let temp_dir = unique_temp_dir("native-visual");
        fs::create_dir_all(&temp_dir).map_err(|err| {
            format!(
                "failed to create native visual temp dir {}: {err}",
                temp_dir.display()
            )
        })?;

        let tmux_socket = format!(
            "harness-native-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|err| format!("system clock before unix epoch: {err}"))?
                .as_nanos()
        );
        let session_name = format!("native-visual-{}", std::process::id());
        let session_target = format!("{session_name}:0.0");
        let window_title = format!("harness-native-visual-{}", std::process::id());
        let script_path = temp_dir.join("run-harness.sh");
        write_launcher_script(&script_path, harness_bin, session_dir, extra_args)?;

        run_command(
            Command::new("tmux")
                .arg("-L")
                .arg(&tmux_socket)
                .arg("new-session")
                .arg("-d")
                .arg("-s")
                .arg(&session_name)
                .arg("-c")
                .arg(workspace_root)
                .arg("-x")
                .arg(grid.cols.to_string())
                .arg("-y")
                .arg(grid.rows.to_string())
                .arg(&script_path),
            "spawn isolated tmux session",
        )?;

        let font_family = env::var("HARNESS_NATIVE_VISUAL_FONT_FAMILY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_FONT_FAMILY.to_string());
        let font_size = env::var("HARNESS_NATIVE_VISUAL_FONT_SIZE")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_FONT_SIZE.to_string());
        let ghostty_version = read_first_line(run_command_output(
            Command::new("ghostty").arg("--version"),
            "read Ghostty version",
        )?);
        let tmux_version = read_first_line(run_command_output(
            Command::new("tmux").arg("-V"),
            "read tmux version",
        )?);

        let ghostty_stderr_path = temp_dir.join("ghostty-stderr.log");
        let ghostty_stderr = File::create(&ghostty_stderr_path).map_err(|err| {
            format!(
                "failed to create Ghostty stderr log {}: {err}",
                ghostty_stderr_path.display()
            )
        })?;

        let mut ghostty = Command::new("ghostty")
            .env("GDK_BACKEND", "x11")
            .env("NO_AT_BRIDGE", "1")
            .env("GTK_USE_PORTAL", "0")
            .arg(format!("--title={window_title}"))
            .arg(format!("--font-family={font_family}"))
            .arg(format!("--font-size={font_size}"))
            .arg(format!("--window-width={}", grid.cols))
            .arg(format!("--window-height={}", grid.rows))
            .arg("--window-decoration=false")
            .arg("--gtk-titlebar=false")
            .arg("--window-padding-x=0")
            .arg("--window-padding-y=0")
            .arg("--background-opacity=1")
            .arg("--cursor-style=block")
            .arg("--cursor-style-blink=false")
            .arg("--copy-on-select=false")
            .arg("--confirm-close-surface=false")
            .arg("--shell-integration-features=no-cursor,no-title,no-sudo,no-ssh-env,no-ssh-terminfo,path")
            .arg(format!("--working-directory={}", workspace_root.display()))
            .arg("-e")
            .arg("tmux")
            .arg("-L")
            .arg(&tmux_socket)
            .arg("attach-session")
            .arg("-t")
            .arg(&session_name)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(ghostty_stderr))
            .spawn()
            .map_err(|err| format!("failed to spawn Ghostty native visual session: {err}"))?;

        let window_id = wait_for_window_id(
            &mut ghostty,
            &window_title,
            WINDOW_LOOKUP_TIMEOUT,
            &ghostty_stderr_path,
        )?;

        Ok(Self {
            ghostty,
            tmux_socket,
            session_name,
            session_target,
            window_title,
            window_id,
            workspace_root: workspace_root.to_path_buf(),
            temp_dir,
            grid,
            font_family,
            font_size,
            ghostty_version,
            tmux_version,
            ghostty_stderr_path,
        })
    }

    pub(crate) fn spawn_mock(
        harness_bin: &Path,
        workspace_root: &Path,
        session_dir: &Path,
        grid: NativeVisualGrid,
    ) -> Result<Self, String> {
        Self::spawn_with_args(harness_bin, workspace_root, session_dir, grid, &["--mock"])
    }

    pub(crate) fn wait_for_text(&self, marker: &str, timeout: Duration) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        let mut last_screen = String::new();
        while Instant::now() < deadline {
            let screen = self.capture_screen_text()?;
            if screen.contains(marker) {
                return Ok(screen);
            }
            last_screen = screen;
            thread::sleep(SCREEN_POLL_INTERVAL);
        }

        Err(format!(
            "timed out waiting for native visual marker `{marker}`; last screen:\n{last_screen}"
        ))
    }

    pub(crate) fn wait_for_all_text(
        &self,
        markers: &[&str],
        timeout: Duration,
    ) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        let mut last_screen = String::new();
        while Instant::now() < deadline {
            let screen = self.capture_screen_text()?;
            if markers.iter().all(|marker| screen.contains(marker)) {
                return Ok(screen);
            }
            last_screen = screen;
            thread::sleep(SCREEN_POLL_INTERVAL);
        }

        Err(format!(
            "timed out waiting for native visual markers {:?}; last screen:\n{last_screen}",
            markers
        ))
    }

    pub(crate) fn send_text(&self, text: &str) -> Result<(), String> {
        run_command(
            Command::new("tmux")
                .arg("-L")
                .arg(&self.tmux_socket)
                .arg("send-keys")
                .arg("-t")
                .arg(&self.session_target)
                .arg("-l")
                .arg(text),
            "send literal text to tmux pane",
        )?;
        Ok(())
    }

    pub(crate) fn send_enter(&self) -> Result<(), String> {
        run_command(
            Command::new("tmux")
                .arg("-L")
                .arg(&self.tmux_socket)
                .arg("send-keys")
                .arg("-t")
                .arg(&self.session_target)
                .arg("Enter"),
            "send Enter to tmux pane",
        )?;
        Ok(())
    }

    pub(crate) fn send_key(&self, key: &str) -> Result<(), String> {
        run_command(
            Command::new("tmux")
                .arg("-L")
                .arg(&self.tmux_socket)
                .arg("send-keys")
                .arg("-t")
                .arg(&self.session_target)
                .arg(key),
            "send tmux key to native visual pane",
        )?;
        Ok(())
    }

    pub(crate) fn capture_checkpoint(
        &self,
        visual_run: &mut LiveVisualRun,
        checkpoint_id: &str,
        markers: &[&str],
        focus: &FocusCapture,
        metadata: Option<Value>,
    ) -> Result<LiveVisualCheckpoint, String> {
        let screen_text = self.capture_screen_text()?;
        let temp_png = self.temp_dir.join(format!("capture-{checkpoint_id}.png"));
        let capture = self.capture_window_png(&temp_png)?;
        let metadata = merge_metadata(
            metadata,
            json!({
                "backend": "ghostty",
                "window_id": self.window_id,
                "window_title": self.window_title,
                "display_protocol": detect_display_protocol(),
                "capture_tool": capture.capture_tool,
                "capture_backend": capture.capture_backend,
                "capture_helper": capture.helper_path.display().to_string(),
                "captured_window_id": capture.captured_window_id,
                "captured_window_title": capture.captured_window_title,
                "workspace_root": self.workspace_root.display().to_string(),
                "terminal": {
                    "font_family": self.font_family,
                    "font_size": self.font_size,
                    "rows": self.grid.rows,
                    "cols": self.grid.cols,
                },
            }),
        );
        visual_run.capture_external_png_checkpoint(ExternalPngCheckpointSpec {
            checkpoint_id,
            source_png_path: &temp_png,
            screen_text: &screen_text,
            terminal_size: (self.grid.rows, self.grid.cols),
            screen_markers: markers,
            focus,
            metadata: Some(&metadata),
        })
    }

    pub(crate) fn cleanup(mut self) -> Result<NativeVisualSummary, String> {
        best_effort_run(
            Command::new("tmux")
                .arg("-L")
                .arg(&self.tmux_socket)
                .arg("detach-client")
                .arg("-s")
                .arg(&self.session_name),
        );

        self.ghostty
            .kill()
            .map_err(|err| format!("failed to terminate Ghostty native visual window: {err}"))?;
        let ghostty_status = self
            .ghostty
            .wait()
            .map_err(|err| format!("failed waiting for Ghostty native visual window: {err}"))?;

        run_command(
            Command::new("tmux")
                .arg("-L")
                .arg(&self.tmux_socket)
                .arg("kill-server"),
            "kill isolated tmux server",
        )?;

        let cleanup_verified = wait_for_window_gone(&self.window_id, Duration::from_secs(5))?;
        if !cleanup_verified {
            return Err(format!(
                "native visual Ghostty window {} still exists after cleanup (status: {ghostty_status})",
                self.window_id
            ));
        }
        let _ = fs::remove_dir_all(&self.temp_dir);

        Ok(NativeVisualSummary {
            backend: "ghostty".to_string(),
            display_protocol: detect_display_protocol(),
            window_id: self.window_id,
            font_family: self.font_family,
            font_size: self.font_size,
            cols: self.grid.cols,
            rows: self.grid.rows,
            ghostty_version: self.ghostty_version,
            tmux_version: self.tmux_version,
            cleanup_verified,
        })
    }

    fn capture_screen_text(&self) -> Result<String, String> {
        run_command_output(
            Command::new("tmux")
                .arg("-L")
                .arg(&self.tmux_socket)
                .arg("capture-pane")
                .arg("-p")
                .arg("-t")
                .arg(&self.session_target),
            "capture tmux pane text",
        )
        .map(|output| output.stdout)
        .and_then(|stdout| {
            String::from_utf8(stdout)
                .map_err(|err| format!("tmux capture-pane output was not valid UTF-8: {err}"))
        })
    }

    fn capture_window_png(&self, destination: &Path) -> Result<ExternalCaptureProvenance, String> {
        let helper_path = native_visual_capture_helper_path()?;
        let metadata_path = self.temp_dir.join(format!(
            "capture-{}.json",
            destination
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("metadata")
        ));
        let mut last_error = None;
        for attempt in 0..CAPTURE_RETRY_ATTEMPTS {
            let attempt_result: Result<ExternalCaptureProvenance, String> = (|| {
                run_command(
                    Command::new(&helper_path)
                        .arg("--output")
                        .arg(destination)
                        .arg("--metadata-out")
                        .arg(&metadata_path)
                        .arg("--window-id")
                        .arg(&self.window_id)
                        .arg("--window-title")
                        .arg(&self.window_title),
                    "capture native screenshot with external helper",
                )?;
                if !destination.exists() {
                    return Err(format!(
                        "native visual capture helper {} succeeded but did not write {}",
                        helper_path.display(),
                        destination.display()
                    ));
                }
                if !metadata_path.exists() {
                    return Err(format!(
                        "native visual capture helper {} succeeded but did not write metadata {}",
                        helper_path.display(),
                        metadata_path.display()
                    ));
                }
                let rendered = image::open(destination)
                    .map_err(|err| {
                        format!(
                            "failed to read native screenshot {}: {err}",
                            destination.display()
                        )
                    })?
                    .to_rgb8();
                if capture_uniform_color(&rendered).is_none() {
                    return read_external_capture_provenance(
                        &metadata_path,
                        &helper_path,
                        &self.window_id,
                    );
                }
                ensure_non_uniform_capture(&rendered, &self.window_id).and_then(|_| {
                    read_external_capture_provenance(&metadata_path, &helper_path, &self.window_id)
                })
            })();

            match attempt_result {
                Ok(provenance) => return Ok(provenance),
                Err(err) => {
                    last_error = Some(err);
                    if attempt + 1 < CAPTURE_RETRY_ATTEMPTS {
                        thread::sleep(CAPTURE_RETRY_DELAY);
                    }
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            format!(
                "native visual capture helper {} exhausted retries for {}",
                helper_path.display(),
                destination.display()
            )
        }))
    }
}

pub(crate) fn default_native_visual_run_metadata(
    test_name: &str,
    grid: &NativeVisualGrid,
) -> Value {
    json!({
        "test_name": test_name,
        "backend": "ghostty",
        "display_protocol": detect_display_protocol(),
        "capture_tool": "external_helper",
        "capture_helper": native_visual_capture_helper_path()
            .ok()
            .map(|path| path.display().to_string()),
        "terminal": {
            "font_family": env::var("HARNESS_NATIVE_VISUAL_FONT_FAMILY")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_FONT_FAMILY.to_string()),
            "font_size": env::var("HARNESS_NATIVE_VISUAL_FONT_SIZE")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_FONT_SIZE.to_string()),
            "rows": grid.rows,
            "cols": grid.cols,
        }
    })
}

pub(crate) fn require_native_visual_availability() -> Result<NativeVisualAvailability, String> {
    if env::var("HARNESS_NATIVE_VISUAL").as_deref() != Ok("1") {
        return Ok(NativeVisualAvailability::Disabled);
    }
    if !cfg!(target_os = "linux") {
        return Err("native visual lane currently supports Linux only".to_string());
    }
    if env::var("DISPLAY")
        .ok()
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err(
            "HARNESS_NATIVE_VISUAL=1 was requested but no X11/XWayland DISPLAY is available"
                .to_string(),
        );
    }
    native_visual_capture_helper_path()?;
    Ok(NativeVisualAvailability::Ready)
}

pub(crate) fn write_native_visual_summary(
    run_dir: &Path,
    summary: &NativeVisualSummary,
) -> Result<(), String> {
    let json_path = run_dir.join("native_visual_summary.json");
    let text_path = run_dir.join("native_visual_summary.txt");
    let json_value = json!({
        "backend": summary.backend,
        "display_protocol": summary.display_protocol,
        "window_id": summary.window_id,
        "font_family": summary.font_family,
        "font_size": summary.font_size,
        "cols": summary.cols,
        "rows": summary.rows,
        "ghostty_version": summary.ghostty_version,
        "tmux_version": summary.tmux_version,
        "cleanup_verified": summary.cleanup_verified,
    });
    let rendered = serde_json::to_string_pretty(&json_value)
        .map_err(|err| format!("failed to serialize native visual summary JSON: {err}"))?;
    fs::write(&json_path, rendered).map_err(|err| {
        format!(
            "failed to write native visual summary {}: {err}",
            json_path.display()
        )
    })?;
    let text = [
        format!("backend: {}", summary.backend),
        format!("display_protocol: {}", summary.display_protocol),
        format!("window_id: {}", summary.window_id),
        format!("font_family: {}", summary.font_family),
        format!("font_size: {}", summary.font_size),
        format!("cols: {}", summary.cols),
        format!("rows: {}", summary.rows),
        format!("ghostty_version: {}", summary.ghostty_version),
        format!("tmux_version: {}", summary.tmux_version),
        format!("cleanup_verified: {}", summary.cleanup_verified),
    ]
    .join("\n");
    fs::write(&text_path, format!("{text}\n")).map_err(|err| {
        format!(
            "failed to write native visual summary {}: {err}",
            text_path.display()
        )
    })
}

pub(crate) fn write_solid_png(
    path: &Path,
    width: u32,
    height: u32,
    color: [u8; 3],
) -> Result<(), String> {
    let image = RgbImage::from_pixel(width, height, image::Rgb(color));
    image
        .save(path)
        .map_err(|err| format!("failed to save test PNG {}: {err}", path.display()))
}

fn ensure_native_visual_prereqs() -> Result<(), String> {
    for command in ["ghostty", "tmux", "xprop"] {
        let found = env::var_os("PATH").is_some_and(|_| {
            Command::new("sh")
                .arg("-lc")
                .arg(format!("command -v {command}"))
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        });
        if !found {
            return Err(format!(
                "native visual lane requires `{command}` to be installed"
            ));
        }
    }

    if env::var("DISPLAY")
        .ok()
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err("native visual lane requires an X11/XWayland DISPLAY".to_string());
    }
    native_visual_capture_helper_path()?;
    Ok(())
}

fn write_launcher_script(
    script_path: &Path,
    harness_bin: &Path,
    session_dir: &Path,
    extra_args: &[&str],
) -> Result<(), String> {
    let extra_args = if extra_args.is_empty() {
        String::new()
    } else {
        format!(
            " {}",
            extra_args
                .iter()
                .map(|arg| shell_escape_text(arg))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };
    let script = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\nexec {} tui{} --deterministic --session-dir {}\n",
        shell_escape(harness_bin),
        extra_args,
        shell_escape(session_dir),
    );
    fs::write(script_path, script).map_err(|err| {
        format!(
            "failed to write native visual launcher {}: {err}",
            script_path.display()
        )
    })?;
    let mut permissions = fs::metadata(script_path)
        .map_err(|err| {
            format!(
                "failed to stat native visual launcher {}: {err}",
                script_path.display()
            )
        })?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(script_path, permissions).map_err(|err| {
        format!(
            "failed to mark native visual launcher {} executable: {err}",
            script_path.display()
        )
    })
}

fn shell_escape(path: &Path) -> String {
    shell_escape_text(&path.to_string_lossy())
}

fn shell_escape_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn run_command(command: &mut Command, description: &str) -> Result<(), String> {
    let output = run_command_output(command, description)?;
    if output.status.success() {
        return Ok(());
    }
    Err(format_command_failure(description, &output))
}

fn run_command_output(command: &mut Command, description: &str) -> Result<Output, String> {
    command
        .output()
        .map_err(|err| format!("failed to {description}: {err}"))
}

fn best_effort_run(command: &mut Command) {
    let _ = command.output();
}

fn format_command_failure(description: &str, output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    format!(
        "failed to {description}: status={} stdout=`{}` stderr=`{}`",
        output.status, stdout, stderr
    )
}

fn read_first_line(output: Output) -> String {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("unknown")
        .trim()
        .to_string()
}

fn wait_for_window_id(
    ghostty: &mut Child,
    title: &str,
    timeout: Duration,
    stderr_path: &Path,
) -> Result<String, String> {
    let pid = ghostty.id();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(window_id) = locate_window_id(pid, title)? {
            return Ok(window_id);
        }
        if let Some(status) = ghostty
            .try_wait()
            .map_err(|err| format!("failed checking Ghostty native visual status: {err}"))?
        {
            return Err(format!(
                "Ghostty native visual process exited before window lookup succeeded for `{title}` (pid {pid}, status: {status}); stderr=`{}`",
                read_trimmed_file(stderr_path)
            ));
        }
        thread::sleep(SCREEN_POLL_INTERVAL);
    }

    Err(format!(
        "timed out waiting for Ghostty window titled `{title}` for pid {pid}; stderr=`{}`",
        read_trimmed_file(stderr_path)
    ))
}

fn read_trimmed_file(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn locate_window_id(pid: u32, title: &str) -> Result<Option<String>, String> {
    let output = run_command_output(
        Command::new("xprop").arg("-root").arg("_NET_CLIENT_LIST"),
        "query X11 client window list",
    )?;
    if !output.status.success() {
        return Err(format_command_failure(
            "query X11 client window list",
            &output,
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| format!("xprop root window output was not valid UTF-8: {err}"))?;
    let id_regex = Regex::new(r"0x[0-9a-fA-F]+")
        .map_err(|err| format!("failed to compile X11 window id regex: {err}"))?;
    for mat in id_regex.find_iter(&stdout) {
        let window_id = mat.as_str().to_string();
        let output = run_command_output(
            Command::new("xprop")
                .arg("-id")
                .arg(&window_id)
                .arg("_NET_WM_PID")
                .arg("_NET_WM_NAME")
                .arg("WM_NAME")
                .arg("WM_CLASS"),
            "inspect X11 window properties",
        )?;
        if !output.status.success() {
            continue;
        }
        let properties = String::from_utf8_lossy(&output.stdout);
        let pid_match = properties.contains(&format!("= {pid}"));
        let title_match = properties.contains(title);
        if pid_match || title_match {
            return Ok(Some(window_id));
        }
    }

    Ok(None)
}

fn wait_for_window_gone(window_id: &str, timeout: Duration) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if locate_window_id_by_id(window_id)?.is_none() {
            return Ok(true);
        }
        thread::sleep(SCREEN_POLL_INTERVAL);
    }
    Ok(false)
}

fn locate_window_id_by_id(window_id: &str) -> Result<Option<String>, String> {
    let output = run_command_output(
        Command::new("xprop").arg("-root").arg("_NET_CLIENT_LIST"),
        "query X11 client window list",
    )?;
    if !output.status.success() {
        return Err(format_command_failure(
            "query X11 client window list",
            &output,
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| format!("xprop root window output was not valid UTF-8: {err}"))?;
    let id_regex = Regex::new(r"0x[0-9a-fA-F]+")
        .map_err(|err| format!("failed to compile X11 window id regex: {err}"))?;
    let matched = id_regex
        .find_iter(&stdout)
        .map(|mat| mat.as_str())
        .find(|candidate| *candidate == window_id)
        .map(str::to_string);
    Ok(matched)
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let base = env::temp_dir().join("harness-testkit");
    let _ = fs::create_dir_all(&base);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    base.join(format!("{prefix}-{}-{nanos}", std::process::id()))
}

fn detect_display_protocol() -> String {
    let has_display = env::var("DISPLAY")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    let has_wayland = env::var("WAYLAND_DISPLAY")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    match (has_display, has_wayland) {
        (true, true) => "xwayland".to_string(),
        (true, false) => "x11".to_string(),
        (false, true) => "wayland".to_string(),
        (false, false) => "headless".to_string(),
    }
}

fn native_visual_capture_helper_path() -> Result<PathBuf, String> {
    let helper = if let Ok(raw) = env::var(NATIVE_VISUAL_CAPTURE_HELPER_ENV) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(format!(
                "native visual lane requires {} to be non-empty when set",
                NATIVE_VISUAL_CAPTURE_HELPER_ENV
            ));
        }
        PathBuf::from(trimmed)
    } else {
        resolve_bundled_native_visual_capture_helper()?
    };
    validate_executable_file(&helper, "native visual capture helper")?;
    ensure_native_visual_capture_helper_desktop_entry(&helper)?;
    Ok(helper)
}

fn resolve_bundled_native_visual_capture_helper() -> Result<PathBuf, String> {
    if let Some(path) = option_env!("CARGO_BIN_EXE_native_visual_helper") {
        let helper = PathBuf::from(path);
        if helper.exists() {
            return Ok(helper);
        }
    }
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let helper = crate_root
        .join("..")
        .join("..")
        .join("target")
        .join("debug")
        .join(binary_name("native_visual_helper"));
    if !helper.exists() {
        let workspace_root = crate_root.join("..").join("..");
        run_command(
            Command::new("cargo")
                .arg("build")
                .arg("-p")
                .arg("harness-testkit")
                .arg("--bin")
                .arg("native_visual_helper")
                .current_dir(&workspace_root),
            "build bundled native visual helper",
        )?;
    }
    if !helper.exists() {
        return Err(format!(
            "failed to locate bundled native visual helper at {}",
            helper.display()
        ));
    }
    Ok(helper)
}

fn ensure_native_visual_capture_helper_desktop_entry(helper: &Path) -> Result<(), String> {
    let desktop_dir = desktop_applications_dir()?;
    fs::create_dir_all(&desktop_dir).map_err(|err| {
        format!(
            "failed to create desktop applications directory {}: {err}",
            desktop_dir.display()
        )
    })?;
    let desktop_path = desktop_dir.join(NATIVE_VISUAL_HELPER_DESKTOP_ID);
    let contents = format!(
        "[Desktop Entry]\nType=Application\nName=Agent Harness Native Visual Helper\nExec={}\nStartupNotify=false\nTerminal=false\nCategories=Utility;\nX-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n",
        helper.display(),
    );
    let should_write = match fs::read_to_string(&desktop_path) {
        Ok(existing) => existing != contents,
        Err(_) => true,
    };
    if should_write {
        fs::write(&desktop_path, contents).map_err(|err| {
            format!(
                "failed to write native visual desktop entry {}: {err}",
                desktop_path.display()
            )
        })?;
        best_effort_run(Command::new("kbuildsycoca6").arg("--noincremental"));
        best_effort_run(Command::new("kbuildsycoca5").arg("--noincremental"));
        thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}

fn desktop_applications_dir() -> Result<PathBuf, String> {
    if let Ok(data_home) = env::var("XDG_DATA_HOME") {
        let trimmed = data_home.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed).join("applications"));
        }
    }
    let home = env::var("HOME")
        .map_err(|_| "could not determine HOME for native visual desktop entry".to_string())?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("applications"))
}

fn validate_executable_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|err| format!("{label} {} is not accessible: {err}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{label} {} is not a file", path.display()));
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!("{label} {} is not executable", path.display()));
    }
    Ok(())
}

fn binary_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn merge_metadata(metadata: Option<Value>, additions: Value) -> Value {
    match (metadata, additions) {
        (Some(Value::Object(mut base)), Value::Object(extra)) => {
            for (key, value) in extra {
                base.insert(key, value);
            }
            Value::Object(base)
        }
        (None, value) => value,
        (Some(existing), _) => existing,
    }
}

fn read_external_capture_provenance(
    metadata_path: &Path,
    helper_path: &Path,
    expected_window_id: &str,
) -> Result<ExternalCaptureProvenance, String> {
    let metadata = fs::read_to_string(metadata_path).map_err(|err| {
        format!(
            "failed to read native visual capture metadata {}: {err}",
            metadata_path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&metadata).map_err(|err| {
        format!(
            "failed to parse native visual capture metadata {}: {err}",
            metadata_path.display()
        )
    })?;
    let captured_window_id = metadata_string_field(&value, "captured_window_id")?;
    if captured_window_id != expected_window_id {
        return Err(format!(
            "native visual capture helper {} reported captured_window_id={} but expected {}",
            helper_path.display(),
            captured_window_id,
            expected_window_id
        ));
    }
    Ok(ExternalCaptureProvenance {
        helper_path: helper_path.to_path_buf(),
        capture_backend: metadata_string_field(&value, "capture_backend")?,
        capture_tool: metadata_string_field(&value, "capture_tool")?,
        captured_window_id,
        captured_window_title: value
            .get("captured_window_title")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn metadata_string_field(value: &Value, field: &str) -> Result<String, String> {
    let string = value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("native visual capture metadata missing non-empty `{field}`"))?;
    Ok(string.to_string())
}

fn ensure_non_uniform_capture(image: &RgbImage, window_id: &str) -> Result<(), String> {
    if let Some(color) = capture_uniform_color(image) {
        return Err(format!(
            "native visual window {window_id} capture was uniformly {:?}; refusing to treat a flat-color frame as a trustworthy screenshot",
            color
        ));
    }
    Ok(())
}

fn capture_uniform_color(image: &RgbImage) -> Option<[u8; 3]> {
    let first = image.get_pixel(0, 0);
    image
        .pixels()
        .all(|pixel| pixel == first)
        .then_some(first.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_capture_provenance_accepts_matching_window_id() {
        let dir = unique_temp_dir("native-visual-helper-metadata");
        fs::create_dir_all(&dir).expect("create helper metadata dir");
        let metadata_path = dir.join("capture.json");
        let helper_path = dir.join("helper");
        fs::write(
            &metadata_path,
            r#"{
  "captured_window_id": "0x123",
  "captured_window_title": "ghostty-test",
  "capture_backend": "kwin_screenshot2",
  "capture_tool": "native-visual-helper"
}"#,
        )
        .expect("write capture metadata");

        let provenance = read_external_capture_provenance(&metadata_path, &helper_path, "0x123")
            .expect("matching metadata should parse");
        assert_eq!(provenance.captured_window_id, "0x123");
        assert_eq!(provenance.capture_backend, "kwin_screenshot2");
        assert_eq!(provenance.capture_tool, "native-visual-helper");
        assert_eq!(
            provenance.captured_window_title.as_deref(),
            Some("ghostty-test")
        );
    }

    #[test]
    fn external_capture_provenance_rejects_mismatched_window_id() {
        let dir = unique_temp_dir("native-visual-helper-metadata-mismatch");
        fs::create_dir_all(&dir).expect("create helper metadata dir");
        let metadata_path = dir.join("capture.json");
        let helper_path = dir.join("helper");
        fs::write(
            &metadata_path,
            r#"{
  "captured_window_id": "0xabc",
  "capture_backend": "kwin_screenshot2",
  "capture_tool": "native-visual-helper"
}"#,
        )
        .expect("write capture metadata");

        let err = read_external_capture_provenance(&metadata_path, &helper_path, "0xdef")
            .expect_err("mismatched window id should fail");
        assert!(err.contains("captured_window_id=0xabc"));
        assert!(err.contains("expected 0xdef"));
    }
}

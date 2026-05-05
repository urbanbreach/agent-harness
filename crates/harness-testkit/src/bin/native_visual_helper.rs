use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::os::fd::OwnedFd as StdOwnedFd;
use std::path::PathBuf;
use std::process::Command;

use image::{ImageBuffer, Rgba};
use serde_json::json;
use zbus::blocking::Connection;
use zbus::blocking::Proxy;
use zbus::zvariant::{OwnedFd, OwnedValue};

const KWIN_SCREENSHOT_DESTINATION: &str = "org.kde.KWin.ScreenShot2";
const KWIN_SCREENSHOT_PATH: &str = "/org/kde/KWin/ScreenShot2";
const KWIN_SCREENSHOT_INTERFACE: &str = "org.kde.KWin.ScreenShot2";
const QIMAGE_FORMAT_RGB32: u32 = 4;
const QIMAGE_FORMAT_ARGB32: u32 = 5;
const QIMAGE_FORMAT_ARGB32_PREMULTIPLIED: u32 = 6;
const QIMAGE_FORMAT_RGBX8888: u32 = 16;
const QIMAGE_FORMAT_RGBA8888: u32 = 17;
const QIMAGE_FORMAT_RGBA8888_PREMULTIPLIED: u32 = 18;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse(env::args().skip(1))?;
    let active_before = active_window_id()?;
    if active_before != args.window_id {
        return Err(format!(
            "expected active X11 window {} before capture, got {}",
            args.window_id, active_before
        ));
    }

    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create native visual output directory {}: {err}",
                parent.display()
            )
        })?;
    }
    if let Some(parent) = args.metadata_out.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create native visual metadata directory {}: {err}",
                parent.display()
            )
        })?;
    }

    let raw_output = args.output.with_extension("raw");
    let file = File::create(&raw_output).map_err(|err| {
        format!(
            "failed to create native visual raw screenshot {}: {err}",
            raw_output.display()
        )
    })?;
    let owned_fd: StdOwnedFd = file.into();
    let dbus_fd = OwnedFd::from(owned_fd);

    let connection = Connection::session()
        .map_err(|err| format!("failed to open session D-Bus connection: {err}"))?;
    let proxy = Proxy::new(
        &connection,
        KWIN_SCREENSHOT_DESTINATION,
        KWIN_SCREENSHOT_PATH,
        KWIN_SCREENSHOT_INTERFACE,
    )
    .map_err(|err| format!("failed to create KWin screenshot proxy: {err}"))?;
    let options: HashMap<String, OwnedValue> = HashMap::new();
    let reply: HashMap<String, OwnedValue> = proxy
        .call("CaptureActiveWindow", &(options, dbus_fd))
        .map_err(|err| format!("failed to call KWin CaptureActiveWindow: {err}"))?;
    write_png_from_kwin_reply(&raw_output, &args.output, &reply)?;
    let _ = fs::remove_file(&raw_output);

    let active_after = active_window_id()?;
    if active_after != args.window_id {
        return Err(format!(
            "expected active X11 window {} after capture, got {}",
            args.window_id, active_after
        ));
    }

    let metadata = json!({
        "captured_window_id": args.window_id,
        "captured_window_title": args.window_title,
        "capture_backend": "kwin_screenshot2_active_window",
        "capture_tool": "native_visual_helper",
        "active_window_before": active_before,
        "active_window_after": active_after,
        "kwin_reply": stringify_reply(&reply),
        "output_path": args.output.display().to_string(),
    });
    fs::write(
        &args.metadata_out,
        serde_json::to_string_pretty(&metadata)
            .map_err(|err| format!("failed to serialize native visual metadata: {err}"))?,
    )
    .map_err(|err| {
        format!(
            "failed to write native visual metadata {}: {err}",
            args.metadata_out.display()
        )
    })?;

    Ok(())
}

fn active_window_id() -> Result<String, String> {
    let output = Command::new("xprop")
        .arg("-root")
        .arg("_NET_ACTIVE_WINDOW")
        .output()
        .map_err(|err| format!("failed to run xprop for active window lookup: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "failed to query active X11 window: status={} stdout=`{}` stderr=`{}`",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| format!("active window xprop output was not valid UTF-8: {err}"))?;
    stdout
        .split_whitespace()
        .find(|token| token.starts_with("0x"))
        .map(str::to_string)
        .ok_or_else(|| format!("could not parse active window id from xprop output `{stdout}`"))
}

fn write_png_from_kwin_reply(
    raw_path: &PathBuf,
    png_path: &PathBuf,
    reply: &HashMap<String, OwnedValue>,
) -> Result<(), String> {
    let width = reply_u32(reply, "width")?;
    let height = reply_u32(reply, "height")?;
    let stride = reply_u32(reply, "stride")?;
    let format = reply_u32(reply, "format")?;
    let raw = fs::read(raw_path).map_err(|err| {
        format!(
            "failed to read native visual raw screenshot {}: {err}",
            raw_path.display()
        )
    })?;
    let expected_len = stride as usize * height as usize;
    if raw.len() < expected_len {
        return Err(format!(
            "native visual raw screenshot {} was {} bytes but expected at least {} from stride={} height={}",
            raw_path.display(),
            raw.len(),
            expected_len,
            stride,
            height
        ));
    }
    let mut image = ImageBuffer::<Rgba<u8>, Vec<u8>>::new(width, height);
    for y in 0..height {
        let row = &raw[y as usize * stride as usize..(y as usize + 1) * stride as usize];
        for x in 0..width {
            let pixel = match format {
                QIMAGE_FORMAT_RGB32 | QIMAGE_FORMAT_ARGB32 | QIMAGE_FORMAT_ARGB32_PREMULTIPLIED => {
                    let offset = x as usize * 4;
                    Rgba([
                        row[offset + 2],
                        row[offset + 1],
                        row[offset],
                        row[offset + 3],
                    ])
                }
                QIMAGE_FORMAT_RGBX8888
                | QIMAGE_FORMAT_RGBA8888
                | QIMAGE_FORMAT_RGBA8888_PREMULTIPLIED => {
                    let offset = x as usize * 4;
                    Rgba([
                        row[offset],
                        row[offset + 1],
                        row[offset + 2],
                        row[offset + 3],
                    ])
                }
                other => {
                    return Err(format!(
                        "unsupported KWin QImage format {other}; raw reply {:?}",
                        stringify_reply(reply)
                    ));
                }
            };
            image.put_pixel(x, y, pixel);
        }
    }
    image.save(png_path).map_err(|err| {
        format!(
            "failed to save native visual PNG {}: {err}",
            png_path.display()
        )
    })
}

fn reply_u32(reply: &HashMap<String, OwnedValue>, key: &str) -> Result<u32, String> {
    let value = reply
        .get(key)
        .ok_or_else(|| format!("KWin screenshot reply missing `{key}`"))?
        .clone();
    u32::try_from(value)
        .map_err(|err| format!("failed to decode KWin screenshot reply `{key}`: {err}"))
}

fn stringify_reply(reply: &HashMap<String, OwnedValue>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (key, value) in reply {
        map.insert(key.clone(), serde_json::Value::String(format!("{value:?}")));
    }
    serde_json::Value::Object(map)
}

#[derive(Debug, Clone)]
struct Args {
    output: PathBuf,
    metadata_out: PathBuf,
    window_id: String,
    window_title: String,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut output = None;
        let mut metadata_out = None;
        let mut window_id = None;
        let mut window_title = None;
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .ok_or_else(|| format!("missing value for argument `{flag}`"))?;
            match flag.as_str() {
                "--output" => output = Some(PathBuf::from(value)),
                "--metadata-out" => metadata_out = Some(PathBuf::from(value)),
                "--window-id" => window_id = Some(value),
                "--window-title" => window_title = Some(value),
                _ => return Err(format!("unknown argument `{flag}`")),
            }
        }
        let output = output.ok_or_else(|| "missing required --output".to_string())?;
        let metadata_out =
            metadata_out.ok_or_else(|| "missing required --metadata-out".to_string())?;
        let window_id = window_id.ok_or_else(|| "missing required --window-id".to_string())?;
        let window_title =
            window_title.ok_or_else(|| "missing required --window-title".to_string())?;
        if output.as_os_str().is_empty() {
            return Err("--output cannot be empty".to_string());
        }
        if metadata_out.as_os_str().is_empty() {
            return Err("--metadata-out cannot be empty".to_string());
        }
        for (flag, value) in [
            ("--window-id", window_id.as_str()),
            ("--window-title", window_title.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(format!("{flag} cannot be empty"));
            }
        }
        Ok(Self {
            output,
            metadata_out,
            window_id,
            window_title,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Args;

    fn parse_args(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(|value| value.to_string()))
    }

    #[test]
    fn args_parse_rejects_empty_required_text_values() {
        let err = parse_args(&[
            "--output",
            "capture.png",
            "--metadata-out",
            "capture.json",
            "--window-id",
            "  ",
            "--window-title",
            "title",
        ])
        .expect_err("blank window id should be rejected");
        assert_eq!(err, "--window-id cannot be empty");

        let err = parse_args(&[
            "--output",
            "capture.png",
            "--metadata-out",
            "capture.json",
            "--window-id",
            "0x123",
            "--window-title",
            "\t",
        ])
        .expect_err("blank window title should be rejected");
        assert_eq!(err, "--window-title cannot be empty");
    }

    #[test]
    fn args_parse_preserves_non_empty_required_text_values() {
        let args = parse_args(&[
            "--output",
            "capture.png",
            "--metadata-out",
            "capture.json",
            "--window-id",
            " 0x123 ",
            "--window-title",
            " title ",
        ])
        .expect("non-empty values should parse");

        assert_eq!(args.window_id, " 0x123 ");
        assert_eq!(args.window_title, " title ");
    }
}

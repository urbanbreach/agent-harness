// Record the README demo: the real TUI fixing a failing test with a signed-in
// model. See docs/assets/README.md.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { basename, join } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";
import { openBrowserTerminal } from "./lib/browser-terminal.mjs";
import { resolveCommand, spawnHarnessPty } from "./lib/pty-session.mjs";
import { assertSecretFree } from "./lib/security.mjs";

const repo = fileURLToPath(new URL("../../", import.meta.url));
const assets = join(repo, "docs/assets");
const realHome = homedir();
const harnessHome = process.env.HARNESS_HOME || join(realHome, ".harness");
const prompt = "cargo test is failing. Find the root cause and fix it.";
const [cols, rows] = [100, 32];
// Individual tool cards read better than eval cells in a short clip, and a
// cold rust-analyzer only answers "server cancelled the request".
const config = { lsp: false, permission: { "*": "allow", eval: "deny" } };
if (process.env.HARNESS_DEMO_MODEL) config.model = process.env.HARNESS_DEMO_MODEL;
const temporary = await mkdtemp(join(tmpdir(), "harness-readme-"));
// A fake HOME makes the header read ~/tempo instead of a temporary path.
const home = join(temporary, "home");
const workspace = join(home, "tempo");
const frames = [];
let terminal;
let pty;

const files = {
  ".gitignore": "/target\n",
  "Cargo.toml": '[package]\nname = "tempo"\nversion = "0.1.0"\nedition = "2024"\n',
  "src/lib.rs": `//! Parse human durations such as "1h30m", "45s" or "90".

/// Returns the number of seconds in \`input\`. A bare trailing number counts as seconds.
pub fn parse_duration(input: &str) -> Result<u64, String> {
    let mut total = 0;
    let mut digits = String::new();
    for ch in input.trim().chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        let value: u64 = digits
            .parse()
            .map_err(|_| format!("expected a number before '{ch}'"))?;
        total += value
            * match ch {
                'h' => 3600,
                'm' => 60,
                's' => 1,
                _ => return Err(format!("unknown unit '{ch}'")),
            };
        digits.clear();
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::parse_duration;

    #[test]
    fn units() {
        assert_eq!(parse_duration("1h30m"), Ok(5400));
        assert_eq!(parse_duration("2h5m10s"), Ok(7510));
    }

    #[test]
    fn bare_seconds() {
        assert_eq!(parse_duration("90"), Ok(90));
        assert_eq!(parse_duration("1m30"), Ok(90));
    }

    #[test]
    fn rejects_unknown_units() {
        assert!(parse_duration("3d").is_err());
    }
}
`,
};

async function shot(seconds) {
  const file = join(temporary, `frame-${String(frames.length).padStart(4, "0")}.png`);
  await terminal.screenshot(file);
  frames.push({ file, seconds });
}

// Agent edits are attributed under <home>/projects/<key>. Remove only a
// directory this run created whose journal holds the fixture's own text.
async function removeDemoProjectData(before) {
  const root = join(harnessHome, "projects");
  const marker = Buffer.from(files["src/lib.rs"].split("\n")[0]).toString("hex");
  for (const name of (await readdir(root).catch(() => [])).filter((entry) => !before.includes(entry))) {
    const journal = await readFile(join(root, name, "edit-attribution.jsonl"), "utf8").catch(() => "");
    if (journal.includes(marker)) await rm(join(root, name), { recursive: true, force: true });
  }
}

const projectsBefore = await readdir(join(harnessHome, "projects")).catch(() => []);
try {
  for (const [name, body] of Object.entries(files)) {
    await mkdir(join(workspace, name, ".."), { recursive: true });
    await writeFile(join(workspace, name), body);
  }
  const git = (...args) => execFileSync("git", args, { cwd: workspace, stdio: "ignore" });
  git("init", "-q", "-b", "main");
  git("-c", "user.name=Harness demo", "-c", "user.email=demo@example.invalid", "add", ".");
  git("-c", "user.name=Harness demo", "-c", "user.email=demo@example.invalid", "commit", "-qm", "init");

  terminal = await openBrowserTerminal({
    browser: process.env.HARNESS_QA_BROWSER ?? "/usr/bin/chromium",
    cols,
    rows,
    timeoutMs: 30000,
    title: "Harness demo",
    profilePath: join(temporary, "browser-profile"),
    onInput: (data) => pty?.write(data) ?? false,
  });
  pty = spawnHarnessPty({
    command: await resolveCommand(
      "harness tui --session-dir $HARNESS_QA_SESSION_DIR", repo, join(repo, "target/release/harness"),
    ),
    cols,
    rows,
    cwd: workspace,
    sessionDir: join(temporary, "sessions"),
    tempRoot: temporary,
    disableAnimations: false,
    environment: {
      TERM: "xterm-256color",
      COLORTERM: "truecolor",
      HOME: home,
      HARNESS_HOME: harnessHome,
      HARNESS_DETERMINISTIC: "0",
      HARNESS_CONFIG_CONTENT: JSON.stringify(config),
      HARNESS_MODEL_SELECTION_STATE_FILE: join(temporary, "model.json"),
      CARGO_HOME: process.env.CARGO_HOME ?? join(realHome, ".cargo"),
      RUSTUP_HOME: process.env.RUSTUP_HOME ?? join(realHome, ".rustup"),
    },
    onOutput: (bytes) => terminal.write(bytes),
  });
  await terminal.waitForText("❯");
  await delay(1500);
  for (let index = 0; index < 8; index += 1) await shot(0.1);
  for (const character of prompt) {
    await terminal.type(character);
    await shot(0.04);
  }
  for (let index = 0; index < 6; index += 1) await shot(0.1);
  await terminal.key("Enter");

  // Sample the live turn. Long waits keep a short stretch of frames so the
  // animation stays watchable; letters-only keys ignore timers and token rates.
  let previous = "";
  let idle = 0;
  const deadline = Date.now() + 300_000;
  for (;;) {
    assert(Date.now() < deadline, "the demo turn did not finish within five minutes");
    const { text } = await terminal.snapshot();
    const key = text.replace(/[^A-Za-z]/g, "");
    idle = key === previous ? idle + 1 : 0;
    previous = key;
    if (idle < 6) await shot(0.12);
    if (text.includes("Worked for")) break;
    await delay(130);
  }
  await delay(800);
  for (let index = 0; index < 40; index += 1) await shot(0.1);
  const still = await terminal.capture(join(temporary, "harness-tui.png"));
  assert(still.text.includes("Worked for"), "the demo must end on a finished turn");
  assertSecretFree(pty.raw());
  execFileSync("cargo", ["test", "--quiet"], { cwd: workspace, stdio: "ignore" });

  const list = frames.map(({ file, seconds }) => `file '${basename(file)}'\nduration ${seconds}\n`);
  await writeFile(join(temporary, "frames.txt"), `ffconcat version 1.0\n${list.join("")}file '${basename(frames.at(-1).file)}'\n`);
  execFileSync("ffmpeg", [
    "-hide_banner", "-loglevel", "error", "-y",
    "-f", "concat", "-safe", "0", "-i", join(temporary, "frames.txt"),
    "-filter_complex", "split[a][b];[a]palettegen=stats_mode=full[p];[b][p]paletteuse=dither=none:diff_mode=rectangle",
    "-fps_mode", "vfr", "-loop", "0", join(temporary, "harness-demo.gif"),
  ]);
  await copyFile(join(temporary, "harness-demo.gif"), join(assets, "harness-demo.gif"));
  await copyFile(join(temporary, "harness-tui.png"), join(assets, "harness-tui.png"));
  const seconds = frames.reduce((sum, frame) => sum + frame.seconds, 0);
  process.stdout.write(`Recorded ${frames.length} frames (${seconds.toFixed(1)}s) in docs/assets/harness-demo.gif\n`);
} finally {
  try { await pty?.cleanup(); }
  finally {
    try { await terminal?.close(); }
    finally {
      await removeDemoProjectData(projectsBefore);
      await rm(temporary, { recursive: true, force: true });
    }
  }
}

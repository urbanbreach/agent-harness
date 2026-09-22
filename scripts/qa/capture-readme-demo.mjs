// Record the real TUI with its offline provider. See docs/assets/README.md.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { copyFile, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";
import { openBrowserTerminal } from "./lib/browser-terminal.mjs";
import { prepareHarnessWorkspace, resolveCommand, spawnHarnessPty } from "./lib/pty-session.mjs";
import { assertSecretFree } from "./lib/security.mjs";

const repo = fileURLToPath(new URL("../../", import.meta.url));
const temporary = await mkdtemp(join(tmpdir(), "harness-readme-"));
const assets = join(repo, "docs/assets");
let terminal;
let pty;
let frame = 0;

async function hold(count) {
  for (let index = 0; index < count; index += 1) {
    await terminal.screenshot(join(temporary, `frame-${String(frame++).padStart(4, "0")}.png`));
    await delay(100);
  }
}

try {
  const fixture = await prepareHarnessWorkspace(temporary);
  terminal = await openBrowserTerminal({
    browser: process.env.HARNESS_QA_BROWSER ?? "/usr/bin/chromium",
    cols: 100,
    rows: 28,
    timeoutMs: 20000,
    title: "Harness offline demo",
    profilePath: join(temporary, "browser-profile"),
    onInput: (data) => pty?.write(data) ?? false,
  });
  const ptySettings = {
    command: await resolveCommand(
      "harness tui --mock --deterministic --session-dir $HARNESS_QA_SESSION_DIR", repo,
    ),
    cols: 100,
    rows: 28,
    cwd: fixture.workspace,
    sessionDir: fixture.sessionDir,
    tempRoot: temporary,
    disableAnimations: false,
    environment: {
      TERM: "xterm-256color", COLORTERM: "truecolor",
      XDG_CONFIG_HOME: join(temporary, "config"),
      HARNESS_DISABLE_MODELS_FETCH: "1",
    },
    onOutput: (bytes) => terminal.write(bytes),
  };
  pty = spawnHarnessPty(ptySettings);
  await terminal.waitForText("Demo mode");
  await hold(12);
  for (const character of "Hello from PTY") {
    await terminal.type(character);
    await hold(1);
  }
  await hold(5);
  await terminal.key("Enter");
  await terminal.waitForText("Hello world");
  await hold(20);
  await terminal.key("Control+P");
  await terminal.waitForText("Commands");
  await hold(18);
  await terminal.key("Escape");
  await terminal.waitForTextAbsent("Commands");
  await hold(12);
  assertSecretFree(pty.raw());
  await pty.cleanup();
  await terminal.write(Buffer.from("\x1bc"));
  pty = spawnHarnessPty({
    ...ptySettings,
    command: await resolveCommand(
      "harness tui --scenario golden_path_interactive --deterministic --session-dir $HARNESS_QA_SESSION_DIR/edit-demo", repo,
    ),
  });
  await terminal.waitForText("Allow Edit");
  await hold(20);
  await terminal.key("Enter");
  await terminal.key("Enter");
  await terminal.waitForTextAbsent("Allow Edit");
  await terminal.waitForText("demo.txt");
  await hold(25);
  const snapshot = await terminal.capture(join(temporary, "harness-tui.png"));
  assert(
    snapshot.text.includes("demo.txt") && snapshot.text.includes("BETA"),
    "The demo must show the applied fixture edit",
  );
  assertSecretFree(pty.raw());
  execFileSync("ffmpeg", [
    "-hide_banner", "-loglevel", "error", "-y", "-framerate", "10",
    "-i", join(temporary, "frame-%04d.png"),
    "-filter_complex", "split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=3",
    "-loop", "0", join(temporary, "harness-demo.gif"),
  ]);
  await copyFile(join(temporary, "harness-demo.gif"), join(assets, "harness-demo.gif"));
  await copyFile(join(temporary, "harness-tui.png"), join(assets, "harness-tui.png"));
  process.stdout.write(`Recorded ${frame} frames in docs/assets/harness-demo.gif\n`);
} finally {
  try { await pty?.cleanup(); }
  finally {
    try { await terminal?.close(); }
    finally { await rm(temporary, { recursive: true, force: true }); }
  }
}

// Record the README demo: the real TUI bringing this repository's docs back in
// line with its code, with a signed-in model. See docs/assets/README.md.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { copyFile, cp, mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
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
const prompt = "The docs have drifted a lot from the code. Go through every page in docs/ and fix anything that's out of date.";
// Typed while the agent waits on its subagents; it reaches the agent's next request.
const steer = "When they're done, sum up the changes in a table with one row per file.";
// A maximized terminal on a 1920x1080 screen: 16px cells are 9.6 by 20 pixels.
const [cols, rows] = [200, 54];
// The docs at this commit lag the code; the next commit, 8512d08f, fixed them
// by hand. The session sees it as HEAD, so every run starts from real drift.
const commit = "ce53083797df9eb75426c6ac1c1f5ff9a6363b29";
// Individual tool cards read better than eval cells, and a cold
// rust-analyzer only answers "server cancelled the request". Nobody answers
// prompts here, so paths outside the checkout and repeated calls fail fast.
const config = {
  model: process.env.HARNESS_DEMO_MODEL ?? "anthropic-subscription/claude-sonnet-5-5/medium",
  lsp: false,
  permission: { "*": "allow", eval: "deny", external_directory: "deny", doom_loop: "deny" },
};
const temporary = await mkdtemp(join(tmpdir(), "harness-readme-"));
// A fake HOME makes the header read ~/agent-harness instead of a temporary path.
const home = join(temporary, "home");
const workspace = join(home, "agent-harness");
const frames = [];
let terminal;
let pty;

async function shot(seconds) {
  const file = join(temporary, `frame-${String(frames.length).padStart(4, "0")}.png`);
  await terminal.screenshot(file);
  frames.push({ file, seconds });
}

// Agent edits are attributed under <home>/projects/<key>. Remove only a
// directory this run created whose journal records a file as it is in the
// clone, and the Claude subscription restart binding of each demo session.
async function removeDemoHomeData(before) {
  const root = join(harnessHome, "projects");
  const sha256 = async (path) => createHash("sha256").update(await readFile(path).catch(() => "")).digest("hex");
  for (const name of (await readdir(root).catch(() => [])).filter((entry) => !before.includes(entry))) {
    const journal = await readFile(join(root, name, "edit-attribution.jsonl"), "utf8").catch(() => "");
    for (const line of journal.split("\n").filter(Boolean)) {
      const entry = JSON.parse(line);
      if (entry.content_sha256 !== await sha256(join(workspace, entry.path))) continue;
      await rm(join(root, name), { recursive: true, force: true });
      break;
    }
  }
  for (const session of await readdir(join(temporary, "sessions")).catch(() => [])) {
    await rm(join(harnessHome, "anthropic-subscription-bindings", `${session}.claude-sdk-oauth-binding.json`), { force: true });
  }
}

const projectsBefore = await readdir(join(harnessHome, "projects")).catch(() => []);
try {
  await mkdir(workspace, { recursive: true });
  const git = (...args) => execFileSync("git", args, { cwd: workspace, stdio: "ignore" });
  git("init", "-q", "-b", "dev");
  git("fetch", "-q", "--no-tags", repo, commit);
  git("reset", "-q", "--hard", "FETCH_HEAD");
  // Skills live outside Git. Bring the ones a session in this checkout sees.
  for (const [from, to] of [
    [join(repo, ".harness/skills"), join(workspace, ".harness/skills")],
    [join(realHome, ".agents/skills"), join(home, ".agents/skills")],
  ]) {
    if (existsSync(from)) await cp(from, to, { recursive: true, dereference: true });
  }

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
      // Builds of the clone stay on disk and warm between recordings.
      CARGO_TARGET_DIR: join(repo, "target/readme-demo"),
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

  // Sample the live turn whenever its letters change (timers and token rates
  // aside), at most twice a second. A long session becomes a time-lapse below.
  const live = frames.length;
  let previous = "";
  let sampled = 0;
  const sample = async (seconds = 0.2) => {
    const { text } = await terminal.snapshot();
    const key = text.replace(/[^A-Za-z]/g, "");
    if (key !== previous && Date.now() - sampled >= 500) {
      previous = key;
      sampled = Date.now();
      await shot(seconds);
    }
    return text;
  };
  const watch = async (ms, seconds) => {
    for (const until = Date.now() + ms; Date.now() < until; await delay(130)) await sample(seconds);
  };
  let delegated = false;
  const deadline = Date.now() + 2_400_000;
  for (;;) {
    assert(Date.now() < deadline, "the demo turn did not finish within forty minutes");
    const text = await sample();
    assert(!text.includes("Allow Permission?"), "the demo cannot answer a permission prompt");
    // Once the agent waits on its subagents, open one, come back and steer.
    if (!delegated && text.includes("[subagent]")) {
      delegated = true;
      await watch(3_000);
      await terminal.clickText("[subagent]");
      await terminal.waitForText("q/Esc:back");
      // The child's view plays slower than the rest so it can be read.
      await watch(20_000, 0.4);
      await terminal.key("Escape");
      await terminal.waitForTextAbsent("q/Esc:back");
      await watch(2_000);
      for (const character of steer) {
        await terminal.type(character);
        await shot(0.04);
      }
      await terminal.key("Enter");
    }
    if (text.includes("Worked for") && text.includes("Enter:send")) break;
    await delay(130);
  }
  assert(delegated, "the demo must show the agent waiting on subagents");
  // Keep at most 300 live frames: a minute of playback at most.
  const step = Math.max(1, Math.ceil((frames.length - live) / 300));
  frames.splice(live, Infinity, ...frames.slice(live).filter((_, index) => index % step === 0));
  await delay(800);
  for (let index = 0; index < 40; index += 1) await shot(0.15);
  const still = await terminal.capture(join(temporary, "harness-tui.png"));
  assert(still.text.includes("Worked for"), "the demo must end on a finished turn");
  assertSecretFree(pty.raw());

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
      try { await removeDemoHomeData(projectsBefore); }
      finally { await rm(temporary, { recursive: true, force: true }); }
    }
  }
}

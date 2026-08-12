import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";
import test from "node:test";

const here = dirname(fileURLToPath(import.meta.url));
const driver = join(here, "web-terminal-visual-qa.mjs");
const fixture = join(here, "xterm-driver-fixture.mjs");
const chromeBin = process.env.CHROME_BIN || process.env.GOOGLE_CHROME_BIN
  || spawnSync("sh", ["-c", "command -v google-chrome || command -v chromium"], { encoding: "utf8" }).stdout.trim();

function runDriver(args) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [driver, ...args], {
      cwd: join(here, "../.."),
      env: { ...process.env, CHROME_BIN: chromeBin },
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, stdout, stderr }));
  });
}

async function json(path) {
  return JSON.parse(await readFile(path, "utf8"));
}

test("table actions capture semantic checkpoints and clean every descendant", { timeout: 30_000 }, async (t) => {
  // Given: a real PTY fixture and a table covering readiness, keys, resize, mouse, and checkpoints.
  const root = await mkdtemp(join(tmpdir(), "harness-xterm-actions-"));
  const evidence = join(root, "evidence");
  t.after(() => rm(root, { recursive: true, force: true }));
  const actionsPath = join(root, "actions.json");
  const actions = [
    { waitForText: { text: "READY", timeoutMs: 5_000 } },
    { checkpoint: { name: "boot" } },
    { key: { key: "Tab", modifiers: { shift: true } } },
    { key: { key: "x", modifiers: { alt: true } } },
    { key: { key: "g", modifiers: { ctrl: true } } },
    { input: { text: "typed" } },
    { resize: { cols: 60, rows: 12 } },
    { waitForText: { text: "RESIZE 60x12", timeoutMs: 5_000 } },
    { mouse: { kind: "move", col: 4, row: 4 } },
    { mouse: { kind: "click", col: 2, row: 2, button: "left" } },
    { mouse: { kind: "wheel", col: 3, row: 3, deltaY: 100 } },
    { mouse: { kind: "drag", from: { col: 2, row: 2 }, to: { col: 8, row: 4 }, button: "left" } },
    { wait: { ms: 25 } },
    { checkpoint: { name: "interactions" } },
  ];
  await writeFile(actionsPath, JSON.stringify(actions));

  // When: the canonical actions-file boundary drives a real browser terminal.
  const result = await runDriver([
    "--title", "driver contract", "--command", `node '${fixture}'`,
    "--source-label", "xterm-driver-fixture", "--evidence-dir", evidence,
    "--cols", "80", "--rows", "20", "--pre-dwell-ms", "50", "--dwell-ms", "50",
    "--actions-file", actionsPath, "--term", "ansi", "--colorterm", "24bit",
    "--no-color", "1", "--unicode-version", "6",
  ]);

  // Then: every checkpoint owns the four evidence files and cleanup is binary-clean.
  assert.equal(result.code, 0, result.stderr);
  for (const name of ["boot", "interactions"]) {
    for (const file of ["terminal.png", "terminal.txt", "terminal-ansi.txt", "metadata.json"]) {
      assert.ok((await readFile(join(evidence, "checkpoints", name, file))).length > 0, `${name}/${file}`);
    }
  }
  const boot = await json(join(evidence, "checkpoints", "boot", "metadata.json"));
  const interactions = await json(join(evidence, "checkpoints", "interactions", "metadata.json"));
  const final = await json(join(evidence, "metadata.json"));
  const ansi = await readFile(join(evidence, "checkpoints", "interactions", "terminal-ansi.txt"), "utf8");
  const bootText = await readFile(join(evidence, "checkpoints", "boot", "terminal.txt"), "utf8");
  assert.match(bootText, /BOOT TERM=ansi COLORTERM=24bit NO_COLOR=1/);
  assert.equal(boot.capabilities.unicodeVersion, "6");
  assert.deepEqual(boot.dimensions, { cols: 80, rows: 20 });
  assert.deepEqual(interactions.dimensions, { cols: 60, rows: 12 });
  assert.ok(interactions.capturedAtMillis >= boot.capturedAtMillis);
  assert.match(ansi, /INPUT_HEX 1b5b5a/);
  assert.match(ansi, /INPUT_HEX 1b78/);
  assert.match(ansi, /INPUT_HEX 07/);
  assert.match(ansi, /INPUT_HEX 1b5b3c/);
  assert.equal(final.cleanupReceipt.status, "clean");
  assert.deepEqual(final.cleanupReceipt.survivingPids, []);
  const fixtureChildPid = Number(/CHILD_PID=(\d+)/.exec(bootText)[1]);
  assert.ok(final.cleanupReceipt.detectedDescendantPids.includes(fixtureChildPid));
  assert.deepEqual(final.actions, actions);
  for (const file of ["terminal.png", "terminal.txt", "terminal-ansi.txt", "metadata.json", "cleanup.json"]) {
    assert.ok((await readFile(join(evidence, file))).length > 0, `legacy final ${file}`);
  }
});

test("unsafe checkpoint leaf names fail before a command is launched", async (t) => {
  // Given: an action that would escape the checkpoint evidence directory.
  const root = await mkdtemp(join(tmpdir(), "harness-xterm-invalid-"));
  t.after(() => rm(root, { recursive: true, force: true }));

  // When: the action crosses the CLI trust boundary.
  const result = await runDriver([
    "--title", "invalid", "--command", "exit 99", "--evidence-dir", join(root, "evidence"),
    "--action", JSON.stringify({ checkpoint: { name: "../escape" } }),
  ]);

  // Then: validation fails closed before the command's exit status can matter.
  assert.equal(result.code, 1);
  assert.match(result.stderr, /checkpoint name/);
});

test("legacy input still writes one final frame", { timeout: 30_000 }, async (t) => {
  // Given: the original repeatable --input CLI with no table actions.
  const root = await mkdtemp(join(tmpdir(), "harness-xterm-legacy-"));
  const evidence = join(root, "evidence");
  t.after(() => rm(root, { recursive: true, force: true }));

  // When: a legacy Ctrl chord drives the real terminal.
  const result = await runDriver([
    "--title", "legacy", "--command", `node '${fixture}'`, "--source-label", "legacy-fixture",
    "--evidence-dir", evidence, "--cols", "40", "--rows", "10", "--pre-dwell-ms", "50",
    "--dwell-ms", "50", "--key-delay-ms", "1", "--input", "{Ctrl+G}",
  ]);

  // Then: the historical final artifact names and interaction metadata remain intact.
  assert.equal(result.code, 0, result.stderr);
  const metadata = await json(join(evidence, "metadata.json"));
  assert.deepEqual(metadata.interaction, ["{Ctrl+G}"]);
  assert.deepEqual(metadata.actions, []);
  assert.deepEqual(metadata.checkpoints, []);
  assert.equal(metadata.cleanupReceipt.status, "clean");
  assert.match(await readFile(join(evidence, "terminal-ansi.txt"), "utf8"), /INPUT_HEX 07/);
  for (const file of ["terminal.png", "terminal.txt", "terminal-ansi.txt", "metadata.json", "cleanup.json"]) {
    assert.ok((await readFile(join(evidence, file))).length > 0, file);
  }
});

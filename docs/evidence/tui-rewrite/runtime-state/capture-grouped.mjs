#!/usr/bin/env node
// Offline composer journey through the actual PTY and xterm.js renderer.
import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
const root = process.cwd();
const { openBrowserTerminal } = await import(join(root, "scripts/qa/lib/browser-terminal.mjs"));
const { prepareHarnessWorkspace, spawnHarnessPty } = await import(join(root, "scripts/qa/lib/pty-session.mjs"));
const { fileReceipt } = await import(join(root, "scripts/qa/lib/provenance.mjs"));
const { assertSecretFree, validateEvidenceDir } = await import(join(root, "scripts/qa/lib/security.mjs"));

assert(process.argv[2] && process.argv[3], "usage: capture-composer.mjs PROBE_BINARY EVIDENCE_DIR");
const binary = resolve(process.argv[2]);
const output = await validateEvidenceDir(process.argv[3], root);
await mkdir(output, { recursive: true });
const temp = await mkdtemp(join(tmpdir(), "harness-composer-"));
const fixture = await prepareHarnessWorkspace(temp);
const socketPath = join(temp, "events.sock");
const quote = (text) => `'${text.replaceAll("'", "'\\''")}'`;
const report = { schema: "tui-rewrite-composer-v1",
  binary: await fileReceipt(binary, dirname(binary)), inputs: [] };
let terminal, pty, socket, failure;
let sequence = 0;
const event = (type, data) => {
  sequence += 1;
  const value = { delivery: "durable", event: { schema_version: 1,
    event_id: `selection-${sequence}`, seq: sequence, run_id: "selection", mono_ms: sequence,
    ts: null, actor: { kind: "worker", agent_id: "worker" }, correlation_id: "turn",
    causation_id: null, stream_key: null, payload: { event_type: type, data } } };
  report.inputs.push(value);
  socket.write(`${JSON.stringify(value)}\n`);
};
try {
  terminal = await openBrowserTerminal({ cols: 140, rows: 40, browser: "/usr/bin/chromium",
    captureAllCells: true, profilePath: join(temp, "browser"), title: "Composer editing",
    timeoutMs: 15000, onInput: (data) => pty?.write(data) });
  const command = `bash -c ${quote(`before=$(stty -g); ${quote(binary)} ${quote(socketPath)}; status=$?; after=$(stty -g); printf '\\nQA_TERMIOS:%s|%s|%s\\n' "$before" "$after" "$status"; exit "$status"`)}`;
  pty = spawnHarnessPty({ command, cols: 140, rows: 40, cwd: fixture.workspace,
    sessionDir: fixture.sessionDir, tempRoot: temp, disableAnimations: true,
    environment: { TERM: "xterm-256color", COLORTERM: "truecolor", HARNESS_TUI_TEST_WORKSPACE: "1",
      HARNESS_EXPERIMENTAL_DISABLE_COPY_ON_SELECT: "1" }, onOutput: (bytes) => terminal.write(bytes) });
  await terminal.waitForPaintedText("Build, inspect, or fix this workspace.");
  socket = connect(socketPath);
  await once(socket, "connect");
  event("run_started", { run_name: "composer fixture", workspace_root: fixture.workspace });
  const frames = [];
  const screens = new Map();
  async function capture(name, input = "", marker) {
    if (input) { report.inputs.push({ name, input }); assert(pty.write(input)); }
    await delay(150);
    await terminal.waitForStableFrame();
    if (marker) await terminal.waitForPaintedText(marker);
    const screen = await terminal.capture(join(output, `${name}.png`));
    await writeFile(join(output, `${name}.screen.json`), JSON.stringify(screen));
    frames.push({ name, cursor: screen.cursor, activeBuffer: screen.activeBuffer });
    screens.set(name, screen);
  }
  await capture("01-prompt");
  const draft = "plain 界 e\u0301 👩‍💻 ".repeat(32);
  await capture("02-unicode-draft", `\x1b[200~${draft}\x1b[201~`, "plain");
  const countX = (name) => (screens.get(name).text.match(/x/g) ?? []).length;
  const baselineX = countX("02-unicode-draft");
  await capture("03-120-edits", "x".repeat(120));
  assert.equal(countX("03-120-edits"), baselineX + 120);
  await capture("04-110-undos", "\x1a".repeat(110));
  assert.equal(countX("04-110-undos"), baselineX + 10);
  await capture("05-110-redos", "\x1b[122;6u".repeat(110));
  for (const key of ["text", "cells", "cursor"]) {
    assert.deepEqual(screens.get("05-110-redos")[key], screens.get("03-120-edits")[key], `deep redo ${key}`);
  }
  await capture("06-branch", "\x1ay");
  assert.equal(countX("06-branch"), baselineX + 119);
  await capture("07-invalidated-redo", "\x1b[122;6u");
  for (const key of ["text", "cells", "cursor"]) {
    assert.deepEqual(screens.get("07-invalidated-redo")[key], screens.get("06-branch")[key], `redo invalidation ${key}`);
  }
  await capture("08-branch-undo", "\x1a");
  assert.equal(countX("08-branch-undo"), baselineX + 119);
  await capture("09-grouped-delete", "\x7f".repeat(80));
  assert.equal(countX("09-grouped-delete"), baselineX + 39);
  await capture("10-grouped-undo", "\x1a");
  await capture("11-grouped-redo", "\x1b[122;6u");
  for (const [restored, original] of [["10-grouped-undo", "08-branch-undo"], ["11-grouped-redo", "09-grouped-delete"]]) {
    for (const key of ["text", "cells", "cursor"]) {
      assert.deepEqual(screens.get(restored)[key], screens.get(original)[key], `${restored} ${key}`);
    }
  }
  report.frames = frames;
  report.emulator = await terminal.metadata();
  pty.write("\x11\x11");
  report.exit = await pty.waitForExit(15000);
  assert.equal(report.exit.code, 0);
  const restoration = pty.raw().toString("utf8").match(/QA_TERMIOS:([^|\r\n]+)\|([^|\r\n]+)\|(\d+)/);
  report.termios_restored = restoration?.[1] === restoration?.[2] && Boolean(restoration);
  assert(report.termios_restored);
  const final = await terminal.snapshot();
  report.protocol_restored = final.activeBuffer === "normal" && final.cursor.visible
    && !final.modes.bracketedPasteMode && !final.modes.sendFocusMode && final.modes.mouseTrackingMode === "none";
  assert(report.protocol_restored);
} catch (error) {
  failure = error;
  report.failure = String(error.stack ?? error);
  if (terminal) await terminal.capture(join(output, "failure.png"));
} finally {
  socket?.destroy();
  if (pty) {
    report.cleanup = await pty.cleanup();
    assertSecretFree(pty.raw());
    await writeFile(join(output, "terminal.ansi"), pty.raw());
  }
  if (terminal) report.browser_cleanup = await terminal.close();
  await rm(temp, { recursive: true, force: true });
  report.temporary_root_removed = true;
}
assertSecretFree(report);
await writeFile(join(output, "report.json"), JSON.stringify(report, null, 2));
if (failure) throw failure;
console.log(JSON.stringify({ frames: report.frames.length, output }));

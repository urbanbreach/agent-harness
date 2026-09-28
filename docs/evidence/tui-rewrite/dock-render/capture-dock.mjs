#!/usr/bin/env node
// Offline dock-disclosure journey through the actual PTY and xterm.js renderer.
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

assert(process.argv[2] && process.argv[3], "usage: capture-dock.mjs PROBE_BINARY EVIDENCE_DIR");
const binary = resolve(process.argv[2]);
const output = await validateEvidenceDir(process.argv[3], root);
await mkdir(output, { recursive: true });
const temp = await mkdtemp(join(tmpdir(), "harness-runtime-"));
const fixture = await prepareHarnessWorkspace(temp);
const socketPath = join(temp, "events.sock");
const quote = (text) => `'${text.replaceAll("'", "'\\''")}'`;
const report = { schema: "tui-rewrite-dock-render-v1",
  binary: await fileReceipt(binary, dirname(binary)), inputs: [] };
let terminal, pty, socket, failure;
let sequence = 0;
const event = (type, data, correlationId = "turn") => {
  sequence += 1;
  const value = { delivery: "durable", event: { schema_version: 1,
    event_id: `selection-${sequence}`, seq: sequence, run_id: "selection", mono_ms: sequence * 10000,
    ts: null, actor: { kind: "worker", agent_id: "worker" }, correlation_id: correlationId,
    causation_id: null, stream_key: null, payload: { event_type: type, data } } };
  report.inputs.push(value);
  socket.write(`${JSON.stringify(value)}\n`);
};
try {
  terminal = await openBrowserTerminal({ cols: 140, rows: 40, browser: "/usr/bin/chromium",
    captureAllCells: true, profilePath: join(temp, "browser"), title: "Dock disclosure",
    timeoutMs: 15000, onInput: (data) => pty?.write(data) });
  const command = `bash -c ${quote(`before=$(stty -g); ${quote(binary)} ${quote(socketPath)}; status=$?; after=$(stty -g); printf '\\nQA_TERMIOS:%s|%s|%s\\n' "$before" "$after" "$status"; exit "$status"`)}`;
  pty = spawnHarnessPty({ command, cols: 140, rows: 40, cwd: fixture.workspace,
    sessionDir: fixture.sessionDir, tempRoot: temp, disableAnimations: true,
    environment: { TERM: "xterm-256color", COLORTERM: "truecolor", HARNESS_TUI_TEST_WORKSPACE: "1",
      HARNESS_EXPERIMENTAL_DISABLE_COPY_ON_SELECT: "1" }, onOutput: (bytes) => terminal.write(bytes) });
  await terminal.waitForPaintedText("Build, inspect, or fix this workspace.");
  socket = connect(socketPath);
  await once(socket, "connect");
  event("run_started", { run_name: "runtime fixture", workspace_root: fixture.workspace });
  const frames = [];
  const screens = new Map();
  async function capture(name, input = "", marker) {
    if (input) { report.inputs.push({ name, input }); assert(pty.write(input)); }
    if (name !== "09-clear-confirmation") {
      await delay(150);
      await terminal.waitForStableFrame();
    }
    if (marker) await terminal.waitForPaintedText(marker);
    const screen = await terminal.capture(join(output, `${name}.png`));
    if (marker) assert(screen.text.includes(marker), `${name}: marker missing from saved cells`);
    if (name === "10-cleared") {
      const row = screen.lines[screen.cursor.y];
      const start = row.text.indexOf("draft to preserve");
      assert(start >= 0);
      assert.equal(screen.cursor.x, start);
      assert(row.cells.slice(start, start + "draft to preserve".length).every(cell => cell.italic));
      assert(!screen.text.includes("press again to clear"));
    }
    await writeFile(join(output, `${name}.screen.json`), JSON.stringify(screen));
    frames.push({ name, cursor: screen.cursor, activeBuffer: screen.activeBuffer });
    screens.set(name, screen);
  }
  await capture("01-ready");
  event("session_compaction", {agent_id:"worker",summary:"Compacted history",first_kept_event_seq:1,tokens_before:13400,tokens_after:1400,trigger_reason:"fixture"});
  await capture("02-compaction", "", "compactions 1");
  async function resize(name,cols,rows) {
    report.inputs.push({name,resize:{cols,rows}});
    await terminal.resize(cols,rows);await pty.resize(cols,rows);await capture(name);
  }
  await resize("03-width-100",100,24);
  await resize("04-width-80",80,24);
  await resize("05-width-60",60,24);
  await resize("06-width-40",40,24);
  await resize("07-restored",140,40);
  await capture("08-draft", "\x1b[200~draft to preserve\x1b[201~", "draft to preserve");
  await capture("09-clear-confirmation", "\x1b", "press again to clear");
  // Capturing can outlast the 800 ms confirmation. Rearm before the second key.
  await delay(850);
  await terminal.waitForTextAbsent("press again to clear");
  report.inputs.push({name:"rearm-clear-confirmation",input:"\x1b"});
  assert(pty.write("\x1b"));
  await delay(100);
  await capture("10-cleared", "\x1b");
  report.inputs.push({disconnect:true});socket.end();
  await capture("11-disconnected", "", "connection lost");
  await resize("12-disconnected-80",80,24);
  await resize("13-disconnected-40",40,24);
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

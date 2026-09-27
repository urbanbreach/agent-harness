#!/usr/bin/env node
// Offline PTY and xterm capture of selection below a sticky prompt.
import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { openBrowserTerminal } from "./lib/browser-terminal.mjs";
import { prepareHarnessWorkspace, spawnHarnessPty } from "./lib/pty-session.mjs";
import { fileReceipt } from "./lib/provenance.mjs";
import { assertSecretFree, validateEvidenceDir } from "./lib/security.mjs";

assert(process.argv[2] && process.argv[3], "usage: capture-rewrite-selection.mjs PROBE_BINARY EVIDENCE_DIR [--reference]");
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const binary = resolve(process.argv[2]);
const output = await validateEvidenceDir(process.argv[3], root);
const reference = process.argv[4] === "--reference";
await mkdir(output, { recursive: true });
const temp = await mkdtemp(join(tmpdir(), "harness-selection-"));
const fixture = await prepareHarnessWorkspace(temp);
const socketPath = join(temp, "events.sock");
const quote = (text) => `'${text.replaceAll("'", "'\\''")}'`;
const report = { schema: "tui-rewrite-sticky-selection-v1", reference,
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
    captureAllCells: true, profilePath: join(temp, "browser"), title: "Sticky selection",
    timeoutMs: 15000, onInput: (data) => pty?.write(data) });
  const command = `bash -c ${quote(`before=$(stty -g); ${quote(binary)} ${quote(socketPath)}; status=$?; after=$(stty -g); printf '\\nQA_TERMIOS:%s|%s|%s\\n' "$before" "$after" "$status"; exit "$status"`)}`;
  pty = spawnHarnessPty({ command, cols: 140, rows: 40, cwd: fixture.workspace,
    sessionDir: fixture.sessionDir, tempRoot: temp, disableAnimations: true,
    environment: { TERM: "xterm-256color", COLORTERM: "truecolor", HARNESS_TUI_TEST_WORKSPACE: "1",
      HARNESS_EXPERIMENTAL_DISABLE_COPY_ON_SELECT: "1" }, onOutput: (bytes) => terminal.write(bytes) });
  await terminal.waitForPaintedText("Build, inspect, or fix this workspace.");
  socket = connect(socketPath);
  await once(socket, "connect");
  event("run_started", { run_name: "selection fixture", workspace_root: fixture.workspace });
  event("user_message_submitted", { request_id: "turn", text: "Select this" });
  event("provider_request_started", { request_id: "turn", provider_id: "mock", model_id: "fixture",
    prompt_summary: "fixture", request_digest: "fixture", metadata: null });
  const text = Array.from({ length: 70 }, (_, index) => index === 10
    ? "Copy this exact reply" : `Other reply ${String(index).padStart(2, "0")}`).join("\n\n");
  event("assistant_message_finished", { request_id: "turn", tool_call_count: 0,
    parts: [{ kind: "text", text }], provenance: null, assistant_message: null });
  event("provider_request_finished", { request_id: "turn", finish_reason: "stop",
    output_digest: "fixture", usage: null, metadata: null });
  await terminal.waitForPaintedText("Other reply 69");
  report.inputs.push({ page_up: 8 });
  pty.write("\x1b[5~".repeat(8));
  await terminal.waitForPaintedText("Other reply 00");
  report.inputs.push({ wheel_down: 4, column: 10, row: 10 });
  pty.write("\x1b[<65;10;10M".repeat(4));
  await delay(200);
  await terminal.waitForStableFrame();
  await terminal.waitForPaintedText("Copy this exact reply");
  const before = await terminal.capture(join(output, "before.png"));
  assert(before.text.includes("Select this"), "sticky prompt must remain visible");
  assert(!before.text.includes("Other reply 00"), "body must scroll below the pinned prompt");
  const targetRow = before.text.split("\n").findIndex((row) => row.includes("Copy this exact reply"));
  const target = before.cells.find((cell) => cell.row === targetRow && cell.chars === "C");
  assert(target, "missing painted selection target");
  report.target = { column: target.column, row: target.row };
  report.inputs.push({ drag: [target.column + 1, target.row + 1, target.column + 21, target.row + 1] });
  await terminal.mouseCell("down", target.column + 1, target.row + 1);
  await terminal.mouseCell("up", target.column + 21, target.row + 1);
  await delay(200);
  await terminal.waitForStableFrame();
  const selected = await terminal.capture(join(output, "selected.png"));
  const cell = selected.cells.find((cell) => cell.column === target.column && cell.row === target.row);
  report.highlighted = cell?.bgColor !== target.bgColor;
  assert.equal(report.highlighted, !reference, "reference reproduces R7; candidate keeps the painted answer selected");
  await writeFile(join(output, "cells.json"), JSON.stringify({ before, selected }));
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
console.log(JSON.stringify({ highlighted: report.highlighted, output }));

#!/usr/bin/env node
// Offline runtime-state journey through the actual PTY and xterm.js renderer.
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

assert(process.argv[2] && process.argv[3], "usage: capture-runtime.mjs PROBE_BINARY EVIDENCE_DIR");
const binary = resolve(process.argv[2]);
const output = await validateEvidenceDir(process.argv[3], root);
await mkdir(output, { recursive: true });
const temp = await mkdtemp(join(tmpdir(), "harness-runtime-"));
const fixture = await prepareHarnessWorkspace(temp);
const socketPath = join(temp, "events.sock");
const quote = (text) => `'${text.replaceAll("'", "'\\''")}'`;
const report = { schema: "tui-rewrite-runtime-v1",
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
  event("run_started", { run_name: "runtime fixture", workspace_root: fixture.workspace });
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
  await capture("01-ready");
  event("user_message_submitted", { request_id: "turn", text: "Inspect the workspace" });
  event("provider_request_started", { request_id: "turn", provider_id: "mock", model_id: "fixture", prompt_summary: "Inspect the workspace", request_digest: "fixture", metadata: null });
  await capture("02-sending", "", "Inspect the workspace");
  event("provider_stream_delta", { request_id: "turn", delta: "Reading the source file." });
  await capture("03-streaming", "", "Reading the source file.");
  event("tool_call_requested", { tool_call_id: "tool", tool_id: "read", args_summary: '{"path":"src/app.rs"}', args_digest: "fixture", metadata: { canonical_tool_id: "fs.read", alias_source_tool_id: "read" } });
  event("task_scheduled", {task_id: "tool", state: "queued", queue_key: "tool:fs.read", metadata: null});
  event("provider_stream_delta", { request_id: "turn", delta: " Checking the details." });
  await capture("04-tool-queued");
  event("tool_call_started", {tool_call_id: "tool"});
  await capture("05-tool-running");
  event("permission_requested", {permission_id: "permission", kind: "edit_fs", tool_call_id: "tool", summary: "Apply hashline edit to demo.txt", request_digest: "fixture", timeout_ms: 30000, default_decision: "deny"});
  await capture("06-permission", "", "Allow Edit");
  await capture("07-permission-pending", "\x19", "decision sent");
  event("permission_resolved", {permission_id: "permission", decision: "allow", reason: "fixture decision"});
  await terminal.waitForTextAbsent("decision sent");
  await capture("08-permission-resolved");
  event("tool_call_finished", {tool_call_id: "tool", status: "succeeded", output_summary: "Read complete", output_digest: "fixture", output_json: null, metadata: null});
  event("task_completed", {task_id: "tool", result_summary: "Read complete", result_digest: "fixture", metadata: null});
  await capture("09-tool-finished");
  event("assistant_message_finished", {request_id: "turn", tool_call_count: 1, parts: [{kind:"text",text:"The source file is ready for review."}], provenance:null, assistant_message:null});
  event("provider_request_finished", {request_id: "turn", finish_reason: "stop", output_digest: "fixture", usage:null, metadata:null});
  await capture("10-success", "", "The source file is ready for review.");
  event("user_message_submitted", {request_id:"cancel-turn", text:"Cancel this request"}, "cancel-turn");
  event("provider_request_started", {request_id:"cancel-turn", provider_id:"mock", model_id:"fixture", prompt_summary:"Cancel this request", request_digest:"fixture", metadata:null}, "cancel-turn");
  event("task_cancelled", {task_id:"cancel-turn", failure:false, reason:"operator cancelled", task_scope:"agent_turn"}, "cancel-turn");
  await capture("11-cancelled");
  const cancelled = screens.get("11-cancelled").text;
  assert(cancelled.indexOf("Cancel this request") < cancelled.indexOf("Turn cancelled by user"), "cancellation must belong to the new request");
  assert(cancelled.indexOf("The source file is ready for review.") < cancelled.indexOf("Cancel this request"), "the first answer must remain above the new request");
  assert(cancelled.includes("Turn cancelled by user in 20.0s."), "cancellation must use the new request's duration");
  event("user_message_submitted", {request_id:"failed-turn", text:"Fail this request"}, "failed-turn");
  event("provider_request_started", {request_id:"failed-turn", provider_id:"mock", model_id:"fixture", prompt_summary:"Fail this request", request_digest:"fixture", metadata:null}, "failed-turn");
  event("run_failed", {error:"Fixture provider unavailable"}, "failed-turn");
  await capture("12-failure", "", "Fixture provider unavailable");
  const failed = screens.get("12-failure").text;
  assert(failed.indexOf("Fail this request") < failed.indexOf("Fixture provider unavailable"), "failure must belong to the new request");
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

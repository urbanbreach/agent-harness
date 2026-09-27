#!/usr/bin/env node
// Real PTY → xterm.js DOM → browser paint. Offline public events, no provider/network.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { openBrowserTerminal } from "./lib/browser-terminal.mjs";
import { prepareHarnessWorkspace, spawnHarnessPty } from "./lib/pty-session.mjs";
import { fileReceipt } from "./lib/provenance.mjs";
import { assertSecretFree, validateEvidenceDir } from "./lib/security.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
if (!process.argv[2] || !process.argv[3]) throw new Error("Usage: measure-rewrite-latency.mjs PROBE_BINARY EVIDENCE_DIR [SAMPLES>=100]");
const binary = resolve(process.argv[2]);
const output = await validateEvidenceDir(process.argv[3], root);
const count = Number(process.argv[4] ?? 120);
assert(Number.isSafeInteger(count) && count >= 100);
await mkdir(output, { recursive: true });
const temp = await mkdtemp(join(tmpdir(), "harness-xterm-latency-"));
const fixture = await prepareHarnessWorkspace(temp);
const socketPath = join(temp, "events.sock");
const quote = (text) => `'${text.replaceAll("'", "'\\''")}'`;
const report = { schema: "tui-rewrite-browser-latency-v1", count, samples_ms: {},
  boundary: "controller write/TIOCSWINSZ to xterm DOM marker plus two animation frames; includes browser/automation overhead",
  binary: await fileReceipt(binary, dirname(binary)), platform: execFileSync("uname", ["-a"], { encoding: "utf8" }).trim() };
let pty;
let terminal;
let socket;
let failure;
let sequence = 0;
let outputBytes = 0;
const event = (type, data, live = false) => {
  sequence += 1;
  socket.write(`${JSON.stringify({ delivery: live ? "live" : "durable", event: {
    schema_version: 1, event_id: `latency-${sequence}`, seq: sequence, run_id: "latency",
    mono_ms: sequence, ts: null, actor: { kind: "worker", agent_id: "worker" },
    correlation_id: "turn", causation_id: null, stream_key: null,
    payload: { event_type: type, data },
  } })}\n`);
};
async function measure(name, operation) {
  const start = performance.now();
  await operation();
  (report.samples_ms[name] ??= []).push(performance.now() - start);
}
try {
  terminal = await openBrowserTerminal({ cols: 120, rows: 40, browser: "/usr/bin/chromium",
    captureAllCells: false, profilePath: join(temp, "browser"), title: "TUI rewrite latency",
    timeoutMs: 15000, onInput: (data) => pty?.write(data) });
  const script = `before=$(stty -g); ${quote(binary)} ${quote(socketPath)}; probe_status=$?; after=$(stty -g); printf '\\nQA_TERMIOS:%s|%s|%s\\n' "$before" "$after" "$probe_status"; exit "$probe_status"`;
  const command = `bash -c ${quote(script)}`;
  await measure("startup", async () => {
    pty = spawnHarnessPty({ command, cols: 120, rows: 40, cwd: fixture.workspace,
      sessionDir: fixture.sessionDir, tempRoot: temp, disableAnimations: true,
      environment: { TERM: "xterm-256color", COLORTERM: "truecolor" },
      onOutput: (bytes) => { outputBytes += bytes.length; return terminal.write(bytes); } });
    await terminal.waitForPaintedText("Build, inspect, or fix this workspace.");
  });
  socket = connect(socketPath);
  await once(socket, "connect");
  event("run_started", { run_name: "latency fixture", workspace_root: fixture.workspace });
  await terminal.waitForPaintedText("Ctrl+x:shortcuts");
  await terminal.capture(join(output, "startup.png"));
  const idleBytes = outputBytes;
  await delay(1000);
  report.idle_output_bytes = outputBytes - idleBytes;
  for (let index = 0; index < count; index += 1) {
    const text = `typed-${String(index).padStart(3, "0")}`;
    await measure("input", async () => {
      assert(pty.write(text));
      await terminal.waitForPaintedText(text);
    });
    assert(pty.write("\x15"));
    await terminal.waitForTextAbsent(text);
  }
  console.log("Input latency samples complete");
  event("user_message_submitted", { request_id: "turn", text: "Offline stream latency fixture" });
  event("provider_request_started", { request_id: "turn", provider_id: "mock", model_id: "fixture",
    prompt_summary: "fixture", request_digest: "fixture", metadata: null });
  for (let index = 0; index < count; index += 1) {
    const text = `stream-${String(index).padStart(3, "0")}`;
    await measure("stream", async () => {
      event("provider_text_delta", { request_id: "turn", delta: `${text}\n` }, true);
      await terminal.waitForPaintedText(text);
    });
  }
  await terminal.capture(join(output, "stream.png"));
  console.log("Stream latency samples complete");
  for (let index = 0; index < count; index += 1) {
    const cols = index % 2 ? 120 : 80;
    await terminal.resize(cols, 40);
    const before = await terminal.snapshot();
    await measure("resize", async () => {
      await pty.resize(cols, 40);
      await terminal.waitForPaintedText(`stream-${String(count - 1).padStart(3, "0")}`, before.parsedCount);
    });
    const snapshot = await terminal.snapshot();
    assert.equal(snapshot.cols, cols);
    assert(snapshot.cells.some((cell) => cell.column === cols - 3 && ["╮", "╯"].includes(cell.chars)), "resized application border missing");
  }
  await terminal.capture(join(output, "resized.png"));
  report.emulator = await terminal.metadata();
  socket.end();
  await once(socket, "close");
  // Confirmation expires after one second; deliver both keys without observer delay.
  pty.write("\x11\x11");
  report.exit = await pty.waitForExit(15000);
  assert.equal(report.exit.code, 0);
  const restoration = pty.raw().toString("utf8").match(/QA_TERMIOS:([^|\r\n]+)\|([^|\r\n]+)\|(\d+)/);
  assert(restoration, "missing terminal restoration receipt");
  report.termios_restored = restoration[1] === restoration[2];
  assert(report.termios_restored, "terminal flags changed after exit");
  const final = await terminal.snapshot();
  report.protocol_restored = final.activeBuffer === "normal" && final.cursor.visible
    && !final.modes.bracketedPasteMode && !final.modes.sendFocusMode && final.modes.mouseTrackingMode === "none";
  assert(report.protocol_restored, "terminal protocol modes not restored");
} catch (error) {
  failure = error;
  report.failure = String(error.stack ?? error);
} finally {
  socket?.destroy();
  if (pty) {
    report.cleanup = await pty.cleanup();
    const raw = pty.raw();
    assertSecretFree(raw);
    await writeFile(join(output, "terminal.ansi"), raw);
  }
  if (terminal) report.browser_cleanup = await terminal.close();
  await rm(temp, { recursive: true, force: true });
  report.temporary_root_removed = true;
}
report.output_bytes = outputBytes;
report.summary = Object.fromEntries(Object.entries(report.samples_ms).map(([name, values]) => {
  const sorted = values.toSorted((a, b) => a - b);
  return [name, Object.fromEntries([50, 95, 99].map((p) => [`p${p}_ms`, sorted[Math.ceil(sorted.length * p / 100) - 1]]))];
}));
assertSecretFree(report);
await writeFile(join(output, "latency.json"), JSON.stringify(report, null, 2));
if (failure) throw failure;
console.log(JSON.stringify({ summary: report.summary, cleanup: report.cleanup, output }));

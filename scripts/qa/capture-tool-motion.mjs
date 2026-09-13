#!/usr/bin/env node
// Synthetic typed updates through run_tui_with_options, a real PTY, and xterm.js.
import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, open, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { openBrowserTerminal } from "./lib/browser-terminal.mjs";
import { prepareHarnessWorkspace, spawnHarnessPty } from "./lib/pty-session.mjs";
import { currentTree, fileReceipt } from "./lib/provenance.mjs";
import { assertSecretFree, safePtyEnvironment, validateEvidenceDir } from "./lib/security.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const output = await validateEvidenceDir(process.argv[2], root);
const directTool = ["--reasoning-to-tool", "--reasoning-to-read"].includes(process.argv[3]);
const readTools = process.argv[3] === "--reasoning-to-read";
const reasoning = directTool || ["--reasoning", "--reasoning-first-only"].includes(process.argv[3]);
const reasoningAfterTools = process.argv[3] === "--reasoning";
const completionLabel = readTools ? "Both reads completed." : "Both commands completed.";
const initialRows = Number(process.argv[4] ?? 40);
if (!Number.isInteger(initialRows) || initialRows < 20) throw new Error("initial rows must be at least 20");
await mkdir(join(output, "frames"), { recursive: true });
const source = await currentTree(root);
const buildArgs = ["nextest", "list", "--all-features", "-p", "harness-tui", "--test",
  "manual_live_turn_visual_capture_test", "--message-format", "json", "--list-type", "binaries-only"];
const listing = JSON.parse(execFileSync("cargo", buildArgs, {
  cwd: root, env: safePtyEnvironment(process.env), encoding: "utf8", maxBuffer: 16 * 1024 * 1024,
}));
const binary = Object.values(listing["rust-binaries"])
  .find(item => item["binary-name"] === "manual_live_turn_visual_capture_test")?.["binary-path"];
if (!binary) throw new Error("nextest did not report the capture executable");
const executable = await fileReceipt(binary, root);
const fixture = JSON.parse(await readFile(join(root, "scripts/qa/fixtures/tool-order-scenarios.json"), "utf8"));
if (readTools) {
  fixture.tools["command-a"] = fixture.tools.a;
  fixture.tools["command-b"] = fixture.tools.b;
}
const sequence = [];
let seq = 0;
let liveSeq = 0;
function event(at, kind, data, delivery = "durable") {
  const envelope = {
    event_id: delivery === "durable" ? `motion-${++seq}` : `motion-live-${++liveSeq}`,
    run_id: "synthetic-tool-motion", mono_ms: at, actor: { kind: "worker", agent_id: "fixture" },
    ...(reasoning ? { ts: "2026-09-13T00:43:00Z" } : {}),
    correlation_id: "turn", payload: { event_type: kind, data },
  };
  if (delivery === "durable") Object.assign(envelope, { schema_version: 1, seq });
  sequence.push([at, { delivery, event: envelope }]);
}
function startProvider(at, request) {
  event(at, "provider_request_started", { request_id: request, provider_id: "mock", model_id: "model",
    prompt_summary: "Inspect the tool rows", request_digest: "synthetic" });
}
function finishProvider(at, request, parts) {
  event(at, "provider_request_finished", { request_id: request, finish_reason: "stop" });
  event(at + 300, "assistant_message_finished", { request_id: request, parts,
    tool_call_count: parts.filter(part => part.kind === "tool_call").length });
}
event(800, "user_message_submitted", { request_id: "turn", text: "Inspect the tool rows" });
event(800, "task_scheduled", { task_id: "turn-task", state: "started", queue_key: "provider_model:mock:model" });
startProvider(800, "provider");
const headers = ["**Planning the inspection**", "\n\n**Checking the commands**", "\n\n**Verifying the result**"];
if (reasoning) for (const [index, delta] of headers.entries()) {
  event(950 + index * 150, "provider_reasoning_delta", { request_id: "provider", delta }, "live");
}
if (!directTool) event(1400, "provider_text_delta", { request_id: "provider", delta: "Inspect first." }, "live");
event(1750, "provider_tool_input_delta", { request_id: "provider", tool_call_id: "command-a", delta: "{" }, "live");
for (const [id, at] of [["command-a", 2100], ["command-b", 2750]]) {
  const tool = fixture.tools[id];
  event(at, "tool_call_requested", { tool_call_id: id, tool_id: tool.tool,
    args_summary: JSON.stringify(tool.args), args_digest: "synthetic" });
  event(at + 150, "task_scheduled", { task_id: `task-${id}`, state: "started", metadata: {
    lineage: { parent_tool_call_id: id },
  } });
  event(at + 150, "tool_call_started", { tool_call_id: id, task_id: `task-${id}` });
}
finishProvider(3300, "provider", [
  ...(reasoning ? [{ kind: "reasoning", text: headers.join("") }] : []),
  ...(!directTool ? [{ kind: "text", text: "Inspect first." }] : []),
  ...["command-a", "command-b"].map(id => ({ kind: "tool_call", tool_call_id: id,
    tool_id: fixture.tools[id].tool, args_summary: JSON.stringify(fixture.tools[id].args), args_digest: "synthetic" })),
]);
for (const [id, at] of [["command-a", 4800], ["command-b", 5100]]) {
  const tool = fixture.tools[id];
  event(at, "tool_call_finished", { tool_call_id: id, status: "succeeded",
    output_summary: tool.output, output_digest: "synthetic", output_json: readTools
      ? { metadata: { display: { text: tool.output, lineStart: 1 } } }
      : { stdout: tool.output, stderr: "", exit_code: 0 } });
  event(at, "task_completed", { task_id: `task-${id}`, result_summary: "Complete", result_digest: "synthetic",
    metadata: { task_scope: "tool_call", lineage: { parent_tool_call_id: id } } });
}
startProvider(5650, "provider-next");
if (reasoningAfterTools) for (const [index, delta] of headers.entries()) {
  event(5700 + index * 75, "provider_reasoning_delta", { request_id: "provider-next", delta }, "live");
}
const chunks = reasoning ? [`${completionLabel}\n\n`, "- Inspected the first result\n- Inspected the second result\n- Verified the output\n", "- Checked the workspace\n- Confirmed no residue remains\n\nNo other files were modified."] : [`${completionLabel}\n\nThe tool rows stayed in place.`];
const answer = chunks.join("");
for (const [index, delta] of chunks.entries()) {
  event(5950 + index * 150, "provider_text_delta", { request_id: "provider-next", delta }, "live");
}
finishProvider(6500, "provider-next", [
  ...(reasoningAfterTools ? [{ kind: "reasoning", text: headers.join("") }] : []),
  { kind: "text", text: answer },
]);
event(7100, "task_completed", { task_id: "turn-task", result_summary: "Complete", result_digest: "synthetic",
  metadata: { task_scope: "agent_turn", outcome: "completed" } });
sequence.sort((a, b) => a[0] - b[0]);
// Durable sequence numbers follow delivery order, including interleaved tools.
let durableSeq = 0;
for (const [, update] of sequence) if (update.delivery === "durable") update.event.seq = ++durableSeq;
assertSecretFree(sequence);
const sequencePath = join(output, "synthetic-events.json");
await writeFile(sequencePath, JSON.stringify(sequence, null, 2));
const tempRoot = await mkdtemp(join(tmpdir(), "harness-xterm-motion-"));
const workspace = await prepareHarnessWorkspace(tempRoot);
const streamPath = join(tempRoot, "synthetic-events.fifo");
execFileSync("mkfifo", [streamPath]);
const frames = [];
const failures = [];
const anchors = new Map();
const colors = new Set();
const statusSamples = new Map();
const statusWindows = [
  [1025, 1300, "first-model-phase", reasoning ? "Thinking…" : "Waiting for response…"],
  [1850, 2030, "tool-arguments", "Preparing tool call…"],
  [3700, 4500, "running-tool", `Run ${readTools ? "read" : "bash"}`],
  [5300, 5550, "after-tools", "Waiting for response…"],
  [5830, 5900, "next-model-phase", reasoningAfterTools ? "Thinking…" : "Waiting for response…"],
  [6250, 6450, "answer", "Responding…"],
  [6650, 6950, "after-answer", "Waiting for response…"],
];
let terminal;
let pty;
let exitResult;
let recording;
let streamWorker;
try {
  terminal = await openBrowserTerminal({ cols: 120, rows: initialRows, browser: "/usr/bin/chromium",
    profilePath: join(tempRoot, "browser"), title: "Harness tool motion", timeoutMs: 15000,
    videoDir: join(output, "browser-video"),
    captureAllCells: true, onInput: data => pty?.write(data),
  });
  recording = await terminal.recordingInfo();
  const quote = value => `'${String(value).replaceAll("'", "'\\''")}'`;
  pty = spawnHarnessPty({ cols: 120, rows: initialRows, tempRoot, ...workspace, cwd: workspace.workspace,
    command: `${quote(binary)} --exact capture_live_turn_state_from_environment --nocapture`,
    disableAnimations: false,
    environment: { TERM: "xterm-256color", COLORTERM: "truecolor", HARNESS_TUI_MANUAL_SCENARIO: "streamed_tools",
      HARNESS_TUI_MANUAL_EVENT_STREAM: streamPath },
    onOutput: bytes => terminal.write(bytes),
  });
  const started = performance.now();
  streamWorker = (async () => {
    const stream = await open(streamPath, "w");
    try {
      for (const [at, update] of sequence) {
        await new Promise(accept => setTimeout(accept, Math.max(0, at - (performance.now() - started))));
        await stream.write(`${JSON.stringify(update)}\n`);
      }
    } finally {
      await stream.close();
    }
  })();
  while (performance.now() - started < 9000) {
    const name = `frame-${String(frames.length).padStart(4, "0")}`;
    const snapshot = await terminal.motionSample();
    const at = Math.round(performance.now() - started);
    const rows = snapshot.text.split("\n");
    const footer = rows.findLast(row => row.includes("[stop]")) ?? "";
    for (const [from, until, stage, label] of statusWindows) {
      if (at < from || at > until) continue;
      if (!footer.includes(label)) failures.push(`footer at ${at}ms (${stage}): expected ${label}, got ${footer.trim()}`);
      if (!statusSamples.has(stage)) {
        statusSamples.set(stage, { at_ms: at, expected: label, footer: footer.trim() });
      }
    }
    const labels = initialRows < 40 ? (at > 6350 ? [completionLabel, "No other files were modified."] : [])
      : [...(!directTool ? ["Inspect first."] : []), ...(!readTools ? ["printf alpha", "printf beta"] : []), ...(reasoning ? [completionLabel] : [])];
    for (const label of labels) {
      const row = rows.findIndex(line => line.includes(label));
      if (row >= 0) {
        if (!anchors.has(label)) anchors.set(label, row);
        if (row !== anchors.get(label)) failures.push(`${label} moved at ${at}ms: ${row}`);
      } else if (anchors.has(label)) failures.push(`${label} disappeared at ${at}ms`);
    }
    if (rows.some(row => /^\s*◆ tool\s*$/.test(row))) failures.push(`argument placeholder at ${at}ms`);
    if (directTool && at > 2300 && at < 5500 && rows.some(row => row.includes("Thought for")) === readTools) {
      failures.push(`wrong reasoning-to-tool fold at ${at}ms`);
    }
    const markers = (readTools ? ["Reading 2 files"] : ["printf alpha", "printf beta"]).map(label => {
      const row = rows.findIndex(row => row.includes(label));
      return snapshot.cells.find(cell => cell.row === row && (cell.chars === "◆" || cell.chars === "◈"));
    });
    if (at > 3600 && at < 4700 && markers.every(Boolean)) {
      if (!markers.every(marker => marker.fgColor === markers[0].fgColor)) failures.push(`tool waves diverged at ${at}ms`);
      colors.add(markers[0].fgColor);
    }
    frames.push({ name, at_ms: at, text: snapshot.text, markers });
    await new Promise(accept => setTimeout(accept, 33));
  }
  const expectedAnchors = initialRows < 40 ? 2 : readTools ? 1 : directTool ? 3 : reasoning ? 4 : 3;
  if (anchors.size !== expectedAnchors) failures.push(`missing text or tool anchors: ${anchors.size}/${expectedAnchors}`);
  if (colors.size < 3) failures.push(`insufficient animation samples: ${colors.size}`);
  for (const [, , stage] of statusWindows) {
    if (!statusSamples.has(stage)) failures.push(`missing footer samples: ${stage}`);
  }
  await terminal.capture(join(output, "completed.png"));
  if (initialRows < 40) {
    await terminal.resize(120, 40);
    await pty.resize(120, 40);
    await new Promise(accept => setTimeout(accept, 250));
  }
  if (directTool) {
    await terminal.clickText(readTools ? "Read 2 files" : "Thought for");
    await terminal.waitForStableFrame();
    if ((await terminal.snapshot()).text.includes("Planning the inspection")) {
      failures.push("single click expanded the thought instead of selecting it");
    }
    await terminal.key("Control+e");
    if (readTools) {
      await terminal.waitForText("Thought for");
      const revealed = await terminal.capture(join(output, "thought-revealed.png"));
      if (!revealed.text.includes("Thought for")) failures.push("group expansion lost its Thought member");
      await terminal.key("Control+e");
    }
    await terminal.waitForText("Planning the inspection");
    const thought = await terminal.capture(join(output, "thought-open.png"));
    for (const header of headers) {
      if (!thought.text.includes(header.replaceAll("*", "").trim())) failures.push(`lost reasoning header ${header}`);
    }
    await terminal.key("Control+e");
    await terminal.waitForTextAbsent("Planning the inspection");
  }
  const toolLabel = readTools ? "Read alpha.txt" : "printf alpha";
  await terminal.clickText(toolLabel);
  await terminal.waitForStableFrame();
  await terminal.key("Control+e");
  if (readTools) await terminal.waitForText("alpha first line");
  await terminal.capture(join(output, "expanded-command.png"));
  await terminal.key("Enter");
  await terminal.waitForText("Enter:quote");
  const viewer = await terminal.capture(join(output, "full-output.png"));
  if (!viewer.text.includes(readTools ? "alpha first line" : "printf alpha")) failures.push("full output lost the tool content");
  await terminal.key("Escape");
  for (const [cols, rows] of [[40, 30], [80, 34], [120, 40]]) {
    await terminal.resize(cols, rows);
    await pty.resize(cols, rows);
    await new Promise(accept => setTimeout(accept, 250));
    const snapshot = await terminal.capture(join(output, `resize-${cols}x${rows}.png`));
    await writeFile(join(output, `resize-${cols}x${rows}.json`), JSON.stringify(snapshot));
  }
  // Keep both quit keys inside the runtime's confirmation window; full browser snapshots are slower.
  pty.write("\x11\x11");
  exitResult = await pty.waitForExit(10000);
  if (exitResult.code !== 0) failures.push(`PTY exit ${exitResult.code}`);
  assertSecretFree(pty.raw());
  await writeFile(join(output, "terminal.ansi"), pty.raw());
  await writeFile(join(output, "pty-actions.json"), JSON.stringify(pty.actions, null, 2));
} finally {
  if (pty) await pty.cleanup();
  if (streamWorker) await streamWorker;
  if (terminal) await terminal.close();
  await rm(tempRoot, { recursive: true, force: true });
  await writeFile(join(output, "frames.json"), JSON.stringify(frames, null, 2));
}
const { x, y, width, height } = recording.screen;
execFileSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y",
  "-i", recording.path, "-vf", `crop=${Math.floor(width / 2) * 2}:${Math.floor(height / 2) * 2}:${Math.round(x)}:${Math.round(y)}`, "-c:v", "libx264", "-crf", "18",
  "-pix_fmt", "yuv420p", "-movflags", "+faststart", join(output, "tool-motion.mp4")]);
const after = await currentTree(root);
if (source.hash !== after.hash) failures.push("source changed during capture");
const executableAfter = await fileReceipt(binary, root);
if (executable.sha256 !== executableAfter.sha256) failures.push("executable changed during capture");
await writeFile(join(output, "manifest.json"), JSON.stringify({ source, after, buildArgs, executable,
  scenario: directTool ? (readTools ? "reasoning-directly-to-read" : "reasoning-directly-to-command")
    : reasoningAfterTools ? "reasoning-headers-each-response" : reasoning ? "reasoning-headers-first-response" : "plain-text",
  initialRows,
  executableAfter, exitResult, recording, frames: frames.length, waveColors: [...colors], anchors: [...anchors],
  statusSamples: Object.fromEntries(statusSamples), failures,
  provenance: "Synthetic typed runtime updates, real run_tui_with_options event loop and PTY, Chromium + xterm.js 6.0.0",
}, null, 2));
if (failures.length) throw new Error(failures.join("\n"));
process.stdout.write(`PASS ${frames.length} real-runtime frames, ${colors.size} wave colors: ${output}\n`);

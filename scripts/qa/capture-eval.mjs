#!/usr/bin/env node
// Actual coordinator + eval kernel events, played through the native TUI in xterm.js.
import { execFileSync } from "node:child_process";
import { cp, mkdir, mkdtemp, open, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { openBrowserTerminal } from "./lib/browser-terminal.mjs";
import { prepareHarnessWorkspace, spawnHarnessPty } from "./lib/pty-session.mjs";
import { currentTree, fileReceipt } from "./lib/provenance.mjs";
import { assertSecretFree, safePtyEnvironment, validateEvidenceDir } from "./lib/security.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const output = await validateEvidenceDir(process.argv[2], root);
await mkdir(output, { recursive: true });
const eventsPath = join(output, "runtime-events.json");
if (!process.argv.includes("--reuse-events")) {
  execFileSync("cargo", ["nextest", "run", "--profile", "ci", "-p", "harness-tools", "--test", "eval",
    "--run-ignored", "all", "-E", "test(capture_eval)"], { cwd: root, stdio: "inherit",
    env: safePtyEnvironment(process.env, { HARNESS_EVAL_SIGNOFF: "1", HARNESS_EVAL_CAPTURE: eventsPath }) });
}
const events = JSON.parse(await readFile(eventsPath, "utf8"));
assertSecretFree(events);
const source = await currentTree(root);
const listing = JSON.parse(execFileSync("cargo", ["nextest", "list", "--all-features", "-p", "harness-tui",
  "--test", "manual_live_turn_visual_capture_test", "--message-format", "json", "--list-type", "binaries-only"],
{ cwd: root, encoding: "utf8", maxBuffer: 16 * 1024 * 1024, env: safePtyEnvironment(process.env) }));
const binary = Object.values(listing["rust-binaries"]).find(b => b["binary-name"] === "manual_live_turn_visual_capture_test")?.["binary-path"];
if (!binary) throw new Error("capture binary missing");
const executable = await fileReceipt(binary, root);
const tempRoot = await mkdtemp(join(tmpdir(), "harness-xterm-eval-"));
const workspace = await prepareHarnessWorkspace(tempRoot);
const runDir = join(workspace.sessionDir, events[0].event.run_id);
await mkdir(runDir);
await cp(join(output, "run-artifacts"), join(runDir, "artifacts"), {recursive:true});
const fifo = join(tempRoot, "events.fifo");
execFileSync("mkfifo", [fifo]);
const quote = value => `'${value.replaceAll("'", "'\\''")}'`;
let terminal, pty, stream, cursor = 0;
const captures = [];
const failures = [];
const payload = entry => entry.event.payload;
const find = predicate => {
  const index = events.findIndex(predicate);
  if (index < 0) throw new Error("required runtime event missing");
  return index;
};
async function through(index) {
  while (cursor <= index) await stream.write(JSON.stringify(events[cursor++]) + "\n");
}
async function capture(name, expected) {
  await terminal.waitForText(expected);
  const snapshot = await terminal.capture(join(output, name + ".png"));
  await writeFile(join(output, name + ".json"), JSON.stringify(snapshot));
  captures.push({ name, cols: snapshot.cols, rows: snapshot.rows, expected, events: cursor });
  return snapshot;
}
try {
  terminal = await openBrowserTerminal({ cols: 120, rows: 40, browser: "/usr/bin/chromium",
    profilePath: join(tempRoot, "browser"), title: "Harness eval", timeoutMs: 15000,
    captureAllCells: true, onInput: data => pty?.write(data) });
  pty = spawnHarnessPty({ cols: 120, rows: 40, tempRoot, ...workspace, cwd: workspace.workspace,
    command: `${quote(binary)} --exact capture_live_turn_state_from_environment --nocapture`,
    disableAnimations: false, environment: { TERM: "xterm-256color", COLORTERM: "truecolor",
      HARNESS_TUI_MANUAL_SCENARIO: "streamed_tools", HARNESS_TUI_MANUAL_EVENT_STREAM: fifo,
      HARNESS_TUI_MANUAL_RUN_DIR: runDir },
    onOutput: bytes => terminal.write(bytes) });
  stream = await open(fifo, "w");
  await through(find(e => payload(e).event_type === "eval_progress" && payload(e).data.output?.includes("Reading 3 files")));
  await capture("running-120", "Compare the three workspace notes");
  const secondPrompt = find(e => payload(e).event_type === "user_message_submitted" && payload(e).data.text === "Validate the report data.");
  await through(secondPrompt - 1);
  const completed = await capture("completed-120", "All three notes are ready.");
  if (!completed.text.includes("←")) failures.push("eval input gutter missing");
  await terminal.clickText("Compare the three workspace notes");
  await terminal.key("Control+e");
  await terminal.key("Home");
  const expanded = await capture("expanded-120", "Compare the three workspace notes");
  if (!expanded.text.includes("→")) failures.push("expanded eval output gutter missing");
  await terminal.key("Enter");
  await capture("viewer-120", "Enter:quote");
  await terminal.key("Escape");
  await terminal.key("Control+e");
  for (const [cols, rows] of [[80, 34], [40, 30], [120, 40]]) {
    await terminal.resize(cols, rows);
    await pty.resize(cols, rows);
    await terminal.key("Home");
    await capture(`completed-${cols}x${rows}`, "Compare");
  }
  const spillPrompt = find(e => payload(e).event_type === "user_message_submitted" && payload(e).data.text === "Inspect a large output.");
  await through(spillPrompt - 1);
  await capture("error-120", "invalid JSON");
  const backgroundPrompt = find(e => payload(e).event_type === "user_message_submitted" && payload(e).data.text === "Build a summary in the background.");
  await through(backgroundPrompt - 1);
  await terminal.key("End");
  await capture("spill-120", "Output truncated");
  await terminal.clickText("Inspect the large output");
  await terminal.key("Enter");
  await capture("spill-viewer-120", "Enter:quote");
  await terminal.key("Escape");
  const settled = find(e => payload(e).event_type === "eval_cell_finished");
  await through(find(e => payload(e).event_type === "assistant_message_finished" && JSON.stringify(payload(e).data).includes("The summary is running in the background.")));
  await terminal.key("End");
  await capture("detached-120", "running in the background");
  if (!(await terminal.snapshot()).text.includes("detached")) failures.push("detached state is not visible");
  await through(settled);
  await capture("settled-120", "complete");
  await through(events.length - 1);
  await stream.close(); stream = null;
  pty.write("\x11\x11");
  const exit = await pty.waitForExit(10000);
  if (exit.code !== 0) failures.push(`PTY exit ${exit.code}`);
  assertSecretFree(pty.raw());
  await writeFile(join(output, "terminal.ansi"), pty.raw());
  await writeFile(join(output, "pty-actions.json"), JSON.stringify(pty.actions, null, 2));
} catch (error) {
  failures.push(String(error));
  if (terminal) {
    const snapshot = await terminal.capture(join(output, "failure.png"));
    await writeFile(join(output, "failure.json"), JSON.stringify(snapshot));
  }
  throw error;
} finally {
  await stream?.close();
  const cleanup = await pty?.cleanup();
  await terminal?.close();
  await rm(tempRoot, { recursive: true, force: true });
  await writeFile(join(output, "manifest.json"), JSON.stringify({
    provenance: "Real coordinator and JS/Python kernels with a scripted provider; exact runtime events replayed through run_tui_with_options, a real PTY, Chromium and xterm.js 6.",
    source, executable, runtimeEvents: await fileReceipt(eventsPath, root), captures, cleanup, failures,
  }, null, 2));
}
if (failures.length) throw new Error(failures.join("\n"));
process.stdout.write(`PASS eval xterm capture: ${output}\n`);

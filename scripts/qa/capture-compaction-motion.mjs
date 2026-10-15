#!/usr/bin/env node
// Typed maintenance events through the capture runtime, a real PTY and xterm.js.
import { execFileSync } from "node:child_process";
import { lstat, mkdir, mkdtemp, open, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { openBrowserTerminal } from "./lib/browser-terminal.mjs";
import { prepareHarnessWorkspace, spawnHarnessPty } from "./lib/pty-session.mjs";
import { assertSecretFree, safePtyEnvironment, validateEvidenceDir } from "./lib/security.mjs";

import { captureCoordinatorCompaction } from "./lib/compaction-live-capture.mjs";
import { captureSurfaces, resolveCaptureBinary } from "./lib/compaction-surfaces.mjs";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const destination = resolve(process.argv[2] ?? join(root, "docs/evidence/compaction-motion"));
let output;
if (destination === join(root, "docs/evidence/compaction-motion")) {
  for (let path = destination; path !== root; path = dirname(path)) {
    try { if ((await lstat(path)).isSymbolicLink()) throw new Error(`symlink evidence path: ${path}`); }
    catch (error) { if (error.code !== "ENOENT") throw error; }
  }
  output = destination;
} else { output = await validateEvidenceDir(destination, root); }
try { if ((await lstat(join(output,"frames"))).isSymbolicLink()) throw new Error("symlink frame directory"); } catch(error) { if(error.code !== "ENOENT") throw error; }
await rm(join(output,"frames"),{recursive:true,force:true});
await mkdir(join(output, "frames"), { recursive: true });
const target = process.env.CARGO_TARGET_DIR ?? "/home/urbanbreach/.cache/harness-targets/Captures";
const binary = resolveCaptureBinary(root, target);
const sequence = [];
const samples = [];
let seq = 0;
let liveSeq = 0;
function event(at, kind, data, delivery = "live") {
  const envelope = { event_id: `maintenance-${delivery}-${delivery === "live" ? ++liveSeq : ++seq}`, run_id: "synthetic-maintenance", mono_ms: at, actor: { kind: "worker", agent_id: "fixture" }, correlation_id: "turn", payload: { event_type: kind, data } };
  if (delivery === "durable") Object.assign(envelope, { schema_version: 1, seq });
  sequence.push([at, { delivery, event: envelope }]);
}
event(200, "user_message_submitted", { request_id: "turn", text: "Preserve the implementation decisions while reducing context." }, "durable");
event(200, "provider_request_started", { request_id: "provider", provider_id: "mock", model_id: "model", prompt_summary: "Maintenance evidence", request_digest: "synthetic" }, "durable");
event(300, "provider_request_finished", { request_id: "provider", finish_reason: "stop" }, "durable");
event(350, "assistant_message_finished", { request_id: "provider", parts: [{ kind: "text", text: "Earlier decisions remain readable in the archived transcript." }], tool_call_count: 0 }, "durable");
let at = 900;
let generation = 0;
const phases = { soft: [["summarizing", "distilling"], ["turn_prefix", "retaining turn context"], ["short_summary", "writing short summary"]], remote: [["remote", "server compaction"]], archive: [["archiving", "printing frames"]], handoff: [["handoff", "writing handoff"]], shake: [["shaking", "shaking blocks"]] };
const scenarios = [...Object.entries(phases).map(([method, phases]) => [method, [["preparing", "preparing"], ...phases, ["committing", "committing"], ["done", "Context maintenance"]]]), ...["failed", "cancelled", "skipped"].map(phase => ["soft", [[phase, phase]]])];
for (const [method, methodPhases] of scenarios) {
  generation += 1;
  for (const [phase, expected] of methodPhases) {
    const stage = { reason: "manual", method, phase, tokens_before: 182400, tokens_after: phase === "done" ? 31050 : null, summarized_tokens: 151350, kept_tokens: 20000, fixed_tokens: 8000, threshold_tokens: 220000, window_tokens: 262144, first_kept_seq: method === "shake" ? null : 100, windows_done: phase === "preparing" ? 0 : 2, windows_total: 3, frames_done: method === "archive" && phase !== "preparing" ? 3 : 0, frames_total: method === "archive" ? 3 : 0, thumbnails: method === "archive" && phase !== "preparing" ? [0, 1, 2].map(n => ({ width: 16, height: 4, bits: Buffer.from([255, 255, 170 + n, 85, 255, 0, 85, 170]).toString("base64") })) : [], message: null };
    event(at, "compaction_progress", { agent_id: "fixture", generation, trigger_reason: "manual", preview: "Keep the API contract and next steps.", stage });
    if (phase === "archiving") {
      const frames = stage.thumbnails;
      stage.thumbnails = frames.slice(0,1); stage.frames_done = 1;
      for (const [delay,count] of [[350,2],[650,3]]) event(at + delay,"compaction_progress", {agent_id:"fixture",generation,trigger_reason:"manual",preview:"Printing archive frames.",stage:{...stage,frames_done:count,thumbnails:frames.slice(0,count)}});
    }
    if (["done", "failed", "cancelled", "skipped"].includes(phase)) event(at + 30, "compaction_progress", { agent_id: "fixture", generation, trigger_reason: "manual", preview: null, stage: null });
    if (phase === "done" && method === "shake") event(at,"context_pruned",{agent_id:"fixture",kind:"shake",edits:[{kind:"text_range",seq:4,start:0,end:7,text:"[pruned]"}],tokens_saved:151350,artifact_path:null,warning:null},"durable");
    else if (phase === "done") event(at, "session_compaction", { agent_id: "fixture", summary: "## Retained decisions\n\nPreserve the API contract and continue the scoped checks.", first_kept_event_seq: 100, first_kept_request_id: null, first_kept_entry_id: null, tokens_before: 182400, tokens_after: 31050, summary_usage: null, summary_provider_id: null, summary_model_id: null, read_files: ["src/context.rs"], modified_files: ["src/session.rs"], task_intent: null, current_intent: null, trigger_reason: "manual", from_hook: false, short_summary: "Preserve contracts", method, warning: null, provider_replay_through_seq: null, native: null, archive: null }, "durable");
    for (const offset of [200, 500, 850]) samples.push({ at: at + offset, method, phase, expected });
    if (phase === "done") samples.push({at:at+1000,method,phase:"settled",expected:{soft:"soft-compacted",remote:"remote-compacted",archive:"archived",handoff:"handed-off",shake:"shaken"}[method]});
    at += 1100;
  }
}
sequence.sort((left,right) => left[0] - right[0] || Number(left[1].delivery !== "durable") - Number(right[1].delivery !== "durable"));
assertSecretFree(sequence);
await writeFile(join(output, "synthetic-events.json"), JSON.stringify(sequence, null, 2));
const manifest = { cols: 120, rows: 36, sequence: "synthetic-events.json", frames: [], failures: [] };
for (const reduced of [false, true]) {
  const tempRoot = await mkdtemp(join(tmpdir(), "harness-maintenance-"));
  const workspace = await prepareHarnessWorkspace(tempRoot);
  const fifo = join(tempRoot, "events.fifo");
  execFileSync("mkfifo", [fifo]);
  let terminal;
  let pty;
  let stream;
  try {
    terminal = await openBrowserTerminal({ cols: 120, rows: 36, browser: "/usr/bin/chromium", profilePath: join(tempRoot, "browser"), title: "Harness compaction motion", captureAllCells: true, onInput: data => pty?.write(data) });
    const quote = value => `'${String(value).replaceAll("'", "'\\''")}'`;
    pty = spawnHarnessPty({ cols: 120, rows: 36, tempRoot, ...workspace, cwd: workspace.workspace, command: `${quote(binary)} --exact capture_compaction_state_from_environment --nocapture >/dev/null`, disableAnimations: reduced, environment: { TERM: "xterm-256color", COLORTERM: "truecolor", HARNESS_TUI_COMPACTION_STREAM: fifo }, onOutput: bytes => terminal.write(bytes) });
    const started = performance.now();
    stream = await open(fifo,"w");
    let nextUpdate = 0;
    for (const sample of samples) {
      if (reduced && ["done", "failed", "cancelled", "skipped"].includes(sample.phase)) continue;
      if (reduced && sample.at % 1100 !== 0) {
        // One still per phase is enough to prove the reduced-motion alternative.
        if (samples.find(s => s.method === sample.method && s.phase === sample.phase) !== sample) continue;
      }
      while (nextUpdate < sequence.length && sequence[nextUpdate][0] <= sample.at) {
        const [at_ms,event] = sequence[nextUpdate++];
        await stream.write(`${JSON.stringify({at_ms,event})}\n`);
      }
      const frame_id = manifest.frames.length;
      await stream.write(`${JSON.stringify({at_ms:sample.at,frame_id})}\n`);
      await terminal.waitForTitle(`Harness compaction frame ${frame_id}`);
      const file = `frames/${reduced ? "reduced" : "motion"}-${String(manifest.frames.length).padStart(4, "0")}-${sample.method}-${sample.phase}.png`;
      const snapshot = await terminal.capture(join(output, file));
      const expected = sample.phase === "archiving" ? `${sample.at % 1100 === 0 ? 1 : sample.at % 1100 === 300 ? 2 : 3}/3` : sample.expected;
      if (!snapshot.text.includes(expected)) manifest.failures.push(`${file}: missing ${expected}`);
      manifest.frames.push({ file, at_ms: sample.at, captured_after_ms: Math.round(performance.now() - started), method: sample.method, phase: sample.phase, reduced_motion: reduced, expected, text: snapshot.text });
    }
    await stream.close(); stream = null;
    const result = await pty.waitForExit(10000);
    if (result.code !== 0) manifest.failures.push(`PTY exit ${result.code}`);
    assertSecretFree(pty.raw());
    await writeFile(join(output, reduced ? "reduced.ansi" : "motion.ansi"), pty.raw());
  } finally {
    if (stream) await stream.close();
    if (pty) await pty.cleanup();
    if (terminal) await terminal.close();
    await rm(tempRoot, { recursive: true, force: true });
  }
}
const surfaces = await captureSurfaces({ output, binary });
manifest.surfaces = surfaces;
manifest.failures.push(...surfaces.failures);
await writeFile(join(output, "surfaces-gif-frames.txt"), surfaces.frames.map(frame => `file '${join(output, frame.file)}'\nduration 1.2`).join("\n"));
execFileSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i", join(output, "surfaces-gif-frames.txt"), "-vf", "fps=2,scale=800:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=96[p];[b][p]paletteuse=dither=none", join(output, "compaction-surfaces.gif")]);
const list = manifest.frames.filter(frame => !frame.reduced_motion).map(frame => `file '${join(output, frame.file)}'\nduration 0.16`).join("\n");
await writeFile(join(output, "gif-frames.txt"), list);
execFileSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i", join(output, "gif-frames.txt"), "-vf", "fps=8,scale=960:-1:flags=lanczos", join(output, "compaction-motion.gif")]);
await writeFile(join(output, "manifest.json"), JSON.stringify(manifest, null, 2));
if (manifest.failures.length) throw new Error(manifest.failures.join("\n"));
console.log(`Captured ${manifest.frames.length} frames and compaction-motion.gif in ${output}`);
await captureCoordinatorCompaction({root,output,target});

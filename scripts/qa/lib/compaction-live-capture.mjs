import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { access, lstat, mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { openBrowserTerminal } from "./browser-terminal.mjs";
import { captureNotesRollover } from "./compaction-live-notes.mjs";
import { liveSurfaces } from "./compaction-live-surfaces.mjs";
import { spawnHarnessPty } from "./pty-session.mjs";
import { assertSecretFree, validateEvidenceDir } from "./security.mjs";

const delay = ms => new Promise(done => setTimeout(done, ms));
const quote = value => `'${String(value).replaceAll("'", "'\\''")}'`;

// This lane supplies model responses, never fabricated runtime or journal events.
export async function captureCoordinatorCompaction({ root, output, target }) {
  root = resolve(root);
  output = resolve(output);
  if (output === join(root, "docs/evidence/compaction-motion")) {
    for (let path = output; path !== root; path = dirname(path)) {
      try {
        if ((await lstat(path)).isSymbolicLink()) throw new Error(`symlink evidence path: ${path}`);
      } catch (error) {
        if (error.code !== "ENOENT") throw error;
      }
    }
  } else {
    output = await validateEvidenceDir(output, root);
  }
  const destination = join(output, "real");
  for (const path of [destination, join(destination, "frames")]) {
    try {
      if ((await lstat(path)).isSymbolicLink()) throw new Error(`symlink evidence path: ${path}`);
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
  }
  await rm(join(destination,"frames"),{recursive:true,force:true});
  await Promise.all(["failure.json","failure.png"].map(file=>rm(join(destination,file),{force:true})));
  await mkdir(join(destination, "frames"), { recursive: true });
  let binary = resolve(target ?? join(root, "target/debug/harness"));
  if ((await stat(binary)).isDirectory()) binary = join(binary, "debug/harness");
  await access(binary);
  const temporary = await mkdtemp(join(tmpdir(), "harness-compaction-live-"));
  const sessions = join(temporary, "sessions");
  await mkdir(sessions);
  const manifest = { scenario: "compaction_interactive", source: "real coordinator, local scripted provider",
    binary, geometry: { cols: 120, rows: 40 }, frames: [], checks: {}, commands: [] };
  let terminal;
  let pty;
  let recording = false;
  let frameTask;
  let journalPath;
  const save = async name => {
    const filename = `${name}.png`;
    const snapshot = await terminal.capture(join(destination, filename));
    assertSecretFree(snapshot);
    await writeFile(join(destination, `${name}.json`), JSON.stringify({cols:snapshot.cols,rows:snapshot.rows,text:snapshot.text,parsedCount:snapshot.parsedCount}, null, 2), { mode: 0o600 });
    return snapshot;
  };
  const journal = async () => {
    if (!journalPath) {
      const directories = await readdir(sessions, { withFileTypes: true });
      const run = directories.find(entry => entry.isDirectory() && entry.name.startsWith("run_"));
      if (!run) return [];
      journalPath = join(sessions, run.name, "events.jsonl");
    }
    const text = await readFile(journalPath, "utf8");
    // The writer may be between writes; only complete lines are authoritative.
    return text.slice(0, text.lastIndexOf("\n") + 1).split("\n").filter(Boolean).map(line => JSON.parse(line));
  };
  const waitJournal = async (predicate, label) => {
    const deadline = Date.now() + 30000;
    while (Date.now() < deadline) {
      const events = await journal();
      const result = predicate(events);
      if (result) return result;
      await delay(80);
    }
    throw new Error(`Timed out waiting for ${label}`);
  };
  const submit = async text => {
    manifest.commands.push({ text: text.length > 200 ? "generated context seed (20KB)" : text, at: new Date().toISOString() });
    const marker = "capture-input-complete";
    const input = text.length > 200 ? text + " " + marker : text;
    assert.ok(pty.write("\x1b[200~" + input + "\x1b[201~"),"PTY accepts bracketed paste");
    await terminal.waitForText(text.length > 200 ? "[Pasted" : text);
    assert.ok(pty.write("\r"),"PTY accepts submit key");
  };
  const kind = event => event.payload?.event_type;
  const data = event => event.payload?.data;
  try {
    terminal = await openBrowserTerminal({ cols: 120, rows: 40, browser: "/usr/bin/chromium",
      profilePath: join(temporary, "browser"), title: "Harness coordinator compaction", timeoutMs: 30000,
      captureAllCells: true, onInput: bytes => pty?.write(bytes) });
    pty = spawnHarnessPty({ cols: 120, rows: 40, tempRoot: temporary, sessionDir: sessions, cwd: temporary,
      command: `${quote(binary)} tui --scenario compaction_interactive --session-dir ${quote(sessions)} --deterministic`,
      disableAnimations: false, environment: { TERM: "xterm-256color", COLORTERM: "truecolor" },
      onOutput: bytes => terminal.write(bytes) });
    await terminal.waitForText("Local fixture");
    await save("ready");
    recording = true;
    frameTask = (async () => {
      while (recording) {
        const index = manifest.frames.length;
        const filename = `frame-${String(index).padStart(4, "0")}.png`;
        const snapshot = await terminal.capture(join(destination, "frames", filename));
        manifest.frames.push({ file: `frames/${filename}`, at: new Date().toISOString(), text: snapshot.text });
        await delay(160);
      }
    })();
    await submit(`Seed the synthetic context: cedar-checkpoint. ${"Generated public context record. ".repeat(600)}`);
    await terminal.waitForText("distilling");
    const live = await save("automatic-live-card");
    assert.match(live.text, /Auto context-full maintenance/);
    assert.match(live.text, /\[stop\]/);
    assert.match(live.text, /soft \/ threshold/);
    const boundary = await waitJournal(events => events.find(event => kind(event) === "session_compaction" && data(event)?.trigger_reason === "threshold"), "automatic threshold commit");
    assert.equal(data(boundary).method, "soft");
    assert.ok(data(boundary).tokens_before > 5000);
    assert.ok(data(boundary).tokens_after < data(boundary).tokens_before);
    assert.match(data(boundary).summary, /cedar-checkpoint/);
    await waitJournal(events => events.some(event => event.seq > boundary.seq && kind(event) === "task_completed" && data(event)?.metadata?.task_scope === "agent_turn"), "seed turn settlement");
    const divider = await save("automatic-divider");
    assert.match(divider.text, /compacted/i);
    await submit("Continue from cedar-checkpoint after automatic maintenance.");
    await terminal.waitForText("Continued answer:");
    await terminal.waitForText("survived real coordinator compaction.");
    await terminal.key("PageUp");
    await terminal.key("PageDown");
    await terminal.waitForText("compacted");
    const continued = await save("automatic-divider-and-answer");
    assert.match(continued.text, /compacted/i);
    assert.match(continued.text, /Continued answer/);
    await waitJournal(events => events.some(event => event.seq > boundary.seq && kind(event) === "assistant_message_finished"), "durable continued answer");
    manifest.checks.automatic = { passed: true, boundarySeq: boundary.seq, reason: data(boundary).trigger_reason,
      method: data(boundary).method, tokensBefore: data(boundary).tokens_before, tokensAfter: data(boundary).tokens_after,
      liveCard: "automatic-live-card.png", dividerAndAnswer: "automatic-divider-and-answer.png" };
    await delay(500);
    await submit("/compact");
    const manual = await waitJournal(events => events.find(event => kind(event) === "session_compaction" && data(event)?.trigger_reason === "manual"), "manual compaction commit");
    await terminal.waitForTextAbsent("[stop]");
    await delay(1500);
    await save("manual-compact");
    manifest.checks.manualCompact = { passed: true, boundarySeq: manual.seq };
    await submit("/shake");
    await terminal.waitForText("Nothing to shake.");
    const shake = await save("manual-shake");
    assert.doesNotMatch(shake.text, /Shake failed|unavailable/i);
    manifest.checks.manualShake = { passed: true, text: shake.text };
    // The background recorder competes with screenshots, and these surfaces are time sensitive.
    recording = false;
    await frameTask;
    const surfaces = liveSurfaces({ pty, terminal, destination, save, submit, waitJournal, kind, data, manifest });
    await surfaces.history();
    await surfaces.warningOutcome();
    await surfaces.pendingBand();
    await surfaces.cancelOutcomes();
    await surfaces.thresholdEditor();
    await captureNotesRollover({ binary, destination, manifest });
    recording = false;
    await frameTask;
    const events = await journal();
    assertSecretFree(events);
    assert.ok(!events.some(event => ["provider_text_delta", "provider_reasoning_delta", "compaction_stage"].includes(kind(event))), "live deltas must not enter durable journal");
    const journalText = await readFile(journalPath, "utf8");
    assertSecretFree(journalText);
    await writeFile(join(destination, "events.jsonl"), journalText, { mode: 0o600 });
    manifest.journal = { file: "events.jsonl", events: events.length, source: "unmodified coordinator event store" };
    execFileSync("ffmpeg", ["-y", "-framerate", "5", "-i", join(destination, "frames/frame-%04d.png"),
      "-frames:v", String(manifest.frames.length), "-filter_complex", "[0:v]split[a][b];[a]palettegen[p];[b][p]paletteuse", join(destination, "coordinator-compaction.gif")], { stdio: "pipe" });
    manifest.gif = "coordinator-compaction.gif";
    return manifest;
  } catch (error) {
    manifest.error = String(error);
    if (terminal) await save("failure");
    if (!journalPath) await journal();
    if (journalPath) {
      const text = await readFile(journalPath, "utf8");
      assertSecretFree(text);
      await writeFile(join(destination, "events.jsonl"), text, { mode: 0o600 });
    }
    throw error;
  } finally {
    recording = false;
    try { await frameTask; } finally {
      try { if (pty) manifest.ptyCleanup = await pty.cleanup(); } finally {
        try { if (terminal) manifest.browserCleanup = await terminal.close(); } finally {
          assertSecretFree(manifest);
          await writeFile(join(destination, "manifest.json"), JSON.stringify(manifest, null, 2), { mode: 0o600 });
          await rm(temporary, { recursive: true, force: true });
        }
      }
    }
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
  await captureCoordinatorCompaction({ root, output: process.argv[2], target: process.argv[3] });
}

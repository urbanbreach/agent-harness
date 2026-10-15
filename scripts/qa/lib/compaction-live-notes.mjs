// Real coordinator lane for the notes-backed window rollover: `compaction_notes_interactive` turns
// on runtime.compaction.notes_windows and keeps the real notes tools callable, so a full window
// rolls over to a fixed notice instead of being summarized by the provider.
import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { openBrowserTerminal } from "./browser-terminal.mjs";
import { spawnHarnessPty } from "./pty-session.mjs";
import { assertSecretFree } from "./security.mjs";

const delay = ms => new Promise(done => setTimeout(done, ms));
const quote = value => `'${String(value).replaceAll("'", "'\\''")}'`;

export async function captureNotesRollover({ binary, destination, manifest }) {
  const temporary = await mkdtemp(join(tmpdir(), "harness-compaction-notes-"));
  const sessions = join(temporary, "sessions");
  await mkdir(sessions);
  let terminal;
  let pty;
  const notes = { scenario: "compaction_notes_interactive", surfaces: {} };
  try {
    terminal = await openBrowserTerminal({ cols: 120, rows: 40, browser: "/usr/bin/chromium", profilePath: join(temporary, "browser"),
      title: "Harness notes rollover", timeoutMs: 30000, captureAllCells: true, onInput: bytes => pty?.write(bytes) });
    pty = spawnHarnessPty({ cols: 120, rows: 40, tempRoot: temporary, sessionDir: sessions, cwd: temporary,
      command: `${quote(binary)} tui --scenario compaction_notes_interactive --session-dir ${quote(sessions)} --deterministic`,
      disableAnimations: false, environment: { TERM: "xterm-256color", COLORTERM: "truecolor" }, onOutput: bytes => terminal.write(bytes) });
    const surface = async (name, expected) => {
      const snapshot = await terminal.capture(join(destination, `${name}.png`));
      assertSecretFree(snapshot);
      const after = await terminal.snapshot();
      for (const text of expected) assert.ok(snapshot.text.includes(text) && after.text.includes(text), `${name}: missing ${JSON.stringify(text)}`);
      notes.surfaces[name] = { file: `${name}.png`, expected, text: snapshot.text };
    };
    const journal = async () => {
      const run = (await readdir(sessions, { withFileTypes: true })).find(entry => entry.isDirectory() && entry.name.startsWith("run_"));
      if (!run) return [];
      const text = await readFile(join(sessions, run.name, "events.jsonl"), "utf8");
      return text.slice(0, text.lastIndexOf("\n") + 1).split("\n").filter(Boolean).map(line => JSON.parse(line));
    };
    await terminal.waitForText("Local fixture");
    // Several medium turns, so the rollover can cut between turns and free real context.
    let boundary;
    let answered = 0;
    for (let turn = 1; turn <= 6 && !boundary; turn += 1) {
      const input = `Seed the synthetic context: cedar-checkpoint batch ${turn}. ${"Generated public context record. ".repeat(200)} capture-input-complete`;
      assert.ok(pty.write(`\x1b[200~${input}\x1b[201~`), "PTY accepts paste");
      await terminal.waitForText("[Pasted");
      assert.ok(pty.write("\r"), "PTY accepts submit key");
      const deadline = Date.now() + 30000;
      while (Date.now() < deadline) {
        const events = await journal();
        boundary = events.find(event => event.payload?.event_type === "session_compaction");
        const finished = events.filter(event => event.payload?.event_type === "assistant_message_finished").length;
        if (boundary || finished > answered) { answered = finished; break; }
        await delay(80);
      }
    }
    assert.ok(boundary, "a compaction boundary was committed");
    const record = boundary.payload.data;
    assert.equal(record.details?.kind, "notes_rollover", "the pass was a notes rollover, not a summary");
    assert.equal(record.trigger_reason, "threshold");
    assert.ok(record.tokens_after < record.tokens_before, "the rollover freed context");
    await terminal.waitForText("compacted");
    await delay(1200);
    await surface("notes-rollover-divider", ["compacted"]);
    assert.ok(pty.write("\x1b\x0f"), "PTY accepts Ctrl+Alt+O");
    await terminal.waitForText("The context window rolled over, and the notes carry the work forward");
    await surface("notes-rollover-card-expanded", ["The context window rolled over, and the notes carry the work forward into the new window."]);
    const text = (await journal()).map(event => JSON.stringify(event)).join("\n");
    assertSecretFree(text);
    await writeFile(join(destination, "notes-events.jsonl"), `${text}\n`, { mode: 0o600 });
    Object.assign(notes, { passed: true, boundarySeq: boundary.seq, summary: record.summary, tokensBefore: record.tokens_before, tokensAfter: record.tokens_after, journal: "notes-events.jsonl" });
  } finally {
    try { if (pty) notes.ptyCleanup = await pty.cleanup(); } finally {
      try { if (terminal) notes.browserCleanup = await terminal.close(); } finally { await rm(temporary, { recursive: true, force: true }); }
    }
  }
  manifest.notesRollover = notes;
}

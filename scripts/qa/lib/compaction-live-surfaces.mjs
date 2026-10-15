// Terminal surfaces of the real coordinator lane: history toggles, command outcomes, the pending
// band, the stop control and the threshold editor. Keys are typed through xterm.js into the PTY.
import assert from "node:assert/strict";

const delay = ms => new Promise(done => setTimeout(done, ms));
const KEYS = { "ctrl+alt+k": "\x1b\x0b", "ctrl+alt+o": "\x1b\x0f", "ctrl+t": "\x14", "ctrl+c": "\x03", "alt+i": "\x1bi", enter: "\r", esc: "\x1b", pageup: "\x1b[5~", pagedown: "\x1b[6~" };

export function liveSurfaces({ pty, terminal, destination, save, submit, waitJournal, kind, data, manifest }) {
  manifest.surfaces = {};
  const press = async name => {
    assert.ok(pty.write(KEYS[name]), `PTY accepts ${name}`);
    manifest.commands.push({ key: name, at: new Date().toISOString() });
    await delay(150);
  };
  const paste = async text => {
    manifest.commands.push({ text, at: new Date().toISOString() });
    assert.ok(pty.write(`\x1b[200~${text}\x1b[201~`), "PTY accepts paste");
    await terminal.waitForText(text);
  };
  // Paste and a key in one write: the pass is short, so there is no time to wait for the echo.
  const queueNow = (text, name) => {
    manifest.commands.push({ text, key: name, at: new Date().toISOString() });
    assert.ok(pty.write(`\x1b[200~${text}\x1b[201~${KEYS[name]}`), `PTY accepts ${text}`);
  };
  // A named PNG whose screen text must contain every expected string and none of the absent ones.
  const surface = async (name, expected, absent = []) => {
    const snapshot = await save(name);
    // The PNG is taken between two reads of the screen; a pass that moved on in the meantime fails here.
    const after = await terminal.snapshot();
    for (const text of expected) assert.ok(snapshot.text.includes(text) && after.text.includes(text), `${name}: missing ${JSON.stringify(text)}`);
    for (const text of absent) assert.ok(!snapshot.text.includes(text), `${name}: unexpected ${JSON.stringify(text)}`);
    manifest.surfaces[name] = { file: `${name}.png`, expected, absent, text: snapshot.text };
    return snapshot;
  };
  const seed = word => `Seed the synthetic context: ${word}. ${"Generated public context record. ".repeat(600)}`;
  const sessionEvents = (events, type) => events.filter(event => kind(event) === type);

  return {
    press, paste, surface,
    // Collapsed history is the default; Ctrl+Alt+K shows the originals and Ctrl+Alt+O opens a card.
    async history() {
      await surface("history-collapsed-default", ["compacted"], ["Seed the synthetic context"]);
      await press("ctrl+alt+k");
      await terminal.waitForText("Compacted history shown");
      await press("pageup");
      await press("pageup");
      await surface("history-full-after-toggle", ["compacted", "Seed the synthetic context"]);
      await press("ctrl+alt+k");
      await terminal.waitForText("Compacted history collapsed");
      await press("ctrl+alt+o");
      await terminal.waitForText("Retain the checkpoint");
      await surface("summary-card-expanded", ["Retain the checkpoint", "cedar-checkpoint"]);
      await press("ctrl+alt+o");
      manifest.checks.history = { passed: true, collapsed: "history-collapsed-default.png", full: "history-full-after-toggle.png", expanded: "summary-card-expanded.png" };
    },
    // A second /compact right away has nothing left to summarize: a warning row, not a new boundary.
    async warningOutcome() {
      await submit("/compact");
      await terminal.waitForText("Nothing to compact");
      await surface("manual-outcome-warning", ["Warning: Nothing to compact"]);
      manifest.checks.manualWarning = { passed: true, file: "manual-outcome-warning.png" };
    },
    // Queue steering and a follow-up while an automatic pass runs, then let it finish and flush.
    async pendingBand() {
      await submit(seed("ember-ledger"));
      await terminal.waitForText("Auto context-full maintenance");
      queueNow("Queued steering note.", "enter");
      await delay(250);
      queueNow("Queued follow-up note.", "alt+i");
      await terminal.waitForText("After yield");
      await surface("pending-band-steering-then-after-yield", ["Auto context-full maintenance", "[stop]", "Steering", "After yield", "Queued steering note.", "Queued follow-up note."]);
      // A follow-up keeps its text in the composer; clear it so later input starts clean.
      await press("esc");
      await press("esc");
      await waitJournal(events => sessionEvents(events, "session_compaction").filter(event => data(event)?.trigger_reason === "threshold").length >= 2, "second automatic compaction commit");
      await waitJournal(events => sessionEvents(events, "user_message_submitted").some(event => data(event)?.text === "Queued steering note."), "queued steering delivered after the pass");
      await terminal.waitForTextAbsent("[stop]");
      await delay(1500);
      manifest.checks.pendingBand = { passed: true, file: "pending-band-steering-then-after-yield.png" };
    },
    // Ctrl+C during an automatic pass ends it with a status row; the context stays large, so a
    // manual /compact then runs, shows its loader and stop control, and Ctrl+C reports it cancelled.
    async cancelOutcomes() {
      await submit(seed("amber-ridge"));
      await terminal.waitForText("Auto context-full maintenance");
      await press("ctrl+c");
      await terminal.waitForText("Auto context-full maintenance cancelled");
      await surface("auto-outcome-cancelled", ["Auto context-full maintenance cancelled"]);
      await terminal.waitForTextAbsent("[stop]");
      await delay(800);
      await submit("/compact");
      await terminal.waitForText("Compacting context");
      await surface("manual-loader-and-stop", ["Compacting context", "[stop]", "soft / manual"]);
      await press("ctrl+c");
      await terminal.waitForText("Compaction cancelled");
      await surface("manual-outcome-cancelled", ["Error: Compaction cancelled"]);
      manifest.checks.cancel = { passed: true, auto: "auto-outcome-cancelled.png", loader: "manual-loader-and-stop.png", manual: "manual-outcome-cancelled.png" };
    },
    // Ctrl+T in the model switcher edits the compaction threshold of the highlighted model.
    async thresholdEditor() {
      await submit("/model");
      await terminal.waitForText("Ctrl+T threshold");
      await surface("model-switcher", ["Models", "Ctrl+T threshold"]);
      await press("ctrl+t");
      await terminal.waitForText("Compaction threshold for");
      await surface("threshold-editor-empty", ["Compaction threshold for", "Model window: 64K tokens"]);
      await paste("f4000");
      await surface("threshold-editor-fixed-tokens", ["f4000", "Compacts at"]);
      await press("esc");
      await press("esc");
      manifest.checks.thresholdEditor = { passed: true, files: ["model-switcher.png", "threshold-editor-empty.png", "threshold-editor-fixed-tokens.png"] };
    },
  };
}

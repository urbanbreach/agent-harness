import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { openBrowserTerminal } from "./lib/browser-terminal.mjs";
import { writePassEvidence } from "./lib/evidence.mjs";

const canonicalViewports = [
  { cols: 80, rows: 24 },
  { cols: 120, rows: 40 },
  { cols: 160, rows: 50 },
];

for (const viewport of canonicalViewports) {
  test(`xterm collector structures canonical ${viewport.cols}x${viewport.rows} terminal state`, async () => {
    // Given: a real browser xterm with normal-buffer history and an alternate-buffer mode fixture.
    const profilePath = await mkdtemp(join(tmpdir(), "harness-xterm-p0-06-profile-"));
    const terminal = await openBrowserTerminal({
      browser: "/usr/bin/chromium",
      ...viewport,
      timeoutMs: 5000,
      title: `Harness P0-06 ${viewport.cols}x${viewport.rows}`,
      profilePath,
      onInput: () => false,
    });

    try {
      const history = Array.from(
        { length: viewport.rows + 2 },
        (_, index) => `Harness history ${String(index).padStart(3, "0")}\r\n`,
      ).join("");
      await terminal.write(Buffer.from(history));
      const normal = await terminal.snapshot();

      // When: xterm enters its alternate buffer and receives wrapping, cursor, and mode controls.
      await terminal.write(Buffer.from(
        `\u001b[?1049h\u001b[?25l\u001b[?1h\u001b=\u001b[?2004h\u001b[?1000h${"A".repeat(viewport.cols)}!\u001b[2;4H`,
      ));
      const alternate = await terminal.snapshot();
      await terminal.write(Buffer.from("\u001b[?1049l"));
      const restored = await terminal.snapshot();

      // Then: the machine-consumed snapshots expose every P0-06 terminal-state assertion class.
      assert.equal(normal.activeBuffer, "normal");
      assert.ok(normal.scrollback.lines > 0);
      assert.match(normal.scrollback.text, /Harness history 000/);
      assert.equal(alternate.activeBuffer, "alternate");
      assert.deepEqual(alternate.cursor, {
        x: 3,
        y: 1,
        baseY: 0,
        viewportY: 0,
        visible: false,
      });
      assert.equal(alternate.modes.applicationCursorKeysMode, true);
      assert.equal(alternate.modes.applicationKeypadMode, true);
      assert.equal(alternate.modes.bracketedPasteMode, true);
      assert.notEqual(alternate.modes.mouseTrackingMode, "none");
      assert.ok(alternate.wrappedRows.length > 0);
      assert.ok(alternate.cells.some((cell) => cell.chars === "A" && cell.width === 1));
      assert.equal(restored.activeBuffer, "normal");
      assert.ok(restored.scrollback.lines > 0);
    } finally {
      await terminal.close();
    }
  });
}

test("PASS evidence rejects Grok marks in collected runtime text, title, and metadata", async () => {
  // Given: otherwise valid Harness evidence whose collected terminal metadata contains a Grok mark.
  const evidenceDir = await mkdtemp(join(tmpdir(), "harness-xterm-p0-06-brand-reject-"));
  const settings = await passSettings(evidenceDir, {
    text: "Harness runtime\nGrok runtime",
    title: "Harness P0-06",
  });

  try {
    // When/Then: PASS remains fail closed at the runtime evidence boundary.
    await assert.rejects(writePassEvidence(settings), /Grok branding/);
    await assert.rejects(readFile(join(evidenceDir, "PASS.json")), /ENOENT/);
  } finally {
    await rm(evidenceDir, { recursive: true, force: true });
  }
});

test("PASS evidence rejects Harness only in title, QA font, typed input, command, or provenance", async () => {
  // Given: a child that renders no Harness mark while every non-rendered source can mention it.
  const evidenceDir = await mkdtemp(join(tmpdir(), "harness-xterm-p0-06-brand-child-reject-"));
  const settings = await passSettings(evidenceDir, {
    text: "typed input only",
    title: "Harness scenario title",
  });
  settings.capture.cells = [];
  settings.capture.scrollback = { lines: 0, length: 1, text: "typed input only" };
  settings.interactions = [{
    action: { kind: "type", value: "Harness typed input" },
    result: { text: "neutral child output", cells: [], scrollback: { text: "" } },
  }];
  settings.command = "Harness command";
  settings.argv = ["--assert", "Harness provenance"];
  settings.repoRoot = "/tmp/Harness/provenance";
  settings.browserMetadata.font.family = "Harness QA Mono";
  settings.browserMetadata.terminal = { ...settings.capture };

  try {
    // When/Then: PASS evidence is requested without a child-rendered brand mark.
    await assert.rejects(writePassEvidence(settings), /Harness branding missing/);
    await assert.rejects(readFile(join(evidenceDir, "PASS.json")), /ENOENT/);
  } finally {
    await rm(evidenceDir, { recursive: true, force: true });
  }
});

test("runtime branding checks ignore source-reference paths and command arguments", async () => {
  // Given: Harness-only runtime state plus non-runtime provenance containing a reference name.
  const evidenceDir = await mkdtemp(join(tmpdir(), "harness-xterm-p0-06-brand-scope-"));
  const settings = await passSettings(evidenceDir, {
    text: "neutral final child output",
    title: "Harness P0-06",
  });
  settings.interactions = [{
    action: { kind: "wait", value: "Harness" },
    result: { text: "Harness startup mark", cells: [{ chars: "Harness" }], scrollback: { text: "" } },
  }];
  settings.browserMetadata.terminal = { ...settings.capture };
  settings.command = "fixture --reference /tmp/Grok/reference";
  settings.argv = ["--command", "fixture --reference /tmp/Grok/reference"];
  settings.repoRoot = "/tmp/Grok/reference";

  try {
    // When: valid runtime evidence is persisted.
    await writePassEvidence(settings);
    const metadata = JSON.parse(await readFile(join(evidenceDir, "metadata.json"), "utf8"));

    // Then: the runtime branding receipt passes without treating provenance as rendered output.
    assert.deepEqual(metadata.runtimeBranding, {
      requiredMark: "Harness",
      requiredMarkPresent: true,
      forbiddenMarks: ["Grok", "xAI"],
      forbiddenMarksPresent: false,
    });
  } finally {
    await rm(evidenceDir, { recursive: true, force: true });
  }
});

async function passSettings(evidenceDir, runtime) {
  const png = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
    "base64",
  );
  await writeFile(join(evidenceDir, "terminal.png"), png);
  const capture = {
    cols: 80,
    rows: 24,
    activeBuffer: "normal",
    cursor: { x: 0, y: 0, baseY: 0, viewportY: 0, visible: true },
    modes: {},
    cells: [],
    wrappedRows: [],
    scrollback: { lines: 0, length: 24, text: "" },
    text: runtime.text,
    title: runtime.title,
    titleHistory: [runtime.title],
    renderCount: 1,
    parsedCount: 1,
  };
  return {
    evidenceDir,
    raw: Buffer.from(runtime.text),
    capture,
    captures: [],
    interactions: [],
    contract: { name: "smoke", title: runtime.title, command: "fixture" },
    command: "fixture",
    argv: [],
    repoRoot: "/repo",
    browser: "/usr/bin/chromium",
    browserMetadata: {
      browserVersion: "test",
      xtermVersion: "6.0.0",
      playwrightVersion: "1.55.0",
      font: { family: "Harness QA Mono", source: "/font.ttf", embedded: true },
      terminal: capture,
    },
    sourceTree: { hash: "tree" },
    scriptSha256: "script",
    assertions: [],
    cleanup: {
      pty: {
        childExited: true,
        processGroupAlive: false,
        stdinClosed: true,
        temporarySockets: [],
      },
      browser: {
        pageClosed: true,
        contextClosed: true,
        browserConnectedAfterClose: false,
        profileRemoved: true,
        boundPorts: [],
      },
      tempRootRemoved: true,
    },
  };
}

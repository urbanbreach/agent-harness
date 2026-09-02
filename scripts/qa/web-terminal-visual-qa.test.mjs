import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { openBrowserTerminal } from "./lib/browser-terminal.mjs";
import { parseArgs, scenarioContract } from "./lib/config.mjs";
import { writePassEvidence } from "./lib/evidence.mjs";
import { resolveCommand } from "./lib/pty-session.mjs";
import { assertStableExecutable, sha256 } from "./lib/provenance.mjs";
import { executeActions } from "./web-terminal-visual-qa.mjs";

const scenario = "transcript-response-navigation";

test("parseArgs rejects an unknown scenario when input crosses the CLI boundary", () => {
  // Given: a scenario name outside the shipped QA vocabulary.
  const argv = ["--scenario", "unknown", "--evidence-dir", "/tmp/evidence"];

  // When: the CLI boundary parses the input.
  const action = () => parseArgs(argv);

  // Then: the unsupported scenario is rejected before resources are spawned.
  assert.throws(action, /unknown scenario/);
});

test("scenarioContract requires an explicit fixture for a planned P0 scenario", () => {
  // Given: the planned navigation scenario without a production fixture command.
  const options = parseArgs(["--scenario", scenario, "--evidence-dir", "/tmp/evidence"]);

  // When: its executable contract is resolved.
  const action = () => scenarioContract(options, {});

  // Then: the driver fails closed rather than manufacturing a parity pass.
  assert.throws(action, /fixture command is not available/);
});

test("scenarioContract requires an explicit active-block fixture", () => {
  // Given: the planned active-block scenario without a production fixture command.
  const options = parseArgs([
    "--scenario",
    "transcript-active-block",
    "--evidence-dir",
    "/tmp/evidence",
  ]);

  // When: its executable contract is resolved.
  const action = () => scenarioContract(options, {});

  // Then: it is recognized but cannot claim evidence without the fixture.
  assert.throws(action, /fixture command is not available/);
});

test("scenarioContract parses P0-02 navigation and click actions", () => {
  // Given: the shared helper's detach, response navigation, and fold actions.
  const options = parseArgs([
    "--scenario", scenario,
    "--evidence-dir", "/tmp/evidence",
    "--command", "p0-02-helper",
    "--input", "{PageUp}",
    "--input", "{PageDown}",
    "--input", "{Shift+J}",
    "--input", "{Shift+K}",
    "--input", "{Click:Ran 14 commands}",
    "--assert", "Harness 1/3",
  ]);

  // When: the driver resolves the scenario contract.
  const contract = scenarioContract(options, {});

  // Then: browser key names and the visible click target are preserved exactly.
  assert.deepEqual(contract.actions, [
    { kind: "key", value: "PageUp" },
    { kind: "key", value: "PageDown" },
    { kind: "key", value: "Shift+J" },
    { kind: "key", value: "Shift+K" },
    { kind: "click", value: "Ran 14 commands" },
  ]);
});

test("scenarioContract parses and normalizes WaitTitle as a distinct action", () => {
  // Given: a fixture action that waits for the xterm OSC title.
  const options = parseArgs([
    "--scenario", scenario,
    "--evidence-dir", "/tmp/evidence",
    "--command", "p0-02-helper",
    "--input", "{WaitTitle:P0-02 append applied}",
    "--assert", "Harness 1/3",
  ]);

  // When: the driver resolves the scenario contract.
  const contract = scenarioContract(options, {});

  // Then: the title wait remains distinct from a visible-text wait.
  assert.deepEqual(contract.actions, [
    { kind: "waitTitle", value: "P0-02 append applied" },
  ]);
});

test("waitForTitle returns the title that satisfied a transient wait", async () => {
  // Given: an xterm terminal receiving a title that is immediately overwritten.
  const profilePath = await mkdtemp(join(tmpdir(), "harness-xterm-title-test-"));
  const terminal = await openBrowserTerminal({
    browser: "/usr/bin/chromium",
    cols: 80,
    rows: 10,
    timeoutMs: 5000,
    title: "title test",
    profilePath,
    onInput: () => false,
  });

  try {
    // When: the title wait observes the transient OSC title.
    await terminal.write(Buffer.from("\u001b]2;P0-02 append applied\u0007\u001b]2;later title\u0007"));
    const result = await terminal.waitForTitle("P0-02 append applied");

    // Then: the result preserves the match and ordered title history.
    assert.equal(result.matchedTitle, "P0-02 append applied");
    assert.equal(result.title, "later title");
    assert.deepEqual(result.titleHistory, [
      "title test",
      "P0-02 append applied",
      "later title",
    ]);
  } finally {
    await terminal.close();
  }
});

test("capture actions preserve indexed PNGs and finish with a final screenshot", async () => {
  // Given: two capture actions and a browser terminal that writes each requested screenshot.
  const evidenceDir = await mkdtemp(join(tmpdir(), "harness-xterm-capture-test-"));
  const png = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
    "base64",
  );
  const snapshots = ["first", "second", "final"].map((text) => ({ text }));
  const screenshotPaths = [];
  const terminal = {
    async capture(path) {
      screenshotPaths.push(path);
      await writeFile(path, png);
      return snapshots[screenshotPaths.length - 1];
    },
    async snapshot() {
      return snapshots.at(-1);
    },
  };
  const interactions = [];

  try {
    // When: the action runner executes both captures.
    const result = await executeActions({
      actions: [{ kind: "capture" }, { kind: "capture" }],
      terminal,
      pty: { flush: async () => {} },
      interactions,
      evidenceDir,
    });

    // Then: each action has its own receipt and the last explicit state is selected.
    assert.deepEqual(screenshotPaths.map((path) => path.split("/").at(-1)), [
      "capture-001.png",
      "capture-002.png",
    ]);
    assert.equal(result.capture.text, "second");
    assert.deepEqual(
      await readFile(join(evidenceDir, "terminal.png")),
      await readFile(join(evidenceDir, "capture-002.png")),
    );
    assert.deepEqual(interactions.map(({ result: actionResult }) => actionResult.screenshot), [
      "capture-001.png",
      "capture-002.png",
    ]);
    assert.deepEqual(interactions.map(({ bufferSnapshot }) => bufferSnapshot.text), ["first", "second"]);
  } finally {
    await rm(evidenceDir, { recursive: true, force: true });
  }
});

test("smoke keeps its explicit assertion capture through post-capture cleanup", async () => {
  // Given: the built-in smoke journey and cleanup actions after its explicit capture.
  const evidenceDir = await mkdtemp(join(tmpdir(), "harness-xterm-smoke-capture-test-"));
  const png = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
    "base64",
  );
  const selected = { text: "Commands\nHarness xterm smoke" };
  const screenshotPaths = [];
  const terminal = {
    async waitForText() {
      return selected;
    },
    async waitForTextAbsent() {
      return { text: "" };
    },
    async type() {},
    async key() {},
    async capture(path) {
      screenshotPaths.push(path);
      await writeFile(path, png);
      return selected;
    },
    async snapshot() {
      return { text: "post-cleanup" };
    },
  };
  const contract = scenarioContract(
    parseArgs(["--scenario", "smoke", "--evidence-dir", evidenceDir]),
    {},
  );
  const interactions = [];

  try {
    // When: the smoke actions run through the shared action runner.
    const result = await executeActions({
      actions: contract.actions,
      terminal,
      pty: { flush: async () => {} },
      interactions,
      evidenceDir,
    });

    // Then: assertions and terminal.png retain the explicit Commands capture.
    assert.equal(result.capture.text, selected.text);
    assert.deepEqual(screenshotPaths.map((path) => path.split("/").at(-1)), ["capture-001.png"]);
    assert.deepEqual(
      await readFile(join(evidenceDir, "terminal.png")),
      await readFile(join(evidenceDir, "capture-001.png")),
    );
  } finally {
    await rm(evidenceDir, { recursive: true, force: true });
  }
});

test("pass evidence receipts include indexed captures with PNG dimensions and hashes", async () => {
  // Given: a final screenshot and an indexed action screenshot in an evidence directory.
  const evidenceDir = await mkdtemp(join(tmpdir(), "harness-xterm-manifest-test-"));
  const png = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
    "base64",
  );
  await writeFile(join(evidenceDir, "capture-001.png"), png);
  await writeFile(join(evidenceDir, "terminal.png"), png);
  const capture = { cols: 2, rows: 1, title: "Harness manifest fixture", activeBuffer: "normal", cursor: {}, modes: {}, text: "Harness final", renderCount: 1, parsedCount: 1 };

  try {
    // When: PASS evidence is serialized.
    await writePassEvidence({
      evidenceDir,
      raw: "",
      capture,
      captures: [{ index: 1, path: join(evidenceDir, "capture-001.png") }],
      interactions: [],
      contract: { name: "test", title: "test", command: "test" },
      command: "test",
      argv: [],
      repoRoot: "/repo",
      browser: "/browser",
      browserMetadata: {},
      sourceTree: { hash: "tree" },
      scriptSha256: "script",
      binaryProvenance: {
        source: "target/debug/harness",
        testedCopy: "harness-under-test",
        before: { bytes: 42, sha256: "abc" },
        after: { bytes: 42, sha256: "abc" },
        unchanged: true,
      },
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
    });
    const metadata = JSON.parse(await readFile(join(evidenceDir, "metadata.json"), "utf8"));
    const manifest = JSON.parse(await readFile(join(evidenceDir, "artifact-manifest.json"), "utf8"));

    // Then: metadata and the manifest independently identify the indexed PNG and its valid receipt.
    assert.deepEqual(metadata.captures, [{
      index: 1,
      path: "capture-001.png",
      width: 1,
      height: 1,
      sha256: sha256(png),
      pngSignatureValid: true,
      bytes: png.length,
    }]);
    assert.equal(manifest.artifacts["capture-001"].path, "capture-001.png");
    assert.equal(manifest.artifacts["capture-001"].bytes, png.length);
    assert.equal(manifest.artifacts["capture-001"].png.width, 1);
    assert.equal(manifest.artifacts["capture-001"].png.height, 1);
    assert.equal(manifest.artifacts["capture-001"].sha256, metadata.captures[0].sha256);
    assert.equal(metadata.provenance.binary.unchanged, true);
    assert.equal(manifest.artifacts.binaryProvenance.path, "harness-binary-provenance.txt");
  } finally {
    await rm(evidenceDir, { recursive: true, force: true });
  }
});

test("executable provenance fails closed when tested bytes change", () => {
  const before = { path: "harness-under-test", bytes: 42, sha256: "before" };
  const after = { path: "harness-under-test", bytes: 42, sha256: "after" };

  assert.throws(
    () => assertStableExecutable(before, after),
    /executable changed during QA/,
  );
});

test("waitForText drains queued writes before returning the final cursor", async () => {
  // Given: a marker write followed by a queued cursor update.
  const profilePath = await mkdtemp(join(tmpdir(), "harness-xterm-write-barrier-test-"));
  const terminal = await openBrowserTerminal({
    browser: "/usr/bin/chromium",
    cols: 100,
    rows: 10,
    timeoutMs: 5000,
    title: "write barrier test",
    profilePath,
    onInput: () => false,
  });

  try {
    const firstWrite = terminal.write(Buffer.from("marker"));
    const secondWrite = terminal.write(Buffer.from("\u001b[4;96H"));

    // When: the marker wait observes the first queued write.
    const snapshot = await terminal.waitForText("marker");
    await Promise.all([firstWrite, secondWrite]);

    // Then: the returned snapshot includes the trailing cursor update.
    assert.deepEqual(snapshot.cursor, { x: 95, y: 3, baseY: 0, viewportY: 0, visible: true });
  } finally {
    await terminal.close();
  }
});

test("capture repaints a right-edge label from the current xterm buffer", async () => {
  // Given: a right-edge label written into a browser-backed xterm buffer.
  const profilePath = await mkdtemp(join(tmpdir(), "harness-xterm-edge-test-"));
  const evidenceDir = await mkdtemp(join(tmpdir(), "harness-xterm-edge-evidence-"));
  const terminal = await openBrowserTerminal({
    browser: "/usr/bin/chromium",
    cols: 100,
    rows: 10,
    timeoutMs: 5000,
    title: "edge test",
    profilePath,
    onInput: () => false,
  });

  try {
    await terminal.write(Buffer.from("\u001b[1;89HHarness 1/3"));
    const capturePath = join(evidenceDir, "capture-001.png");

    // When: the synchronized capture forces a covering repaint.
    const after = await terminal.capture(capturePath);

    // Then: the edge marker remains in the selected buffer after the repaint.
    assert.match(after.text, /Harness 1\/3/);
    assert.deepEqual(after.lastRender, { start: 0, end: 9 });
    assert.deepEqual((await readFile(capturePath)).subarray(0, 8), Buffer.from("89504e470d0a1a0a", "hex"));
  } finally {
    await terminal.close();
    await rm(evidenceDir, { recursive: true, force: true });
  }
});

test("right-edge pixels change when a label is written", async () => {
  // Given: a blank 100-column browser terminal with its cursor hidden.
  const profilePath = await mkdtemp(join(tmpdir(), "harness-xterm-edge-pixels-test-"));
  const evidenceDir = await mkdtemp(join(tmpdir(), "harness-xterm-edge-pixels-evidence-"));
  const terminal = await openBrowserTerminal({
    browser: "/usr/bin/chromium",
    cols: 100,
    rows: 10,
    timeoutMs: 5000,
    title: "edge pixels test",
    profilePath,
    onInput: () => false,
  });

  try {
    await terminal.write(Buffer.from("\u001b[?25l"));
    const blankPath = join(evidenceDir, "blank.png");
    const labelPath = join(evidenceDir, "label.png");
    await terminal.capture(blankPath);
    await terminal.write(Buffer.from("\u001b[1;87HHarness 2/3"));
    await terminal.capture(labelPath);

    // When: the right-edge label is rendered after the blank capture.
    // Then: the DOM renderer places its text span over the column-90 cell.
    assert.equal(await terminal.renderedCellHasText(86, 0, "Harness 2/3"), true);
  } finally {
    await terminal.close();
    await rm(evidenceDir, { recursive: true, force: true });
  }
});

test("scenarioContract exposes the deterministic Harness smoke journey", () => {
  // Given: the built-in smoke scenario.
  const options = parseArgs(["--scenario", "smoke", "--evidence-dir", "/tmp/evidence"]);

  // When: its executable contract is resolved.
  const contract = scenarioContract(options, {});

  // Then: it drives the real Harness mock TUI through observable markers.
  assert.match(contract.command, /harness tui --mock --deterministic/);
  assert.deepEqual(contract.actions, [
    { kind: "wait", value: "Demo mode" },
    { kind: "type", value: "P0-06 canonical" },
    { kind: "wait", value: "P0-06 canonical" },
    { kind: "key", value: "Control+P" },
    { kind: "wait", value: "Commands" },
    { kind: "capture" },
  ]);
  assert.equal(contract.expectNaturalExit, false);
});

test("scenarioContract normalizes multiline composer modifier tokens", () => {
  // Given: the three modifier combinations supported by the composer contract.
  const options = parseArgs([
    "--scenario", "transcript-response-navigation",
    "--evidence-dir", "/tmp/evidence",
    "--command", "p0-04-helper",
    "--input", "{Shift+Enter}",
    "--input", "{Alt+Enter}",
    "--input", "{Alt+M}",
    "--input", "{Alt+S}",
    "--input", "{Alt+I}",
    "--input", "{Alt+R}",
    "--input", "{Ctrl+Alt+Enter}",
    "--input", "{Ctrl+Shift+Enter}",
    "--assert", "sentinel",
  ]);

  // When: the driver resolves the action contract.
  const contract = scenarioContract(options, {});

  // Then: browser key names use the modifier names expected by Playwright.
  assert.deepEqual(contract.actions, [
    { kind: "key", value: "Shift+Enter" },
    { kind: "key", value: "Alt+Enter" },
    { kind: "key", value: "Alt+m" },
    { kind: "key", value: "Alt+s" },
    { kind: "key", value: "Alt+i" },
    { kind: "key", value: "Alt+r" },
    { kind: "key", value: "Control+Alt+Enter" },
    { kind: "key", value: "Control+Shift+Enter" },
  ]);
});

test("scenarioContract exposes the deterministic multiline composer journey", () => {
  // Given: the shipped P0-04 composer scenario.
  const options = parseArgs([
    "--scenario", "composer-multiline-actions", "--evidence-dir", "/tmp/evidence",
  ]);

  // When: its executable contract is resolved.
  const contract = scenarioContract(options, {});

  // Then: the contract includes the multiline sentinels and queued status marker.
  assert.match(contract.command, /HARNESS_TUI_P0_04_SCENARIO=1/);
  assert.deepEqual(contract.assertions, [
    "MULTILINE",
    "QUEUED",
    "second line",
    "interject text",
    "replacement text",
  ]);
  assert.deepEqual(contract.actions.slice(0, 13), [
    { kind: "wait", value: "P0-04 active streaming" },
    { kind: "key", value: "Alt+m" },
    { kind: "wait", value: "MULTILINE" },
    { kind: "type", value: "first line" },
    { kind: "key", value: "Enter" },
    { kind: "type", value: "second line" },
    { kind: "wait", value: "Enter:newline" },
    { kind: "wait", value: "Alt+s:send" },
    { kind: "wait", value: "Alt+i:interject" },
    { kind: "wait", value: "Alt+r:replace" },
    { kind: "capture" },
    { kind: "key", value: "Alt+s" },
    { kind: "waitCount", value: "QUEUED", count: 1 },
  ]);
  assert.deepEqual(contract.actions.slice(13), [
    { kind: "type", value: "interject text" },
    { kind: "key", value: "Alt+i" },
    { kind: "waitCount", value: "QUEUED", count: 2 },
    { kind: "type", value: "replacement text" },
    { kind: "key", value: "Alt+r" },
    { kind: "waitCount", value: "QUEUED", count: 3 },
    { kind: "capture" },
    { kind: "key", value: "Control+Q" },
    { kind: "key", value: "Control+Q" },
  ]);
});

test("resolveCommand anchors the deterministic helper to the repository", async () => {
  const command = await resolveCommand(
    "cargo test --manifest-path $HARNESS_QA_REPO_ROOT/Cargo.toml",
    "/tmp/harness repo",
  );

  assert.equal(command, "cargo test --manifest-path '/tmp/harness repo'/Cargo.toml");
});

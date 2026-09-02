import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { openBrowserTerminal } from "./lib/browser-terminal.mjs";
import { parseArgs, scenarioContract } from "./lib/config.mjs";

test("P1-02 modal chrome drives the shipped Harness binary and complete interaction contract", () => {
  // arrange
  const contract = scenarioContract(parseArgs([
    "--scenario", "p1-02-modal-chrome",
    "--evidence-dir", "/tmp/evidence",
    "--cols", "120",
    "--rows", "40",
  ]), {});

  // act and assert
  assert.match(contract.command, /harness tui --mock --deterministic/);
  assert.deepEqual(contract.assertions, ["Models", "navigate", "Esc close"]);
  assert.ok(contract.actions.some((action) => action.kind === "key" && action.value === "Tab"));
  assert.ok(contract.actions.some((action) => action.kind === "key" && action.value === "Shift+Tab"));
  assert.ok(contract.actions.some((action) => action.kind === "mouseDown"));
  assert.ok(contract.actions.some((action) => action.kind === "mouseUp"));
  assert.ok(contract.actions.some((action) => action.kind === "clickCell"));
  assert.equal(contract.actions.filter((action) => action.kind === "waitFrame").length, 2);
  assert.equal(contract.actions.filter((action) => action.kind === "capture").length, 5);
});

test("P1-02 frame wait observes a complete rendered modal", async () => {
  const tempRoot = await mkdtemp(join(tmpdir(), "harness-xterm-p1-02-frame-wait-"));
  const terminal = await openBrowserTerminal({
    browser: "/usr/bin/chromium",
    cols: 20,
    rows: 8,
    timeoutMs: 5000,
    title: "Harness P1-02 frame wait",
    profilePath: join(tempRoot, "profile"),
    onInput: () => false,
  });

  try {
    await terminal.write(Buffer.from([
      "\u001b[2;3H┌────────┐",
      "\u001b[3;3H│ [TUI]  │",
      "\u001b[4;3H│        │",
      "\u001b[5;3H└────────┘",
    ].join("")));

    const snapshot = await terminal.waitForFrame({
      marker: "[TUI]",
      left: 3,
      top: 2,
      right: 12,
      bottom: 5,
    });

    assert.match(snapshot.text, /\[TUI\]/);
  } finally {
    await terminal.close();
    await rm(tempRoot, { recursive: true, force: true });
  }
});

test("P1-02 screenshots tightly frame the full xterm surface at every canonical size", async () => {
  for (const [cols, rows] of [[80, 24], [120, 40], [160, 50]]) {
    // arrange
    const tempRoot = await mkdtemp(join(tmpdir(), "harness-xterm-p1-02-frame-"));
    const screenshotPath = join(tempRoot, "terminal.png");
    const terminal = await openBrowserTerminal({
      browser: "/usr/bin/chromium",
      cols,
      rows,
      timeoutMs: 5000,
      title: `Harness P1-02 framing ${cols}x${rows}`,
      profilePath: join(tempRoot, "profile"),
      onInput: () => false,
    });

    try {
      await terminal.write(Buffer.from(`Harness ${cols}x${rows}\u001b[${rows};${cols - 3}Hedge`));

      // act
      const capture = await terminal.capture(screenshotPath);
      const [png, metadata, edgeRendered] = await Promise.all([
        readFile(screenshotPath),
        terminal.metadata(),
        terminal.renderedCellHasText(cols - 4, rows - 1, "edge"),
      ]);
      assert.match(capture.lines.at(-1).text, /edge$/);
      assert.equal(edgeRendered, true);
      const width = png.readUInt32BE(16);
      const height = png.readUInt32BE(20);

      // assert
      assert.deepEqual(
        { width, height },
        {
          width: Math.ceil(metadata.renderSurface.screen.width),
          height: Math.ceil(metadata.renderSurface.screen.height),
        },
      );
      assert.ok(width >= Math.ceil(metadata.renderDimensions.css.canvas.width));
      assert.ok(height >= Math.ceil(metadata.renderDimensions.css.canvas.height));
    } finally {
      await terminal.close();
      await rm(tempRoot, { recursive: true, force: true });
    }
  }
});

test("P1-02 close target coordinates stay inside the six-cell target at every canonical size", () => {
  for (const [cols, rows] of [[80, 24], [120, 40], [160, 50]]) {
    // arrange
    const contract = scenarioContract(parseArgs([
      "--scenario", "p1-02-modal-chrome",
      "--evidence-dir", "/tmp/evidence",
      "--cols", String(cols),
      "--rows", String(rows),
    ]), {});

    // act
    const close = contract.actions.find((action) => action.kind === "clickCell");
    const popupWidth = Math.min(cols, 88);
    const popupX = Math.floor((cols - popupWidth) / 2);

    // assert
    assert.equal(close.row, Math.floor((rows - Math.min(rows, 28)) / 2) + 1);
    assert.ok(close.column >= popupX + popupWidth - 5);
    assert.ok(close.column <= popupX + popupWidth);
  }
});

import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { openBrowserTerminal } from "./lib/browser-terminal.mjs";
import { commandRecorder } from "./lib/command-recorder.mjs";

test("capture commands retain concurrent exits, redacted logs and timeout receipts", async () => {
  const output = await mkdtemp(join(tmpdir(), "harness-command-recorder-"));
  const run = commandRecorder({ cwd: output, output, timeout: 1000,
    env: { NO_COLOR: "1" }, receipts: [{ label: "prior" }],
  });
  try {
    await Promise.all([
      run("success", process.execPath, ["-e", `
        if (process.env.NO_COLOR || process.env.TERM !== "xterm-256color") process.exit(2);
        process.stdout.write(["TOKEN", "synthetic"].join("="));
        process.stdout.write(String.fromCharCode(10));
        process.stdout.write(Buffer.from([0xe2]));
        setTimeout(() => process.stdout.write(Buffer.from([0x9c, 0x93])), 10);
      `]),
      assert.rejects(run("failure", process.execPath, ["-e", "process.stderr.write('failed'); process.exit(7)"]), /exit 7/),
      assert.rejects(run("timeout", process.execPath, ["-e", "setInterval(() => {}, 1000)"]), /SIGTERM/),
    ]);
    const receipts = JSON.parse(await readFile(join(output, "commands.json"), "utf8"));
    assert.equal(receipts.length, 4);
    assert.equal(receipts[0].label, "prior");
    const byLabel = Object.fromEntries(receipts.slice(1).map(receipt => [receipt.label, receipt]));
    assert.equal(byLabel.success.code, 0);
    assert.equal(byLabel.failure.code, 7);
    assert.equal(byLabel.timeout.signal, "SIGTERM");
    assert.equal(new Set(receipts.slice(1).map(receipt => receipt.logPath)).size, 3);
    assert.equal(await readFile(join(output, byLabel.success.logPath), "utf8"), "TOKEN=[REDACTED]\n✓");
    assert.equal(await readFile(join(output, byLabel.failure.logPath), "utf8"), "failed");
  } finally {
    await rm(output, { recursive: true, force: true });
  }
});

test("full-cell captures preserve blank backgrounds and text modifiers", async () => {
  // Given: a real xterm surface with full-cell evidence enabled.
  const terminal = await openBrowserTerminal({
    browser: "/usr/bin/chromium",
    cols: 12,
    rows: 3,
    timeoutMs: 5000,
    title: "Tool fidelity",
    profilePath: await mkdtemp(join(tmpdir(), "harness-tool-fidelity-")),
    captureAllCells: true,
    onInput() {},
  });
  try {
    // When: styled text and a styled blank cell are written and parsed.
    await terminal.write(Buffer.from("\x1b[1;2;3;4;7;9;53;48;2;10;20;30mX \x1b[0m"));
    const snapshot = await terminal.snapshot();

    // Then: comparison evidence retains the paint that sparse text loses.
    assert.equal(snapshot.cells.length, 36);
    const [text, blank, plain] = snapshot.cells;
    assert.equal(blank.chars, " ");
    assert.equal(blank.bgColor, 0x0a141e);
    for (const attribute of ["bold", "dim", "italic", "underline", "inverse", "strikethrough", "overline"]) {
      assert.equal(text[attribute], true, attribute);
      assert.equal(blank[attribute], true, attribute);
      assert.equal(plain[attribute], false, attribute);
    }
  } finally {
    await terminal.close();
  }
});

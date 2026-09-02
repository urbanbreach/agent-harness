import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { openBrowserTerminal } from "./lib/browser-terminal.mjs";
import { parseArgs, scenarioContract } from "./lib/config.mjs";
import { spawnHarnessPty } from "./lib/pty-session.mjs";
import { executeActions } from "./web-terminal-visual-qa.mjs";

const variants = [
  {
    name: "unicode",
    expectedEnvironment: {
      TERM: "xterm-256color",
      TERM_PROGRAM: "WezTerm",
      COLORTERM: "truecolor",
    },
    expectedClassification: {
      color: "true_color",
      glyphs: "preferred",
      width: "unicode11",
      motion: "full",
    },
  },
  {
    name: "basic-ascii",
    expectedEnvironment: {
      TERM: "dumb",
      TERM_PROGRAM: "",
      COLORTERM: "",
      NO_COLOR: "1",
      HARNESS_TUI_REDUCED_MOTION: "1",
    },
    expectedClassification: {
      color: "no_color",
      glyphs: "ascii",
      width: "compact",
      motion: "reduced",
    },
  },
];

test("P1-04 contract drives the copied shipped binary through both production capability classifiers", () => {
  for (const variant of variants) {
    const options = parseArgs([
      "--scenario", "p1-04-responsive-feedback",
      "--evidence-dir", "/tmp/evidence",
      "--capability-variant", variant.name,
      "--cols", "120",
      "--rows", "40",
    ]);
    const contract = scenarioContract(options, {});

    assert.match(contract.command, /HARNESS_TUI_P1_04_SCENARIO=1/);
    assert.match(contract.command, /--exact p1_04_pty_helper/);
    assert.equal(contract.capabilityVariant, variant.name);
    assert.deepEqual(contract.environment, variant.expectedEnvironment);
    assert.deepEqual(contract.classification, variant.expectedClassification);
    assert.deepEqual(
      contract.actions.filter(({ kind }) => kind === "capture").map(({ state }) => state),
      ["following", "detached", "resize-final", "reduced-motion"],
    );
    assert.ok(contract.actions.some(({ kind }) => kind === "resize"));
    assert.equal(contract.actions.filter(({ kind }) => kind === "resize").at(-1).cols, 80);
    assert.equal(contract.actions.filter(({ kind }) => kind === "resize").at(-1).rows, 24);
  }
});

test("P1-04 resize action mutates the live PTY before resizing xterm and records state artifacts", async () => {
  const evidenceDir = await mkdtemp(join(tmpdir(), "harness-xterm-p1-04-actions-"));
  const order = [];
  const snapshot = {
    cols: 80,
    rows: 24,
    text: "Harness resized",
    cells: [{ row: 0, column: 0, chars: "H", width: 1 }],
    cursor: { x: 0, y: 0 },
  };
  const terminal = {
    async resize(cols, rows) {
      order.push(`xterm:${cols}x${rows}`);
      return { before: { cols: 120, rows: 40 }, after: { cols, rows } };
    },
    async snapshot() { return snapshot; },
    async capture(path) {
      await import("node:fs/promises").then(({ writeFile }) => writeFile(path, Buffer.from(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
        "base64",
      )));
      return snapshot;
    },
  };
  const pty = {
    async resize(cols, rows) {
      order.push(`pty:${cols}x${rows}`);
      return { before: { cols: 120, rows: 40 }, after: { cols, rows }, mechanism: "TIOCSWINSZ" };
    },
    async flush() {},
  };
  const interactions = [];

  try {
    const result = await executeActions({
      actions: [
        { kind: "resize", cols: 80, rows: 24 },
        { kind: "capture", state: "resize-final" },
      ],
      terminal,
      pty,
      interactions,
      evidenceDir,
    });

    assert.deepEqual(order, ["pty:80x24", "xterm:80x24"]);
    assert.equal(interactions[0].result.pty.mechanism, "TIOCSWINSZ");
    assert.equal(result.captures[0].state, "resize-final");
    assert.deepEqual(
      JSON.parse(await readFile(join(evidenceDir, "capture-001-resize-final.buffer.json"), "utf8")),
      snapshot,
    );
    assert.equal(
      await readFile(join(evidenceDir, "capture-001-resize-final.txt"), "utf8"),
      "Harness resized\n",
    );
  } finally {
    await rm(evidenceDir, { recursive: true, force: true });
  }
});

test("P1-04 Chromium xterm resize preserves structured cells at the final dimensions", async () => {
  const tempRoot = await mkdtemp(join(tmpdir(), "harness-xterm-p1-04-browser-"));
  const terminal = await openBrowserTerminal({
    browser: "/usr/bin/chromium",
    cols: 120,
    rows: 40,
    timeoutMs: 5000,
    title: "Harness P1-04 browser resize",
    profilePath: join(tempRoot, "profile"),
    onInput: () => false,
  });

  try {
    await terminal.write(Buffer.from("Harness Unicode 川山 가\r\n"));
    const receipt = await terminal.resize(80, 24);
    const snapshot = await terminal.snapshot();

    assert.deepEqual(receipt, {
      before: { cols: 120, rows: 40 },
      after: { cols: 80, rows: 24 },
    });
    assert.equal(snapshot.cols, 80);
    assert.equal(snapshot.rows, 24);
    assert.ok(snapshot.cells.some(({ chars, width }) => chars === "川" && width === 2));
    assert.ok(snapshot.cells.some(({ chars, width }) => chars === "가" && width === 2));
  } finally {
    await terminal.close();
    await rm(tempRoot, { recursive: true, force: true });
  }
});

test("P1-04 live PTY resize reports the kernel-applied final window size", async () => {
  const tempRoot = await mkdtemp(join(tmpdir(), "harness-xterm-p1-04-pty-"));
  let output = "";
  let readyResolve;
  let resizedResolve;
  const ready = new Promise((resolve) => { readyResolve = resolve; });
  const resized = new Promise((resolve) => { resizedResolve = resolve; });
  const pty = spawnHarnessPty({
    command: "sh -c 'echo Harness READY; while read -r line; do [ \"$line\" = size ] && printf \"RESIZED:%s\\n\" \"$(stty size)\"; [ \"$line\" = quit ] && exit 0; done'",
    cols: 120,
    rows: 40,
    cwd: tempRoot,
    sessionDir: join(tempRoot, "sessions"),
    tempRoot,
    environment: {},
    onOutput: async (bytes) => {
      output += bytes.toString("utf8");
      if (output.includes("Harness READY")) readyResolve();
      if (output.includes("RESIZED:24 80")) resizedResolve();
    },
  });

  try {
    await bounded(ready, 5000, "PTY ready");
    const receipt = await pty.resize(80, 24);
    pty.write("size\n");
    await bounded(resized, 5000, "PTY resize output");

    assert.equal(receipt.mechanism, "TIOCSWINSZ");
    assert.deepEqual(receipt.after, { cols: 80, rows: 24 });
    assert.match(output, /RESIZED:24 80/);
    pty.write("quit\n");
    assert.equal((await pty.waitForExit(5000)).code, 0);
  } finally {
    await pty.cleanup();
    await rm(tempRoot, { recursive: true, force: true });
  }
});

function bounded(promise, timeoutMs, label) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${label} timed out`)), timeoutMs);
    promise.then(
      (value) => { clearTimeout(timer); resolve(value); },
      (error) => { clearTimeout(timer); reject(error); },
    );
  });
}

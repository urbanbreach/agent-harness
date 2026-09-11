#!/usr/bin/env node
// Replay exact-clock renderer frames through the same xterm.js/font/browser as live PTY QA.
import { mkdir, mkdtemp, readFile, readdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { openBrowserTerminal } from "./lib/browser-terminal.mjs";
import { currentTree, fileReceipt } from "./lib/provenance.mjs";
import { validateEvidenceDir } from "./lib/security.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const input = resolve(process.argv[2] ?? "");
if (!process.argv[2] || !process.argv[3]) throw new Error("Usage: render-recorded-frames.mjs INPUT_DIR EVIDENCE_DIR");
const output = await validateEvidenceDir(process.argv[3], repoRoot);
await mkdir(output, { recursive: true });
const reference = process.argv[4] === "--reference-grok";
const sourceRoot = reference ? join(repoRoot, "inspirations/grok-build") : repoRoot;
const source = await currentTree(sourceRoot);
const producerMetadata = await readFile(join(input, "producer.json"), "utf8")
  .then(JSON.parse).catch((error) => { if (error.code === "ENOENT") return null; throw error; });
const names = (await readdir(input)).filter((name) => /^[a-z][a-z0-9-]*-\d+x\d+-(?:motion|reduced)-\d+ms\.ansi$/.test(name)).sort((a, b) => {
  const size = (name) => name.match(/(\d+x\d+)-(?:motion|reduced)/)[1];
  // Reset xterm between frames; reuse the browser while the geometry is the same.
  return size(a).localeCompare(size(b)) || a.localeCompare(b);
});
if (names.length === 0) throw new Error("No exact-clock welcome frames found");
const captures = [];
let terminal;
let dimensions;
try {
  for (const name of names) {
    const [, cols, rows] = name.match(/^[a-z][a-z0-9-]*-(\d+)x(\d+)-(?:motion|reduced)/);
    const next = `${cols}x${rows}`;
    if (next !== dimensions) {
      if (terminal) await terminal.close();
      terminal = await openBrowserTerminal({
        cols: Number(cols), rows: Number(rows), browser: "/usr/bin/chromium",
        profilePath: await mkdtemp(join(tmpdir(), "harness-xterm-frame-profile-")),
        title: reference ? "Grok Build reference renderer evidence" : "Harness exact-clock parity evidence", timeoutMs: 20000, onInput() {},
      });
      dimensions = next;
    }
    await terminal.write(Buffer.from("\x1bc"));
    await terminal.write(await readFile(join(input, name)));
    const stem = name.replace(/\.ansi$/, "");
    const png = join(output, `${stem}.png`);
    const snapshot = await terminal.capture(png);
    await writeFile(join(output, `${stem}.screen.json`), JSON.stringify(snapshot, null, 2));
    captures.push({ name: stem, input: await fileReceipt(join(input, name), input), image: await fileReceipt(png, output) });
  }
} finally {
  if (terminal) await terminal.close();
}
const after = await currentTree(sourceRoot);
if (source.hash !== after.hash) throw new Error("Source changed while recording frames");
await writeFile(join(output, "manifest.json"), JSON.stringify({
  schema: "harness-parity-renderer-xterm-v1", producer: reference ? "Grok Build reference" : "Harness", source,
  entrypoint: producerMetadata?.entrypoints ?? (reference ? "xai_grok_pager::views::welcome::render_welcome" : "harness_tui::ui::render_app"),
  producerMetadata,
  timing: producerMetadata?.timing ?? (reference
    ? { mode: "real elapsed time in reference renderer", samples: JSON.parse(await readFile(join(input, "runtime-timing.json"), "utf8")) }
    : { mode: "injected motion clock; separate from real-runtime PTY captures" }),
  emulator: "xterm.js 6.0.0", browser: "/usr/bin/chromium", captures,
}, null, 2));
process.stdout.write(`Captured ${captures.length} ${reference ? "reference runtime" : "exact-clock"} frames in ${output}\n`);

#!/usr/bin/env node
// Owned orchestration only: production adapters -> ANSI -> existing xterm.js recorder.
import { commandRecorder } from "./lib/command-recorder.mjs";
import { mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { currentTree, fileReceipt } from "./lib/provenance.mjs";
import { assertSecretFree, validateEvidenceDir } from "./lib/security.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const output = await validateEvidenceDir(process.argv[2] ?? join(root,
  ".omo/evidence/tool-parity-20260911/ordering-scenes"), root);
const sheetsOnly = process.argv.includes("--sheets-only");
const renderOnly = process.argv.includes("--render-only") || sheetsOnly;
const fixture = join(root, "scripts/qa/fixtures/tool-order-scenarios.json");
const model = JSON.parse(await readFile(fixture, "utf8"));
const scenarioFilter = process.argv.find(arg => arg.startsWith("--scenario="))?.slice("--scenario=".length) ?? "";
model.scenarios = model.scenarios.filter(scenario => scenario.name.includes(scenarioFilter));
if (!model.scenarios.length) throw new Error("no matching scenarios");
assertSecretFree(model);
const receipts = await readFile(join(output, "commands.json"), "utf8").then(JSON.parse).catch(error => {
  if (error.code === "ENOENT") return [];
  throw error;
});
await mkdir(join(output, "logs"), { recursive: true });
const selectedFixture = join(output, "scenarios.json");
await writeFile(selectedFixture, JSON.stringify(model, null, 2));
// Bind native compilation, ANSI production, and browser recording to the same source trees.
// The Harness tree includes the tracked Grok fixture outside the reference workspace.
const sourceRoots = [root, join(root, "inspirations/grok-build")];
const sourcesBefore = await Promise.all(sourceRoots.map(currentTree));
const inputs = await Promise.all([
  fixture, selectedFixture, fileURLToPath(import.meta.url),
  join(root, "scripts/qa/fixtures/grok-tool-order-capture.rs"),
  join(root, "crates/harness-tui/tests/tool_order_capture_test.rs"),
].map(path => fileReceipt(path, root)));
await writeFile(join(output, "source-start.json"), JSON.stringify({ sourcesBefore, inputs }, null, 2));

const run = commandRecorder({ cwd: root, output, receipts, timeout: 900000,
  env: { CARGO_BUILD_JOBS: "2", TZ: "UTC" },
});

if (!renderOnly) {
  await run("harness-capture", "cargo", ["nextest", "run", "--profile", "ci", "-p", "harness-tui",
    "--test", "tool_order_capture_test"], { HARNESS_TOOL_ORDER_ARTIFACT_DIR: join(output, "harness-ansi"),
      HARNESS_TOOL_ORDER_FILTER: scenarioFilter });
  await run("grok-capture", "cargo", ["run", "--manifest-path", "inspirations/grok-build/Cargo.toml",
    "-p", "xai-grok-pager", "--example", "harness_tool_order_capture", "--", selectedFixture, join(output, "grok-ansi")]);
}

for (const producer of ["harness", "grok"]) {
  const input = join(output, `${producer}-ansi`);
  for (const name of await readdir(input)) {
    if (!name.endsWith(".ansi") && !name.endsWith(".txt")) continue;
    // Reject rather than silently redact terminal bytes: unsafe payloads must never reach screenshots.
    assertSecretFree(await readFile(join(input, name)));
  }
  if (!sheetsOnly) {
    await run(`${producer}-xterm`, process.execPath, ["scripts/qa/render-recorded-frames.mjs", input,
      join(output, producer), ...(producer === "grok" ? ["--reference-grok"] : [])]);
  }
  // Reusing an already recorded frame is allowed only with its original successful manifest.
  // Never bypass the shared recorder's source-stability check or accept an unbound PNG.
  const manifest = JSON.parse(await readFile(join(output, producer, "manifest.json"), "utf8"));
  if (manifest.source.hash !== sourcesBefore[producer === "harness" ? 0 : 1].hash) {
    throw new Error(`${producer} manifest does not match the capture source tree`);
  }
  for (const capture of manifest.captures) {
    for (const [receipt, directory] of [[capture.input, input], [capture.image, join(output, producer)]]) {
      const actual = await fileReceipt(join(directory, receipt.path), directory);
      if (actual.sha256 !== receipt.sha256 || actual.bytes !== receipt.bytes) {
        throw new Error(`changed artifact ${actual.path}`);
      }
    }
  }
}

const harness = JSON.parse(await readFile(join(output, "harness-ansi/producer.json"), "utf8"));
const grok = JSON.parse(await readFile(join(output, "grok-ansi/producer.json"), "utf8"));
const grokNames = new Set(grok.captures.map(c => c.name));
const groups = new Map();
for (const capture of harness.captures) {
  if (!grokNames.delete(capture.name)) throw new Error(`unpaired capture ${capture.name}`);
  const key = capture.name.replace(/-\d+x\d+-motion-\d+ms$/, "");
  const pair = [];
  for (const producer of ["grok", "harness"]) {
    const png = join(output, producer, `${capture.name}.png`);
    if ((await stat(png)).size === 0) throw new Error(`empty image ${png}`);
    const screen = JSON.parse(await readFile(join(output, producer, `${capture.name}.screen.json`), "utf8"));
    assertSecretFree(screen);
    pair.push(png);
  }
  if (!groups.has(key)) groups.set(key, []);
  groups.get(key).push({ pair, capture });
}
if (grokNames.size) throw new Error("extra unpaired Grok captures");
await mkdir(join(output, "sheets"), { recursive: true });
// Contact sheets contain unchanged, full-resolution source PNGs. Left = Grok, right = Harness;
// row order follows fixture sizes (40, 120, 80). The originals remain the primary artifacts.
for (const [key, rows] of groups) {
  await run(`sheet-${key}`, "magick", ["montage", ...rows.flatMap(r => r.pair),
    "-tile", "2x", "-geometry", "+8+8", "-background", "none", join(output, "sheets", `${key}.png`)]);
}
await writeFile(join(output, "index.json"), JSON.stringify({ schema: model.schema,
  fixture: "scripts/qa/fixtures/tool-order-scenarios.json", pairedFrames: harness.captures.length,
  states: groups.size, orientation: "Grok left, Harness right; 40/120/80 rows", captures: harness.captures }, null, 2));
const sourcesAfter = await Promise.all(sourceRoots.map(currentTree));
const unchanged = sourcesBefore.every((source, i) => source.hash === sourcesAfter[i].hash);
await writeFile(join(output, "source-finish.json"), JSON.stringify({ sourcesAfter, unchanged }, null, 2));
if (!unchanged) throw new Error("Source changed between native capture and final paired sheets");
console.log(`${harness.captures.length} paired frames, ${groups.size} state sheets; visual disposition requires opening PNGs.`);

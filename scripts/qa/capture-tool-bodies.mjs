#!/usr/bin/env node
// Own-lane command monitor and paired xterm capture adapter. No backend or shared-library edits.
import { commandRecorder } from "./lib/command-recorder.mjs";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { assertSecretFree, validateEvidenceDir } from "./lib/security.mjs";
import { currentTree } from "./lib/provenance.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const root = await validateEvidenceDir(process.argv[3] ?? join(repo, ".omo/evidence/tool-parity-20260911/tool-scenes"), repo);
const mode = process.argv[2] ?? "--all";
if (!["--all", "--prepare", "--render-only", "--sheets-only"].includes(mode)) throw new Error("Unknown capture mode");
await mkdir(root, { recursive: true });
for (const dir of ["harness-ansi", "reference-ansi", "harness", "reference", "paired", "media", "logs"]) {
  await mkdir(await validateEvidenceDir(join(root, dir), repo), { recursive: true });
}
const sources = mode === "--all" ? await Promise.all([
  currentTree(repo), currentTree(join(repo, "inspirations/grok-build")),
]) : null;
const monitor = commandRecorder({ cwd: repo, output: root, timeout: 600000,
  env: { CARGO_BUILD_JOBS: "4" }, receiptFile: `commands-${mode.slice(2)}.json`,
});
async function allCommands(commands) {
  const settled = await Promise.allSettled(commands);
  const errors = settled.filter(result => result.status === "rejected").map(result => result.reason);
  if (errors.length) throw new AggregateError(errors, "Capture commands failed; all child exits observed");
}
const scenarioPath = join(root, "resolved-scenarios.json");
if (mode === "--prepare" || mode === "--all") {
  const config = JSON.parse(await readFile(join(repo, "scripts/qa/fixtures/tool-body-scenarios.json"), "utf8"));
  assertSecretFree(config);
  await monitor("asset-png", "magick", ["-size", "2x2", "xc:white", "-strip", join(root, "media/pixel.png")]);
  await monitor("asset-jpeg", "magick", [join(root, "media/pixel.png"), join(root, "media/pixel.jpg")]);
  // A real deterministic three-page PDF; rasterization/viewer launch is outside this pane capture.
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>",
    ...Array(3).fill("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 72 72] /Resources << >> >>"),
  ];
  let pdf = "%PDF-1.4\n";
  const offsets = [0];
  for (const [index, object] of objects.entries()) {
    offsets.push(Buffer.byteLength(pdf));
    pdf += `${index + 1} 0 obj\n${object}\nendobj\n`;
  }
  const xref = Buffer.byteLength(pdf);
  pdf += `xref\n0 ${offsets.length}\n0000000000 65535 f \n`;
  pdf += offsets.slice(1).map(offset => `${String(offset).padStart(10, "0")} 00000 n \n`).join("");
  pdf += `trailer\n<< /Size ${offsets.length} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  await writeFile(join(root, "media/document.pdf"), pdf);
  await monitor("asset-pdf", "pdfinfo", [join(root, "media/document.pdf")]);
  config.assets = {};
  for (const [name, mime] of [["media/pixel.png", "image/png"], ["media/pixel.jpg", "image/jpeg"], ["media/document.pdf", "application/pdf"]]) {
    const bytes = await readFile(join(root, name));
    const base64 = bytes.toString("base64");
    config.assets[name] = { base64, bytes: bytes.length, data_url: `data:${mime};base64,${base64}` };
  }
  await writeFile(scenarioPath, JSON.stringify(config, null, 2));
}
if (mode === "--all") {
  await allCommands([
    monitor("harness-capture", "cargo", ["nextest", "run", "--profile", "ci", "-p", "harness-tui", "--test", "tool_body_capture_test"], {
      TOOL_BODY_SCENARIOS: scenarioPath, TOOL_BODY_HARNESS_OUT: join(root, "harness-ansi"),
    }),
    monitor("reference-capture", "cargo", ["run", "--manifest-path", "inspirations/grok-build/Cargo.toml", "-p", "xai-grok-pager", "--example", "harness_tool_body_capture", "--", scenarioPath, join(root, "reference-ansi"), root]),
  ]);
}
if (mode === "--all" || mode === "--render-only") {
  // Keep the real xterm renderer and its full-cell/style/security capture contract intact.
  await allCommands([
    monitor("harness-xterm", "node", ["scripts/qa/render-recorded-frames.mjs", join(root, "harness-ansi"), join(root, "harness")]),
    monitor("reference-xterm", "node", ["scripts/qa/render-recorded-frames.mjs", join(root, "reference-ansi"), join(root, "reference"), "--reference-grok"]),
  ]);
}
if (mode !== "--prepare") {
  const config = JSON.parse(await readFile(scenarioPath, "utf8"));
  const expected = [];
  const boards = [];
  for (const scene of config.cases) {
    for (const alias of scene.aliases) {
      for (const width of config.widths) {
        const pairNames = [];
        for (const state of scene.states ?? config.states) {
          if (width === 80 && state !== "success-open") continue;
          const stem = `body-${scene.id}-${alias.replace(/[._]/g, "-")}-${state}-${width}x${config.height}-motion-0ms`;
          const pair = join(root, "paired", `${stem}.png`);
          const images = [];
          const crops = [];
          for (const side of ["harness", "reference"]) {
            const image = join(root, side, `${stem}.png`);
            const data = await readFile(image);
            if (data.length < 100) throw new Error(`Empty PNG: ${image}`);
            const screen = JSON.parse(await readFile(join(root, side, `${stem}.screen.json`), "utf8"));
            assertSecretFree(screen.text);
            images.push(image);
            const lines = screen.text.split("\n");
            const markerRow = lines.findIndex(line => /[◆◇◈⌄]/u.test(line));
            // An expanded long generic body can scroll its own header off-screen.
            // Preserve that real viewport result; never manufacture a replacement header.
            const first = markerRow >= 0 ? markerRow : lines.findIndex((line, row) => row > 0 && line.trim());
            if (first < 0) throw new Error(`Blank tool viewport: ${stem} (${side})`);
            const bottom = side === "harness" ? config.height - 10 : config.height - 2;
            let last = first;
            for (let row = first; row <= bottom; row++) if (lines[row]?.trim()) last = row;
            // PNG IHDR dimensions give exact pixels per row; no CSS reconstruction or rescaling.
            const pixelsWide = data.readUInt32BE(16);
            const pixelsHigh = data.readUInt32BE(20);
            const rowHeight = pixelsHigh / screen.rows;
            const rect = `${pixelsWide}x${(last - first + 2) * rowHeight}+0+${first * rowHeight}`;
            const crop = join(root, "paired", `${stem}-${side}-body.png`);
            await monitor(`crop-${expected.length}-${side}`, "magick", [image, "-crop", rect, "+repage", crop]);
            crops.push({ side, crop, first, last, rect, headerClipped: markerRow < 0 });
          }
          await monitor(`pair-${expected.length}`, "magick", [...images, "+append", pair]);
          const bodyPair = join(root, "paired", `${stem}-body.png`);
          await monitor(`body-pair-${expected.length}`, "magick", ["-background", "none", ...crops.map(item => item.crop), "+append", bodyPair]);
          pairNames.push(bodyPair);
          expected.push({ scene: scene.id, alias, state, width, audit: config.producers[scene.family].audit, pair, bodyPair, crops });
        }
        // Each board is made only of real paired PNGs; it does not reconstruct cells or styles.
        const name = `${scene.id}-${alias.replace(/[._]/g, "-")}-${width}.png`;
        await monitor(`board-${boards.length}`, "magick", [...pairNames, "-append", join(root, "paired", name)]);
        boards.push({ scene: scene.id, alias, width, image: name, states: (scene.states ?? config.states).filter(s => width !== 80 || s === "success-open") });
      }
    }
  }
  const actual = (await readdir(join(root, "harness"))).filter(name => name.endsWith(".screen.json"));
  if (actual.length !== expected.length) throw new Error(`Expected ${expected.length} paired cases; got ${actual.length}`);
  await writeFile(join(root, "pairs.json"), JSON.stringify({ expected, boards, unsupported: config.unsupported }, null, 2));
  process.stdout.write(`Paired ${expected.length} production frames across ${config.cases.length} payload cases\n`);
}
if (sources) {
  const after = await Promise.all([currentTree(repo), currentTree(join(repo, "inspirations/grok-build"))]);
  const stable = sources.every((source, index) => source.hash === after[index].hash);
  await writeFile(join(root, "fresh-source-guard.json"), JSON.stringify({ before: sources, after, stable }, null, 2));
  if (!stable) throw new Error("Source changed between producer build and paired capture; evidence is diagnostic only");
}

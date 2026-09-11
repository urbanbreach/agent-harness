#!/usr/bin/env node
// Compare production chat cells after removing Harness's surrounding shell margins.
import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";

const [harness, reference, output] = process.argv.slice(2);
if (!harness || !reference || !output) {
  throw new Error("Usage: compare-chat-frames.mjs HARNESS_DIR REFERENCE_DIR REPORT.json");
}
const scenes = ["thinking", "thinkingcode", "running", "runningopen", "success", "successopen", "failed", "failedopen", "answer", "context", "contextopen", "searchsources", "commands", "commandsopen"];
const times = [0, 330, 660];
const read = async (root, name) => JSON.parse(await readFile(join(root, name), "utf8"));
const fields = ["chars", "width", "fgColor", "bgColor", "fgColorMode", "bgColorMode"];
const signature = (cell) => fields.map((field) => cell[field]);
const normal = [];
for (const scene of scenes) {
  for (const time of times) {
    const name = `chat-${scene}-120x40-motion-${time}ms.screen.json`;
    const [actual, expected] = await Promise.all([read(harness, name), read(reference, name)]);
    const lastRow = Math.max(...expected.cells.map((cell) => cell.row));
    const actualCells = new Map(actual.cells
      .filter((cell) => cell.row >= 3 && cell.row <= lastRow + 3 && cell.column >= 2 && cell.column < 118)
      .map((cell) => [`${cell.row - 3}:${cell.column - 2}`, signature(cell)]));
    const expectedCells = new Map(expected.cells.map((cell) => [`${cell.row}:${cell.column}`, signature(cell)]));
    assert.deepEqual(actualCells, expectedCells, `${name}: chat text, geometry, or colors differ`);
    assert(expected.cells.some((cell) => cell.fgRgb), `${name}: RGB SGR is missing`);
    normal.push({ name, cells: expectedCells.size, identical: true });
  }
}

const reduced = [];
for (const scene of scenes) {
  for (const size of ["40x24", "80x24", "120x40"]) {
    const frames = await Promise.all(times.map((time) => read(harness, `chat-${scene}-${size}-reduced-${time}ms.screen.json`)));
    // Exclude the runtime clock and composer below the transcript viewport.
    const transcript = (frame) => frame.cells.filter((cell) => cell.row >= 3 && cell.row < frame.rows - 9);
    for (const frame of frames.slice(1)) {
      assert.deepEqual(transcript(frame), transcript(frames[0]), `${scene}/${size}: reduced motion changed chat cells`);
    }
    reduced.push({ scene, size, static: true });
  }
}
const manifests = await Promise.all([read(harness, "manifest.json"), read(reference, "manifest.json")]);
const report = {
  scope: "Normal-motion nonblank transcript cells at 120x40: text, width, position, foreground and background. Harness origin translated by (-2,-3); shell chrome excluded. Reduced-motion stability checked independently at all three sizes; the reference fixture freezes its tick rather than implementing Harness's static accessibility policy.",
  producers: manifests.map(({ producer, source, captures }) => ({ producer, source, captures: captures.length })),
  normal,
  reduced,
  comparedCells: normal.reduce((sum, pair) => sum + pair.cells, 0),
};
await mkdir(dirname(output), { recursive: true });
await writeFile(output, `${JSON.stringify(report, null, 2)}\n`);
process.stdout.write(`PASS ${normal.length} paired frames / ${report.comparedCells} cells; ${reduced.length} reduced-motion cases\n`);

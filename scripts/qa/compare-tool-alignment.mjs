#!/usr/bin/env node
// Compare layout anchors from the production renderers after xterm.js parsing.
import assert from "node:assert/strict";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";

const [harness, reference, output] = process.argv.slice(2);
if (!harness || !reference || !output) {
  throw new Error("Usage: compare-tool-alignment.mjs HARNESS_DIR REFERENCE_DIR REPORT.json");
}
const read = async (root, name) => JSON.parse(await readFile(join(root, name), "utf8"));
const names = (await readdir(harness)).filter((name) => /^align-.*\.screen\.json$/.test(name)).sort();
assert.equal(names.length, 481, "incomplete lifecycle matrix");
const markers = new Set(["◆", "◈", "›", "⌄"]);
const problems = [];
const pairs = [];
function anchors(frame) {
  const user = frame.cells.find((cell) => cell.chars === "❯");
  assert(user, "user prompt missing");
  const tools = frame.cells.filter((cell) => markers.has(cell.chars) && cell.row > user.row && cell.row < frame.rows - 9);
  assert(tools.length > 0, "tool marker missing");
  return { user, tools };
}
function position(frame, token, header) {
  const rows = frame.text.split("\n");
  // Skip the immediate command/description row; require actual output.
  for (let row = header.row + 2; row < frame.rows - 9; row++) {
    const column = rows[row].indexOf(token);
    if (column >= 0) return { column, offset: row - header.row };
  }
  return null;
}
// Body anchors intentionally compare layout, not family-specific tool names.
const bodies = {
  read: ["1  ready", "1  ready"],
  write: ["Initial tool test content.", "Initial tool test content."],
  edit: ["Initial tool test content.", "Initial tool test content."],
  bash: ["ready", "ready"],
  grep: ["1  ready", "1  ready"],
  glob: ["demo.txt", "demo.txt"],
  list: ["ready", "ready"],
  websearch: ["ready", "ready"],
  "mcp-fixture-inspect": ["ready", "ready"],
  question: ["→ First", "→ First"],
};
for (const name of names) {
  const [actual, expected] = await Promise.all([read(harness, name), read(reference, name)]);
  const a = anchors(actual);
  const e = anchors(expected);
  const actualHeader = a.tools[0];
  const expectedHeader = e.tools[0];
  const columns = a.tools.map((cell) => cell.column);
  const expectedColumn = expectedHeader.column;
  if (columns.some((column) => column !== expectedColumn) || a.user.column !== e.user.column) {
    problems.push({ name, kind: "marker columns", columns, expectedColumn });
  }
  const offset = actualHeader.row - a.user.row;
  const expectedOffset = expectedHeader.row - e.user.row;
  if (offset !== expectedOffset) problems.push({ name, kind: "header spacing", offset, expectedOffset });
  const pair = { name, markerColumn: expectedColumn, headerOffset: offset };
  const family = name.match(/^align-(.*)-succeeded-open-120x40-motion-0ms\.screen\.json$/)?.[1];
  if (bodies[family]) {
    const [actualToken, expectedToken] = bodies[family];
    const actualBody = position(actual, actualToken, actualHeader);
    const expectedBody = position(expected, expectedToken, expectedHeader);
    pair.body = { family, actual: actualBody, reference: expectedBody };
    if (!actualBody || !expectedBody || actualBody.column !== expectedBody.column || actualBody.offset !== expectedBody.offset) {
      problems.push({ name, kind: "body anchor", ...pair.body });
    }
  }
  pairs.push(pair);
}
const manifests = await Promise.all([read(harness, "manifest.json"), read(reference, "manifest.json")]);
const report = {
  scope: "13 tool families across streaming, queued, waiting, running, animation tick, success/failure, disclosure and selection at 40x24, 80x24 and 120x40. Permission waits use 120x40 to keep the transcript visible. Marker columns and vertical offsets relative to the user prompt are compared in all 481 pairs. Ten expanded body anchors are compared at 120x40. Shell chrome, tool-specific wording and pixel/color identity are outside this geometry check.",
  producers: manifests.map(({ producer, source, captures }) => ({ producer, source, captures: captures.length })),
  passed: problems.length === 0,
  pairs,
  problems,
};
await mkdir(dirname(output), { recursive: true });
await writeFile(output, `${JSON.stringify(report, null, 2)}\n`);
assert.equal(problems.length, 0, JSON.stringify(problems, null, 2));
process.stdout.write(`PASS ${pairs.length} lifecycle pairs / ${pairs.filter((pair) => pair.body).length} expanded body anchors\n`);

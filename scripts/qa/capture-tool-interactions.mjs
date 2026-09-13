#!/usr/bin/env node
// Native Rust producers -> exact ANSI -> existing real xterm.js renderer.
import { commandRecorder } from "./lib/command-recorder.mjs";
import { mkdir, readFile, readdir, writeFile, stat } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { assertSecretFree, validateEvidenceDir } from "./lib/security.mjs";
import { currentTree, fileReceipt } from "./lib/provenance.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const owned = join(root, ".omo/evidence/tool-parity-20260911/interaction-scenes");
const requested = await validateEvidenceDir(process.argv[2] ?? owned, root);
const output = await validateEvidenceDir(requested === owned ? join(owned, "fresh") : requested, root);
const renderOnly = process.argv.includes("--render-only");
const scenesOnly = process.argv.includes("--scenes-only");
const scenarios = join(root, "scripts/qa/fixtures/tool-interaction-scenarios.json");
const data = JSON.parse(await readFile(scenarios, "utf8"));
assertSecretFree(data);
await mkdir(output, { recursive: true });
const sourceRoots = [root, join(root, "inspirations/grok-build")];
const sourcesBefore = await Promise.all(sourceRoots.map(currentTree));
const inputs = await Promise.all([scenarios, fileURLToPath(import.meta.url),
  join(root, "scripts/qa/fixtures/grok-tool-interaction-capture.rs"),
  join(root, "crates/harness-tui/tests/tool_interaction_capture_test.rs"),
].map(path => fileReceipt(path, root)));
await writeFile(join(output, "source-start.json"), JSON.stringify({ sourcesBefore, inputs }, null, 2));
const receipts = await readFile(join(output, "commands.json"), "utf8").then(JSON.parse).catch((error) => {
  if (error.code === "ENOENT") return [];
  throw error;
});
const native = JSON.parse(await readFile(join(root, data.native_probes.fixture), "utf8"));
assertSecretFree(native);

const run = commandRecorder({ cwd: root, output, receipts, timeout: 1200000 });

if (!renderOnly) {
  await run("harness-build", "cargo", ["nextest", "run", "--profile", "ci", "-p", "harness-tui", "--test", "tool_interaction_capture_test"], {
    HARNESS_TOOL_INTERACTION_ARTIFACT_DIR: join(output, "harness-ansi"),
  });
  await run("reference-build", "cargo", ["run", "--manifest-path", "inspirations/grok-build/Cargo.toml", "-p", "xai-grok-pager", "--features", "test-support", "--example", "harness_tool_interaction_capture", "--", join(output, "reference-ansi"), scenarios]);
  if (!scenesOnly) {
    await run("harness-native-input", "cargo", ["nextest", "run", "--profile", "ci", "-p", "harness-tui", "--lib", "-E", "test(native_tool_scroll_inputs_follow_reference_line_and_half_page_steps) | test(native_tool_header_mouse_capture_uses_rendered_hit_targets) | test(native_tool_viewer_capture_uses_the_enter_handler)"], {
    HARNESS_TOOL_RUNTIME_HARNESS_DIR: join(output, "native-harness-ansi"),
  });
    await run("reference-native-input", "cargo", ["nextest", "run", "--manifest-path", "inspirations/grok-build/Cargo.toml", "-p", "xai-grok-pager", "--lib", "-E", "test(native_tool_)", "--test-threads", "1"], {
    HARNESS_TOOL_RUNTIME_REFERENCE_DIR: join(output, "native-reference-ansi"),
  });
  }
}
for (const side of scenesOnly ? [] : ["harness", "reference"]) {
  const path = join(output, `native-${side}-ansi`, "producer.json");
  const producer = JSON.parse(await readFile(path, "utf8"));
  await writeFile(path, JSON.stringify({
    entrypoints: side === "reference"
      ? ["AppView::handle_input", "app::dispatch::dispatch", "AgentView::handle_scrollback_click", "AgentView::draw"]
      : ["AppState::ingest_event", "AppState::handle_key", "AppState::handle_mouse", "render_app"],
    timing: {
      mode: "Synchronous native handlers; settled tool; no animated-chrome parity claim",
      fixture: data.native_probes.fixture,
      header_clicks: side === "reference"
        ? { relative_ms: [10, 20, 30, 40], target: "stable semantic entry index" }
        : { added_ms: [60010, 60020, 60030, 60040], target: "original screen cell; NOT retargeted after layout changes" },
      limitation: "Header frames 2-4 are NOT matched multi-click evidence: clocks and hit ownership diverge",
    },
    states: Array.isArray(producer) ? producer : producer.states,
  }, null, 2));
}
for (const side of scenesOnly ? ["harness", "reference"] : ["harness", "reference", "native-harness", "native-reference"]) {
  const input = join(output, `${side}-ansi`);
  for (const name of await readdir(input)) {
    if (!name.endsWith(".ansi") && !name.endsWith(".json") && !name.endsWith(".txt")) continue;
    assertSecretFree(await readFile(join(input, name)));
  }
  await run(`${side}-xterm`, process.execPath, ["scripts/qa/render-recorded-frames.mjs", input, join(output, side), ...(side.endsWith("reference") ? ["--reference-grok"] : [])]);
}
const pairs = [];
const referenceOnly = [];
for (const scenario of data.cases) {
  for (const [width, height] of data.sizes) {
    const stem = `interaction-${scenario.name}-${width}x${height}-motion-0ms`;
    for (const side of scenario.reference_only ? ["reference"] : ["harness", "reference"]) {
      if ((await stat(join(output, side, `${stem}.png`))).size === 0) throw new Error(`Empty PNG: ${stem}`);
      const screen = JSON.parse(await readFile(join(output, side, `${stem}.screen.json`), "utf8"));
      assertSecretFree(screen);
    }
    (scenario.reference_only ? referenceOnly : pairs).push({ stem, ids: scenario.ids, prefix: "", disposition: "UNREVIEWED: consult interaction-scenes.md for opened-image findings" });
  }
}
for (const [width, height] of scenesOnly ? [] : native.sizes) {
  for (const name of ["scroll-before", ...native.keys.map((key) => `scroll-${key}`), ...[1, 2, 3, 4].map((count) => `mouse-header-${count}`), ...native.viewer_cases.map((item) => `viewer-${item.name}`)]) {
    const stem = `runtime-${name}-${width}x${height}-motion-0ms`;
    for (const side of ["harness", "reference"]) {
      if ((await stat(join(output, `native-${side}`, `${stem}.png`))).size === 0) throw new Error(`Empty native PNG: ${stem}`);
      assertSecretFree(JSON.parse(await readFile(join(output, `native-${side}`, `${stem}.screen.json`), "utf8")));
    }
    pairs.push({ stem, ids: name.startsWith("scroll") ? data.native_probes.scroll_ids : data.native_probes.header_ids,
      prefix: "native-", disposition: name.startsWith("viewer-")
        ? "Production Enter/key handlers and modal rendering; compare modal and footer independently of the underlying chat context"
        : "UNREVIEWED: header click is not semantic text-selection evidence" });
  }
}
for (const pair of pairs) {
  await run(`pair-${pair.stem}`, "magick", [join(output, `${pair.prefix}reference`, `${pair.stem}.png`),
    join(output, `${pair.prefix}harness`, `${pair.stem}.png`), "+append", join(output, `pair-${pair.stem}.png`)]);
}
await writeFile(join(output, "pairs.json"), JSON.stringify({ schema: data.schema, pairs, referenceOnly, blockers: data.required_blocker_cases }, null, 2));
const sourcesAfter = await Promise.all(sourceRoots.map(currentTree));
const unchanged = sourcesBefore.every((source, index) => source.hash === sourcesAfter[index].hash);
await writeFile(join(output, "source-finish.json"), JSON.stringify({ sourcesAfter, unchanged }, null, 2));
if (!unchanged) throw new Error("Source changed while compiling or recording interaction evidence");
process.stdout.write(`Produced ${pairs.length} paired ANSI / xterm.js PNG / full-cell screen JSON cases\n`);

// Captures the hand-authored compaction surfaces through a PTY, xterm.js and Chromium.
import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, open, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { openBrowserTerminal } from "./browser-terminal.mjs";
import { buildSurfaceScript } from "./compaction-surfaces-script.mjs";
import { prepareHarnessWorkspace, spawnHarnessPty } from "./pty-session.mjs";
import { assertSecretFree, safePtyEnvironment, validateEvidenceDir } from "./security.mjs";

const quote = value => `'${String(value).replaceAll("'", "'\\''")}'`;

export function resolveCaptureBinary(root, target) {
  const listing = JSON.parse(execFileSync("cargo", ["nextest", "list", "-p", "harness-tui", "--test", "manual_live_turn_visual_capture_test", "--message-format", "json", "--list-type", "binaries-only"], {
    cwd: root, env: { ...safePtyEnvironment(process.env), CARGO_TARGET_DIR: target }, encoding: "utf8", maxBuffer: 16 * 1024 * 1024,
  }));
  const binary = Object.values(listing["rust-binaries"]).find(item => item["binary-name"] === "manual_live_turn_visual_capture_test")?.["binary-path"];
  if (!binary) throw new Error("capture executable not found");
  return binary;
}

export async function captureSurfaces({ output, binary, cols = 120, rows = 36 }) {
  const { steps, frames } = buildSurfaceScript();
  assertSecretFree(steps);
  await writeFile(join(output, "surface-events.json"), JSON.stringify(steps, null, 2));
  const manifest = { cols, rows, sequence: "surface-events.json", frames: [], failures: [] };
  const tempRoot = await mkdtemp(join(tmpdir(), "harness-surfaces-"));
  const workspace = await prepareHarnessWorkspace(tempRoot);
  const fifo = join(tempRoot, "events.fifo");
  execFileSync("mkfifo", [fifo]);
  let terminal;
  let pty;
  let stream;
  try {
    terminal = await openBrowserTerminal({ cols, rows, browser: "/usr/bin/chromium", profilePath: join(tempRoot, "browser"), title: "Harness compaction surfaces", captureAllCells: true, onInput: data => pty?.write(data) });
    pty = spawnHarnessPty({ cols, rows, tempRoot, ...workspace, cwd: workspace.workspace, command: `${quote(binary)} --exact capture_compaction_state_from_environment --nocapture >/dev/null`, disableAnimations: false, environment: { TERM: "xterm-256color", COLORTERM: "truecolor", HARNESS_TUI_COMPACTION_STREAM: fifo }, onOutput: bytes => terminal.write(bytes) });
    const started = performance.now();
    stream = await open(fifo, "w");
    for (const step of steps) {
      await stream.write(`${JSON.stringify(step)}\n`);
      if (step.frame_id === undefined) continue;
      const spec = frames[step.frame_id];
      await terminal.waitForTitle(`Harness compaction frame ${step.frame_id}`);
      const file = `frames/surface-${String(step.frame_id).padStart(2, "0")}-${spec.name}.png`;
      const snapshot = await terminal.capture(join(output, file));
      for (const text of spec.expect) if (!snapshot.text.includes(text)) manifest.failures.push(`${file}: missing ${JSON.stringify(text)}`);
      for (const text of spec.absent) if (snapshot.text.includes(text)) manifest.failures.push(`${file}: unexpected ${JSON.stringify(text)}`);
      manifest.frames.push({ file, name: spec.name, at_ms: spec.at_ms, captured_after_ms: Math.round(performance.now() - started), expected: spec.expect, absent: spec.absent, text: snapshot.text });
    }
    await stream.close(); stream = null;
    const result = await pty.waitForExit(10000);
    if (result.code !== 0) manifest.failures.push(`PTY exit ${result.code}: ${pty.stderr().slice(0, 400)}`);
    assertSecretFree(pty.raw());
    await writeFile(join(output, "surfaces.ansi"), pty.raw());
  } finally {
    if (stream) await stream.close();
    if (pty) await pty.cleanup();
    if (terminal) await terminal.close();
    await rm(tempRoot, { recursive: true, force: true });
  }
  return manifest;
}

// Iteration entry: node scripts/qa/lib/compaction-surfaces.mjs /tmp/harness-xterm-surfaces
if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
  const output = await validateEvidenceDir(process.argv[2], root);
  await mkdir(join(output, "frames"), { recursive: true });
  const target = process.env.CARGO_TARGET_DIR ?? "/home/urbanbreach/.cache/harness-targets/Captures";
  const manifest = await captureSurfaces({ output, binary: resolveCaptureBinary(root, target) });
  await writeFile(join(output, "manifest.json"), JSON.stringify(manifest, null, 2));
  console.log(`${manifest.frames.length} frames, ${manifest.failures.length} failures`);
  for (const failure of manifest.failures) console.log(failure);
}

#!/usr/bin/env node
// Render terminal/TUI evidence through a REAL xterm.js terminal in a browser.
//
// A command runs in a real pty (node-pty), streams into xterm.js inside headless
// Chrome, is driven with scripted keystrokes THROUGH the browser terminal, and is
// screenshotted true-color. This replaces the old tmux capture-pane + hand-rolled
// ANSI-to-HTML path, which degraded color and never rendered a real terminal.

import { createRequire } from "node:module";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { captureLive, captureRawPty } from "./xterm-live-terminal.mjs";
import { BUILT_IN_REDACTION_RULE_COUNT, compileRedactions, redactEvidence } from "./web-terminal-redaction.mjs";
import { stripAnsi } from "./strip-ansi.mjs";

const require = createRequire(
  process.env.TUI_FIDELITY_NODE_MODULES
    ? join(process.env.TUI_FIDELITY_NODE_MODULES, "package.json")
    : import.meta.url,
);

const HELP = `web-terminal-visual-qa

Render terminal/TUI evidence through a REAL xterm.js terminal captured in a browser (true color, no tmux).
First-party path: scripts/tui-parity/ (promoted from the proven capture lab).

Usage:
  node scripts/tui-parity/web-terminal-visual-qa.mjs --title "Harness TUI" --command "harness" --evidence-dir artifacts/qa-evidence/run
  node scripts/tui-parity/web-terminal-visual-qa.mjs --title "TUI" --command "my-tui" --input "{Down}" --input "{Down}" --input "{Enter}" --evidence-dir artifacts/qa-evidence/run
  node scripts/tui-parity/web-terminal-visual-qa.mjs --title "Replay" --from-file pane.ansi --evidence-dir artifacts/qa-evidence/run
  node scripts/tui-parity/web-terminal-visual-qa.mjs --self-test

Inputs:
  --command <command>    Run in a real node-pty and render live in xterm.js. The color path is xterm.js - NEVER tmux.
  --from-file <path>     Render an existing raw terminal byte stream through xterm.js (replay; no interaction).
  --input <token>        Scripted interaction, repeatable, applied in order THROUGH the browser terminal.
                         Literal text is typed; {Enter} {Tab} {Escape} {ArrowDown} {Ctrl+C} etc. are pressed as keys.
  --action <json>        Tagged action object, repeatable: waitForText, wait, input, key, resize, mouse, checkpoint.
  --actions-file <path>  JSON array of tagged actions. Entries run in file/CLI order.
  --cwd <path>           Working directory for --command. Default: current directory.
  --cols <n> / --rows <n>  Terminal geometry. Default: 120 x 32.
  --font-size <n>        xterm.js fontSize in CSS px. Default: 15 (freeze receipt may note 14).
  --font-family <css>    xterm.js fontFamily CSS. Default: Menlo, DejaVu Sans Mono, Noto Sans Mono CJK KR, monospace.
  --terminal-background <#rrggbb>  xterm.js reset background. Default: #141414.
  --dwell-ms <n>         Milliseconds to let the TUI settle after input before capture. Default: 1500.
  --key-delay-ms <n>     Pause between --input tokens. Default: 120.
  --evidence-dir <path>  Directory for the final frame, cleanup.json, and checkpoints/<name>/ frame artifacts.
  --chrome-bin <path>    Chrome/Chromium executable (else auto-detect or CHROME_BIN).
  --source-label <text>  Safe label for --command metadata. The raw command is never written to metadata.
  --term <value>         Child TERM. Default: xterm-256color. Use "unset" to remove it.
  --colorterm <value>    Child COLORTERM. Default: truecolor. Use "unset" to remove it.
  --no-color <value>     Child NO_COLOR value. Default: unset. Use "unset" to remove it explicitly.
  --unicode-version <6|11>  xterm.js Unicode width mode. Default: 11.
  --redact <literal>     Literal secret to mask in ALL evidence, PNG included. Repeatable.
  --redact-regex <expr>  JS regex source to mask in ALL evidence, PNG included. Repeatable.
  --no-browser           Skip xterm.js/Chrome; capture the raw pty stream only (no PNG). For chrome-less CI.

Action objects:
  {"waitForText":{"text":"Ready","timeoutMs":5000}}  {"wait":{"ms":100}}  {"input":{"text":"hello"}}
  {"key":{"key":"Enter","modifiers":{"shift":true,"alt":false,"ctrl":false}}}  {"resize":{"cols":80,"rows":24}}
  {"mouse":{"kind":"move","col":4,"row":2}}  {"mouse":{"kind":"click","col":4,"row":2}}
  {"mouse":{"kind":"wheel","deltaY":-100}}
  {"mouse":{"kind":"drag","from":{"col":2,"row":2},"to":{"col":12,"row":2}}}  {"checkpoint":{"name":"settled"}}

Secret handling:
  Text evidence and the screenshot are redacted before anything is written. When a redaction rule matches, the
  masked stream is re-rendered so the PNG never shows the secret. The raw --command string is never stored.
`;

function parsePositiveInt(name, value) {
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed <= 0) throw new Error(`${name} must be a positive integer`);
  return parsed;
}

function parseNonNegativeInt(name, value) {
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 0) throw new Error(`${name} must be a non-negative integer`);
  return parsed;
}

function parseHexColor(name, value) {
  if (!/^#[0-9a-fA-F]{6}$/.test(value)) throw new Error(`${name} must be a #rrggbb color`);
  return value.toLowerCase();
}

function parseAction(value, source) {
  let action;
  try { action = typeof value === "string" ? JSON.parse(value) : value; }
  catch (error) { throw new Error(`${source} must be valid JSON: ${error.message}`); }
  if (!action || typeof action !== "object" || Array.isArray(action)) throw new Error(`${source} must be an action object`);
  const tags = Object.keys(action);
  if (tags.length !== 1) throw new Error(`${source} must contain exactly one action tag`);
  const [tag] = tags;
  const payload = action[tag];
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) throw new Error(`${source}.${tag} must be an object`);
  if (tag === "checkpoint") {
    if (typeof payload.name !== "string" || !/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(payload.name)) {
      throw new Error("checkpoint name must be a safe relative leaf name");
    }
  } else if (tag === "waitForText") {
    if (typeof payload.text !== "string" || payload.text.length === 0) throw new Error("waitForText.text must be non-empty");
    parsePositiveInt("waitForText.timeoutMs", payload.timeoutMs);
  } else if (tag === "wait") {
    if (!Number.isInteger(payload.ms) || payload.ms < 0) throw new Error("wait.ms must be a non-negative integer");
  } else if (tag === "input") {
    if (typeof payload.text !== "string") throw new Error("input.text must be a string");
  } else if (tag === "key") {
    if (typeof payload.key !== "string" || payload.key.length === 0) throw new Error("key.key must be non-empty");
    if (payload.modifiers !== undefined && (!payload.modifiers || typeof payload.modifiers !== "object" || Array.isArray(payload.modifiers))) {
      throw new Error("key.modifiers must be an object");
    }
    for (const [name, enabled] of Object.entries(payload.modifiers || {})) {
      if (!["shift", "alt", "ctrl", "meta"].includes(name) || typeof enabled !== "boolean") {
        throw new Error("key.modifiers supports boolean shift, alt, ctrl, and meta fields");
      }
    }
  } else if (tag === "resize") {
    if (!Number.isInteger(payload.cols) || payload.cols <= 0 || !Number.isInteger(payload.rows) || payload.rows <= 0) {
      throw new Error("resize cols and rows must be positive integers");
    }
  } else if (tag === "mouse") {
    if (!["move", "click", "wheel", "drag"].includes(payload.kind)) throw new Error("mouse.kind must be move, click, wheel, or drag");
  } else {
    throw new Error(`unknown action tag: ${tag}`);
  }
  return action;
}

function parseActionsFile(path) {
  let values;
  try { values = JSON.parse(readFileSync(path, "utf8")); }
  catch (error) { throw new Error(`--actions-file must contain valid JSON: ${error.message}`); }
  if (!Array.isArray(values)) throw new Error("--actions-file must contain a JSON array");
  return values.map((value, index) => parseAction(value, `--actions-file[${index}]`));
}

function parseArgs(argv) {
  const args = { cols: 120, rows: 32, fontSize: 15, fontFamily: undefined, terminalBackground: "#141414", dwellMs: 1500, keyDelayMs: 120, preDwellMs: 400, cwd: process.cwd(), browser: true, redactions: [], redactRegexes: [], inputs: [], actions: [], term: "xterm-256color", colorterm: "truecolor", noColor: "unset", unicodeVersion: "11" };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--help" || arg === "-h") return { ...args, help: true };
    if (arg === "--self-test") return { ...args, selfTest: true };
    if (arg === "--no-browser") { args.browser = false; continue; }
    const next = argv[i + 1];
    if (!next) throw new Error(`missing value for ${arg}`);
    i += 1;
    if (arg === "--title") args.title = next;
    else if (arg === "--from-file") args.fromFile = next;
    else if (arg === "--command") args.command = next;
    else if (arg === "--cwd") args.cwd = next;
    else if (arg === "--evidence-dir") args.evidenceDir = next;
    else if (arg === "--chrome-bin") args.chromeBin = next;
    else if (arg === "--source-label") args.sourceLabel = next;
    else if (arg === "--input") args.inputs.push(next);
    else if (arg === "--action") args.actions.push(parseAction(next, "--action"));
    else if (arg === "--actions-file") args.actions.push(...parseActionsFile(next));
    else if (arg === "--redact") args.redactions.push(next);
    else if (arg === "--redact-regex") args.redactRegexes.push(next);
    else if (arg === "--term") args.term = next;
    else if (arg === "--colorterm") args.colorterm = next;
    else if (arg === "--no-color") args.noColor = next;
    else if (arg === "--unicode-version") {
      if (!["6", "11"].includes(next)) throw new Error("--unicode-version must be 6 or 11");
      args.unicodeVersion = next;
    }
    else if (arg === "--cols") args.cols = parsePositiveInt(arg, next);
    else if (arg === "--rows") args.rows = parsePositiveInt(arg, next);
    else if (arg === "--font-size") args.fontSize = parsePositiveInt(arg, next);
    else if (arg === "--font-family") args.fontFamily = next;
    else if (arg === "--terminal-background") args.terminalBackground = parseHexColor(arg, next);
    else if (arg === "--dwell-ms") args.dwellMs = parseNonNegativeInt(arg, next);
    else if (arg === "--key-delay-ms") args.keyDelayMs = parseNonNegativeInt(arg, next);
    else if (arg === "--pre-dwell-ms") args.preDwellMs = parseNonNegativeInt(arg, next);
    else throw new Error(`unknown argument: ${arg}`);
  }
  return args;
}

function requireArgs(args) {
  if (!args.evidenceDir) throw new Error("--evidence-dir is required");
  if (!args.title) throw new Error("--title is required");
  if (args.fromFile && args.command) throw new Error("choose exactly one of --from-file or --command");
  if (!args.fromFile && !args.command) throw new Error("choose --from-file or --command");
  if (!args.browser && args.actions.length > 0) throw new Error("table actions require browser capture");
  const names = args.actions.flatMap((action) => action.checkpoint ? [action.checkpoint.name] : []);
  if (new Set(names).size !== names.length) throw new Error("checkpoint names must be unique");
}

function sourceMetadata(args) {
  if (args.fromFile) return { kind: "file-replay", path: resolve(args.fromFile) };
  return { kind: "command", label: args.sourceLabel || "redacted command" };
}

function captureFileRaw(content) {
  return { pngBuffer: null, screenText: stripAnsi(content), rawStream: content, connector: "file-raw", cleanup: "file replay; no process" };
}

function redactOsc52Payloads(stream) {
  return stream.replace(/\x1b\]52;([^;]*);[^\x07\x1b]*(\x07|\x1b\\)/g, "\x1b]52;$1;[REDACTED]$2");
}

function redactMetadataValue(value, redactStream) {
  if (typeof value === "string") return redactStream(value);
  if (Array.isArray(value)) return value.map((item) => redactMetadataValue(item, redactStream));
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, redactMetadataValue(item, redactStream)]));
  }
  return value;
}

function writeFrameFiles(directory, frame, redactStream) {
  mkdirSync(directory, { recursive: true });
  const textPath = join(directory, "terminal.txt");
  const ansiPath = join(directory, "terminal-ansi.txt");
  const pngPath = join(directory, "terminal.png");
  const safeText = redactStream(frame.screenText);
  writeFileSync(textPath, safeText.endsWith("\n") ? safeText : `${safeText}\n`, "utf8");
  writeFileSync(ansiPath, redactStream(frame.rawStream), "utf8");
  if (frame.pngBuffer) writeFileSync(pngPath, frame.pngBuffer);
  return { png: frame.pngBuffer ? pngPath : null, text: textPath, ansi: ansiPath };
}

async function run(args) {
  const evidenceDir = resolve(args.evidenceDir);
  mkdirSync(evidenceDir, { recursive: true });
  const rules = compileRedactions(args);
  const redactStream = (s) => redactEvidence(redactOsc52Payloads(s), rules);
  const fromFile = args.fromFile ? readFileSync(args.fromFile, "utf8") : undefined;

  let cap;
  if (args.browser) cap = await captureLive({ ...args, fromFile, redactStream });
  else if (fromFile !== undefined) cap = captureFileRaw(fromFile);
  else cap = await captureRawPty(args);

  const finalFiles = writeFrameFiles(evidenceDir, cap, redactStream);
  const metadataPath = join(evidenceDir, "metadata.json");
  const cleanupPath = join(evidenceDir, "cleanup.json");
  if (cap.cleanupReceipt) writeFileSync(cleanupPath, `${JSON.stringify(cap.cleanupReceipt, null, 2)}\n`, "utf8");
  const terminalProfile = { term: args.term, colorterm: args.colorterm, noColor: args.noColor, unicodeVersion: args.unicodeVersion };
  const checkpointMetadata = [];
  for (const checkpoint of cap.checkpoints || []) {
    const directory = join(evidenceDir, "checkpoints", checkpoint.name);
    const files = writeFrameFiles(directory, checkpoint, redactStream);
    const path = join(directory, "metadata.json");
    const item = {
      title: args.title,
      name: checkpoint.name,
      actionIndex: checkpoint.actionIndex,
      capturedAtMillis: checkpoint.capturedAtMillis,
      connector: cap.connector,
      source: sourceMetadata(args),
      dimensions: checkpoint.dimensions,
      capabilities: checkpoint.capabilities,
      terminalProfile,
      cleanup: cap.cleanup,
      cleanupReceipt: cap.cleanupReceipt,
      files: { ...files, metadata: path },
    };
    writeFileSync(path, `${JSON.stringify(item, null, 2)}\n`, "utf8");
    checkpointMetadata.push({ name: item.name, capturedAtMillis: item.capturedAtMillis, files: item.files });
  }

  const metadata = {
    title: args.title,
    connector: cap.connector,
    colorPath: "xterm.js (true color; not tmux)",
    browserCapture: cap.pngBuffer ? "captured" : "skipped",
    source: sourceMetadata(args),
    interaction: redactMetadataValue(args.inputs, redactStream),
    actions: redactMetadataValue(args.actions, redactStream),
    timeline: cap.timeline || [],
    checkpoints: checkpointMetadata,
    terminalProfile,
    redaction: { builtInRules: BUILT_IN_REDACTION_RULE_COUNT, literalRules: args.redactions.length, regexRules: args.redactRegexes.length },
    dimensions: {
      cols: cap.dimensions?.cols || args.cols,
      rows: cap.dimensions?.rows || args.rows,
      fontSize: args.fontSize,
      ...(args.fontFamily ? { fontFamily: args.fontFamily } : {}),
      terminalBackground: args.terminalBackground,
    },
    ...(cap.capabilities ? { capabilities: cap.capabilities } : {}),
    cleanup: cap.cleanup,
    ...(cap.cleanupReceipt ? { cleanupReceipt: cap.cleanupReceipt } : {}),
    files: { ...finalFiles, metadata: metadataPath, cleanup: cap.cleanupReceipt ? cleanupPath : null },
  };
  writeFileSync(metadataPath, `${JSON.stringify(metadata, null, 2)}\n`, "utf8");
  process.stdout.write(`web terminal visual QA evidence (${basename(evidenceDir)}):\n${JSON.stringify(metadata.files, null, 2)}\ncleanup: ${cap.cleanup}\n`);
}

async function selfTest() {
  // Asset resolution + real pty capture, without requiring Chrome (chrome-less CI safe).
  for (const spec of ["@xterm/xterm/lib/xterm.js", "@xterm/xterm/css/xterm.css", "@xterm/addon-unicode11/lib/addon-unicode11.js"]) {
    if (readFileSync(require.resolve(spec), "utf8").length < 100) throw new Error(`asset too small: ${spec}`);
  }
  const cap = await captureRawPty({ command: "printf '\\033[31mRED\\033[0m \\033[32mGREEN\\033[0m 한글ABC'", cwd: process.cwd(), cols: 40, rows: 8, dwellMs: 300 });
  if (!/RED/.test(cap.rawStream) || !cap.rawStream.includes("[31m")) throw new Error("pty did not emit expected ANSI");
  if (!cap.rawStream.includes("한글")) throw new Error("pty dropped CJK bytes");
  const osc52 = "\x1b]52;c;U0VDUkVU\x07";
  const safeOsc52 = redactOsc52Payloads(osc52);
  if (safeOsc52.includes("U0VDUkVU") || !safeOsc52.includes("[REDACTED]")) {
    throw new Error("OSC52 payload redaction failed");
  }
  process.stdout.write("self-test PASS: xterm assets resolve; node-pty emits true-color ANSI + CJK\n");
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) { process.stdout.write(HELP); return; }
  if (args.selfTest) { await selfTest(); return; }
  requireArgs(args);
  await run(args);
}

main().catch((error) => {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exit(1);
});

// Live xterm.js + node-pty terminal capture core.
//
// Spawns a command in a REAL pty (node-pty), bridges it to a REAL xterm.js
// terminal running in a headless Chrome page (puppeteer-core drives the system
// Chrome), drives scripted interaction THROUGH the browser terminal, then
// screenshots the xterm.js render. The screenshot is true-color because
// xterm.js interprets the raw pty byte stream itself - no tmux capture-pane,
// no hand-rolled ANSI-to-HTML, no color degradation.

import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { stripAnsi } from "./strip-ansi.mjs";

const require = createRequire(
  process.env.TUI_FIDELITY_NODE_MODULES
    ? join(process.env.TUI_FIDELITY_NODE_MODULES, "package.json")
    : import.meta.url,
);

function resolveAsset(spec) {
  const path = require.resolve(spec);
  return readFileSync(path, "utf8");
}

// node-pty ships a `spawn-helper` on macOS/Linux that MUST be executable.
// `bun install` (unlike npm) does not run node-pty's chmod postinstall, so the
// committed harness heals the exec bit itself before the first spawn.
function healSpawnHelper() {
  let ptyRoot;
  try {
    ptyRoot = dirname(require.resolve("node-pty"));
  } catch {
    return;
  }
  const candidates = [
    join(ptyRoot, `../prebuilds/${process.platform}-${process.arch}/spawn-helper`),
    join(ptyRoot, "../build/Release/spawn-helper"),
  ];
  for (const helper of candidates) {
    if (existsSync(helper)) {
      try {
        chmodSync(helper, 0o755);
      } catch {
        // best effort - a read-only store still works when the bit is already set
      }
    }
  }
}

const DEFAULT_FONT_FAMILY = '"JetBrainsMono Nerd Font", Menlo, "DejaVu Sans Mono", "Noto Sans Mono CJK KR", monospace';
const DEFAULT_TERMINAL_BACKGROUND = '#141414';

const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

function terminalEnvironment({ term = "xterm-256color", colorterm = "truecolor", noColor = "unset" }) {
  const env = { ...process.env };
  for (const [key, value] of [["TERM", term], ["COLORTERM", colorterm], ["NO_COLOR", noColor]]) {
    if (value === "unset") delete env[key];
    else env[key] = value;
  }
  if (noColor === "unset") env.FORCE_COLOR = "1";
  else delete env.FORCE_COLOR;
  return env;
}

function spawnPty(options) {
  healSpawnHelper();
  const pty = require("node-pty");
  const env = terminalEnvironment(options);
  return pty.spawn("/bin/bash", ["-c", `stty -ixon 2>/dev/null; ${options.command}`], {
    name: env.TERM || "xterm", cols: options.cols, rows: options.rows,
    cwd: options.cwd || process.cwd(), env,
  });
}

function processTable() {
  if (process.platform === "win32") return [];
  const result = spawnSync("ps", ["-eo", "pid=,ppid=,stat="], { encoding: "utf8" });
  if (result.status !== 0) return [];
  return result.stdout.trim().split("\n").flatMap((line) => {
    const match = /^\s*(\d+)\s+(\d+)\s+(\S+)/.exec(line);
    return match ? [{ pid: Number(match[1]), ppid: Number(match[2]), state: match[3] }] : [];
  });
}

function descendantsOf(roots, rows) {
  const found = new Set();
  let parents = new Set(roots.filter(Number.isInteger));
  while (parents.size > 0) {
    const children = rows.filter((row) => parents.has(row.ppid) && !found.has(row.pid)).map((row) => row.pid);
    parents = new Set(children);
    for (const pid of children) found.add(pid);
  }
  return [...found];
}

function livingPids(pids) {
  const rows = new Map(processTable().map((row) => [row.pid, row.state]));
  return pids.filter((pid) => rows.has(pid) && !rows.get(pid).startsWith("Z"));
}

function signalPids(pids, signal, errors) {
  for (const pid of pids) {
    try { process.kill(pid, signal); }
    catch (error) { if (error.code !== "ESRCH") errors.push(`${signal} ${pid}: ${error.message}`); }
  }
}

async function cleanupProcesses(ptyProc, browser) {
  const ptyRootPid = ptyProc?.pid || null;
  const browserRootPid = browser?.process()?.pid || null;
  const rows = processTable();
  const ptyDescendants = descendantsOf([ptyRootPid], rows);
  const browserDescendants = descendantsOf([browserRootPid], rows);
  const detectedDescendantPids = [...new Set([...ptyDescendants, ...browserDescendants])].sort((a, b) => a - b);
  const observedPids = [...new Set([ptyRootPid, browserRootPid, ...detectedDescendantPids].filter(Number.isInteger))];
  const errors = [];
  signalPids(ptyDescendants.reverse(), "SIGTERM", errors);
  try { ptyProc?.kill(); } catch {}
  try { if (browser) await browser.close(); }
  catch (error) { errors.push(`browser close: ${error.message}`); }
  await delay(100);
  signalPids(livingPids(observedPids), "SIGTERM", errors);
  await delay(200);
  signalPids(livingPids(observedPids), "SIGKILL", errors);
  await delay(200);
  const survivingPids = livingPids(observedPids);
  return {
    status: survivingPids.length === 0 && errors.length === 0 ? "clean" : "dirty",
    verification: process.platform === "win32" ? "root-process-only" : "ps-process-table",
    ptyRootPid,
    browserRootPid,
    detectedDescendantPids,
    terminatedPids: observedPids.filter((pid) => !survivingPids.includes(pid)),
    survivingPids,
    errors,
    verifiedAt: new Date().toISOString(),
  };
}

function cleanupSummary(receipt, replay = false) {
  const pty = replay ? "no pty (replay)" : `pty pid ${receipt.ptyRootPid}`;
  return `${pty}; descendant cleanup ${receipt.status} (${receipt.verification})`;
}

function buildPageHtml({ xtermJs, xtermCss, unicodeJs, cols, rows, fontSize, fontFamily, terminalBackground, unicodeVersion }) {
  const resolvedFontFamily = (fontFamily && String(fontFamily).trim()) || DEFAULT_FONT_FAMILY;
  const resolvedTerminalBackground = terminalBackground || DEFAULT_TERMINAL_BACKGROUND;
  // Escape for embedding in a single-quoted JS string literal.
  const fontFamilyJs = resolvedFontFamily.replace(/\\/g, "\\\\").replace(/'/g, "\\'");
  return `<!doctype html><html><head><meta charset="utf-8"><style>
${xtermCss}
html,body{margin:0;padding:0;background:#141414}
#t{padding:8px}
</style></head><body><div id="t"></div>
<script>${xtermJs}</script>
<script>${unicodeJs}</script>
<script>
  const term = new Terminal({
    cols: ${cols}, rows: ${rows}, fontSize: ${fontSize},
    fontFamily: '${fontFamilyJs}',
    allowProposedApi: true, convertEol: false, scrollback: 0,
    // black=palette index 0: keep resets and indexed black aligned with the GrokNight canvas.
    theme: { background: '${resolvedTerminalBackground}', foreground: '#e1e1e1', black: '${resolvedTerminalBackground}' },
  });
  const unicode = new Unicode11Addon.Unicode11Addon();
  term.loadAddon(unicode);
  term.unicode.activeVersion = '${unicodeVersion}';
  term.open(document.getElementById('t'));
  let cursorVisible = true;
  let cursorSequenceTail = '';
  const trackCursorVisibility = (data) => {
    const scanned = cursorSequenceTail + data;
    for (const match of scanned.matchAll(/\\x1b\\[\\?25([hl])/g)) cursorVisible = match[1] === 'h';
    cursorSequenceTail = scanned.slice(-16);
  };
  window.__writeToTerm = (d) => new Promise((resolve) => {
    trackCursorVisibility(d);
    term.write(d, resolve);
  });
  window.__screenText = () => {
    const b = term.buffer.active; const lines = [];
    for (let i = 0; i < b.length; i++) { const ln = b.getLine(i); lines.push(ln ? ln.translateToString(true) : ''); }
    return lines.join('\\n').replace(/\\n+$/, '\\n');
  };
  window.__resetAndWrite = (d) => new Promise((resolve) => {
    term.reset(); cursorVisible = true; cursorSequenceTail = ''; trackCursorVisibility(d); term.write(d, resolve);
  });
  window.__pasteToTerm = (text) => term.paste(text);
  window.__resizeTerm = (cols, rows) => term.resize(cols, rows);
  window.__terminalGeometry = () => {
    const rect = document.querySelector('.xterm-screen').getBoundingClientRect();
    return { left: rect.left, top: rect.top, width: rect.width, height: rect.height, cols: term.cols, rows: term.rows };
  };
  window.__terminalCursor = () => {
    const buffer = term.buffer.active;
    const focused = document.activeElement === term.textarea;
    return { row: buffer.cursorY, col: buffer.cursorX, visible: cursorVisible && focused, focused };
  };
  window.__terminalCapabilities = () => ({
    unicodeVersion: term.unicode.activeVersion,
    devicePixelRatio: window.devicePixelRatio,
    fontLoaded: document.fonts.check('${fontSize}px ${fontFamilyJs}'),
    color: 'truecolor',
    graphics: 'sixel-disabled',
  });
  term.focus();
  term.onData((d) => { if (window.__ptyInput) window.__ptyInput(d); });
</script></body></html>`;
}

const NAMED_KEYS = new Set([
  "Enter", "Tab", "Escape", "Backspace", "Delete", "Space",
  "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", "PageUp", "PageDown",
]);

async function pressKey(page, key, modifiers = {}) {
  const held = [
    modifiers.shift && "Shift",
    modifiers.alt && "Alt",
    modifiers.ctrl && "Control",
    modifiers.meta && "Meta",
  ].filter(Boolean);
  for (const modifier of held) await page.keyboard.down(modifier);
  try { await page.keyboard.press(key === "Space" ? " " : key, { delay: 15 }); }
  finally { for (const modifier of held.reverse()) await page.keyboard.up(modifier); }
}

// An `input` token wrapped in {Braces} is pressed as a named key; anything else
// is typed literally. Both flow through the browser terminal (xterm onData ->
// pty), so the interaction is genuinely driven in the web terminal.
async function driveInput(page, inputs, keyDelayMs) {
  for (const raw of inputs) {
    const match = /^\{(.+)\}$/.exec(raw);
    if (match && NAMED_KEYS.has(match[1])) {
      await pressKey(page, match[1]);
    } else if (match && /^Ctrl\+(.+)$/i.test(match[1])) {
      const key = /^Ctrl\+(.+)$/i.exec(match[1])[1];
      await pressKey(page, key, { ctrl: true });
    } else {
      await page.keyboard.type(raw, { delay: 10 });
    }
    if (keyDelayMs > 0) await delay(keyDelayMs);
  }
}

async function cellPoint(page, point = {}) {
  const geometry = await page.evaluate(() => window.__terminalGeometry());
  const col = point.col ?? Math.ceil(geometry.cols / 2);
  const row = point.row ?? Math.ceil(geometry.rows / 2);
  if (!Number.isInteger(col) || col < 1 || col > geometry.cols || !Number.isInteger(row) || row < 1 || row > geometry.rows) {
    throw new Error(`mouse cell ${col},${row} is outside ${geometry.cols}x${geometry.rows}`);
  }
  return {
    x: geometry.left + ((col - 0.5) * geometry.width) / geometry.cols,
    y: geometry.top + ((row - 0.5) * geometry.height) / geometry.rows,
  };
}

async function driveMouse(page, mouse) {
  const button = mouse.button || "left";
  if (mouse.kind === "move") {
    const point = await cellPoint(page, mouse);
    await page.mouse.move(point.x, point.y);
  } else if (mouse.kind === "click") {
    const point = await cellPoint(page, mouse);
    await page.mouse.click(point.x, point.y, { button, count: mouse.clicks || 1 });
  } else if (mouse.kind === "wheel") {
    const point = await cellPoint(page, mouse);
    await page.mouse.move(point.x, point.y);
    await page.mouse.wheel({ deltaX: mouse.deltaX || 0, deltaY: mouse.deltaY || 100 });
  } else {
    const from = await cellPoint(page, mouse.from);
    const to = await cellPoint(page, mouse.to);
    await page.mouse.move(from.x, from.y);
    await page.mouse.down({ button });
    await page.mouse.move(to.x, to.y, { steps: mouse.steps || 8 });
    await page.mouse.up({ button });
  }
}

async function settleFrame(page) {
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

async function captureFrame(page, rawStream, redactStream, restore) {
  const masked = redactStream ? redactStream(rawStream) : rawStream;
  const changed = masked !== rawStream;
  if (changed) await page.evaluate((data) => window.__resetAndWrite(data), masked);
  await settleFrame(page);
  const screenText = await page.evaluate(() => window.__screenText());
  const dimensions = await page.evaluate(() => {
    const geometry = window.__terminalGeometry();
    return { cols: geometry.cols, rows: geometry.rows };
  });
  const cursor = await page.evaluate(() => window.__terminalCursor());
  const element = (await page.$(".xterm-screen")) || (await page.$(".xterm")) || page;
  const pngBuffer = await element.screenshot({ type: "png" });
  if (changed && restore) await page.evaluate((data) => window.__resetAndWrite(data), rawStream);
  return { pngBuffer, screenText, rawStream, dimensions, cursor };
}

async function driveActions({ page, ptyProc, actions, pauseWrites, resumeWrites, redactStream, capabilities }) {
  const startedAt = Date.now();
  const timeline = [];
  const checkpoints = [];
  for (let index = 0; index < actions.length; index += 1) {
    const action = actions[index];
    const [type] = Object.keys(action);
    const payload = action[type];
    const startedAtMillis = Date.now() - startedAt;
    if (type === "waitForText") {
      await page.waitForFunction((text) => window.__screenText().includes(text), { timeout: payload.timeoutMs, polling: 20 }, payload.text);
    } else if (type === "wait") {
      await delay(payload.ms);
    } else if (type === "input") {
      await page.keyboard.type(payload.text, { delay: 10 });
    } else if (type === "paste") {
      await page.evaluate((text) => window.__pasteToTerm(text), payload.text);
    } else if (type === "key") {
      await pressKey(page, payload.key, payload.modifiers);
    } else if (type === "resize") {
      await page.setViewport({ width: payload.cols * 10 + 40, height: payload.rows * 20 + 40, deviceScaleFactor: 2 });
      await page.evaluate(({ cols, rows }) => window.__resizeTerm(cols, rows), payload);
      if (ptyProc) ptyProc.resize(payload.cols, payload.rows);
    } else if (type === "mouse") {
      await driveMouse(page, payload);
    } else {
      const snapshot = await pauseWrites();
      try {
        const frame = await captureFrame(page, snapshot, redactStream, true);
        checkpoints.push({ ...frame, capabilities: { ...capabilities }, name: payload.name, actionIndex: index, capturedAtMillis: Date.now() - startedAt });
      } finally {
        resumeWrites();
      }
    }
    timeline.push({ index, type, startedAtMillis, completedAtMillis: Date.now() - startedAt });
  }
  return { timeline, checkpoints };
}

function chromeCandidates(explicit) {
  const c = [explicit, process.env.CHROME_BIN, process.env.GOOGLE_CHROME_BIN];
  if (process.platform === "darwin")
    c.push("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium");
  if (process.platform === "linux") c.push("/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser");
  if (process.platform === "win32") {
    c.push(join(process.env.PROGRAMFILES || "C:\\Program Files", "Google\\Chrome\\Application\\chrome.exe"));
    c.push(join(process.env["PROGRAMFILES(X86)"] || "C:\\Program Files (x86)", "Google\\Chrome\\Application\\chrome.exe"));
  }
  return c.filter((x) => x && (x.includes("/") || x.includes("\\") ? existsSync(x) : true));
}

async function captureLive(options) {
  const {
    command, cwd, cols, rows, fromFile, redactStream, chromeBin,
    inputs = [], actions = [], dwellMs = 1500, keyDelayMs = 120, preDwellMs = 400,
    fontSize, fontFamily, terminalBackground, unicodeVersion = "11",
  } = options;
  const puppeteer = require("puppeteer-core");
  const executablePath = chromeCandidates(chromeBin)[0];
  if (!executablePath) throw new Error("no Chrome/Chromium found; set --chrome-bin or CHROME_BIN");
  const resolvedFontSize = Number.isFinite(fontSize) && fontSize > 0 ? fontSize : 15;
  const resolvedFontFamily = (fontFamily && String(fontFamily).trim()) || DEFAULT_FONT_FAMILY;

  const html = buildPageHtml({
    xtermJs: resolveAsset("@xterm/xterm/lib/xterm.js"),
    xtermCss: resolveAsset("@xterm/xterm/css/xterm.css"),
    unicodeJs: resolveAsset("@xterm/addon-unicode11/lib/addon-unicode11.js"),
    cols, rows, fontSize: resolvedFontSize, fontFamily: resolvedFontFamily, terminalBackground, unicodeVersion,
  });

  const browser = await puppeteer.launch({
    executablePath, headless: "new",
    args: ["--no-sandbox", "--disable-gpu", "--hide-scrollbars", "--force-color-profile=srgb"],
    defaultViewport: { width: cols * 10 + 40, height: rows * 20 + 40, deviceScaleFactor: 2 },
  });

  let rawStream = "";
  let ptyProc;
  let result;
  let failure;
  let cleanupReceipt;
  try {
    const page = await browser.newPage();
    await page.setContent(html, { waitUntil: "load" });
    await page.evaluate(() => document.fonts && document.fonts.ready);
    const capabilities = await page.evaluate(() => window.__terminalCapabilities());
    capabilities.browser = await browser.version();
    if (capabilities.unicodeVersion !== unicodeVersion) throw new Error(`xterm.js Unicode ${unicodeVersion} mode is unavailable`);
    if (!capabilities.fontLoaded) throw new Error(`required terminal font is unavailable: ${resolvedFontFamily}`);

    let writeChain = Promise.resolve();
    let writeFailure;
    let writesPaused = false;
    let pendingWrites = [];
    const queueWrite = (data) => {
      writeChain = writeChain
        .then(() => page.evaluate((chunk) => window.__writeToTerm(chunk), data))
        .catch((error) => { writeFailure = error; });
    };
    const awaitWrites = async () => {
      await writeChain;
      if (writeFailure) throw writeFailure;
    };
    const pauseWrites = async () => {
      writesPaused = true;
      await awaitWrites();
      const snapshot = rawStream;
      pendingWrites = [];
      return snapshot;
    };
    const resumeWrites = () => {
      writesPaused = false;
      const queued = pendingWrites;
      pendingWrites = [];
      for (const data of queued) queueWrite(data);
    };
    if (fromFile !== undefined) {
      rawStream = fromFile;
      const shown = redactStream ? redactStream(rawStream) : rawStream;
      await page.evaluate((d) => window.__writeToTerm(d), shown);
    } else {
      ptyProc = spawnPty({ ...options, command, cwd, cols, rows });
      await page.exposeFunction("__ptyInput", (d) => { try { ptyProc.write(d); } catch {} });
      ptyProc.onData((d) => {
        rawStream += d;
        if (writesPaused) pendingWrites.push(d);
        else queueWrite(d);
      });
      await delay(Number.isFinite(preDwellMs) && preDwellMs >= 0 ? preDwellMs : 400);
      await awaitWrites();
      await page.focus("#t");
    }

    const actionResult = await driveActions({ page, ptyProc, actions, pauseWrites, resumeWrites, redactStream, capabilities });
    if (ptyProc && inputs.length) await driveInput(page, inputs, keyDelayMs);
    if (ptyProc) await delay(dwellMs);
    const snapshot = await pauseWrites();
    const frame = await captureFrame(page, snapshot, redactStream, false);
    result = {
      ...frame,
      capabilities,
      timeline: actionResult.timeline,
      checkpoints: actionResult.checkpoints,
      connector: fromFile !== undefined ? "xterm-replay" : "xterm-node-pty",
    };
  } catch (error) {
    failure = error;
  } finally {
    cleanupReceipt = await cleanupProcesses(ptyProc, browser);
  }
  if (failure) {
    failure.cleanupReceipt = cleanupReceipt;
    throw failure;
  }
  if (cleanupReceipt.status !== "clean") throw new Error(`descendant cleanup failed: ${JSON.stringify(cleanupReceipt)}`);
  return { ...result, cleanupReceipt, cleanup: cleanupSummary(cleanupReceipt, fromFile !== undefined) };
}

async function captureRawPty(options) {
  const proc = spawnPty({ term: "xterm-256color", colorterm: "truecolor", noColor: "unset", ...options });
  let rawStream = "";
  proc.onData((data) => { rawStream += data; });
  await delay((Number.isFinite(options.dwellMs) ? options.dwellMs : 1500) + 400);
  const cleanupReceipt = await cleanupProcesses(proc, null);
  if (cleanupReceipt.status !== "clean") throw new Error(`descendant cleanup failed: ${JSON.stringify(cleanupReceipt)}`);
  return {
    pngBuffer: null,
    screenText: stripAnsi(rawStream),
    rawStream,
    connector: "node-pty-raw",
    cleanupReceipt,
    cleanup: cleanupSummary(cleanupReceipt),
  };
}

export { captureLive, captureRawPty };

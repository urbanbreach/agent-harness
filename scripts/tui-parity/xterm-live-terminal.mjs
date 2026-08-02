// Live xterm.js + node-pty terminal capture core.
//
// Spawns a command in a REAL pty (node-pty), bridges it to a REAL xterm.js
// terminal running in a headless Chrome page (puppeteer-core drives the system
// Chrome), drives scripted interaction THROUGH the browser terminal, then
// screenshots the xterm.js render. The screenshot is true-color because
// xterm.js interprets the raw pty byte stream itself - no tmux capture-pane,
// no hand-rolled ANSI-to-HTML, no color degradation.

import { createRequire } from "node:module";
import { chmodSync, existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";

const require = createRequire(import.meta.url);

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

const DEFAULT_FONT_FAMILY = '"JetBrainsMono Nerd Font Mono", Menlo, "DejaVu Sans Mono", "Noto Sans Mono CJK KR", monospace';

function buildPageHtml({ xtermJs, xtermCss, unicodeJs, cols, rows, fontSize, fontFamily, terminalBackground }) {
  const resolvedFontFamily = (fontFamily && String(fontFamily).trim()) || DEFAULT_FONT_FAMILY;
  const resolvedTerminalBackground = (terminalBackground && String(terminalBackground).trim()) || "#141414";
  // Escape for embedding in a single-quoted JS string literal.
  const fontFamilyJs = resolvedFontFamily.replace(/\\/g, "\\\\").replace(/'/g, "\\'");
  return `<!doctype html><html><head><meta charset="utf-8"><style>
${xtermCss}
html,body{margin:0;padding:0;background:${resolvedTerminalBackground}}
#t{padding:8px}
</style></head><body><div id="t"></div>
<script>${xtermJs}</script>
<script>${unicodeJs}</script>
<script>
  const term = new Terminal({
    cols: ${cols}, rows: ${rows}, fontSize: ${fontSize},
    fontFamily: '${fontFamilyJs}',
    allowProposedApi: true, convertEol: false, scrollback: 0,
    // black=palette index 0: Grok live shell fills with 48;5;0; must match canvas or freezes drift to xterm default #2e3436
    theme: { background: '${resolvedTerminalBackground}', foreground: '#d7dae0', black: '${resolvedTerminalBackground}' },
  });
  try { const u = new Unicode11Addon.Unicode11Addon(); term.loadAddon(u); term.unicode.activeVersion = '11'; } catch (e) {}
  term.open(document.getElementById('t'));
  window.__writeToTerm = (d) => new Promise((resolve) => term.write(d, resolve));
  window.__screenText = () => {
    const b = term.buffer.active; const lines = [];
    for (let i = 0; i < b.length; i++) { const ln = b.getLine(i); lines.push(ln ? ln.translateToString(true) : ''); }
    return lines.join('\\n').replace(/\\n+$/, '\\n');
  };
  window.__resetAndWrite = (d) => { term.reset(); return new Promise((resolve) => term.write(d, resolve)); };
  term.focus();
  term.onData((d) => { if (window.__ptyInput) window.__ptyInput(d); });
</script></body></html>`;
}

const NAMED_KEYS = new Set([
  "Enter", "Tab", "Escape", "Backspace", "Delete", "Space",
  "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", "PageUp", "PageDown",
]);

// An `input` token wrapped in {Braces} is pressed as a named key; anything else
// is typed literally. Both flow through the browser terminal (xterm onData ->
// pty), so the interaction is genuinely driven in the web terminal.
async function driveInput(page, inputs, keyDelayMs, cols, rows) {
  for (const raw of inputs) {
    const match = /^\{(.+)\}$/.exec(raw);
    if (match && NAMED_KEYS.has(match[1])) {
      await page.keyboard.press(match[1] === "Space" ? " " : match[1], { delay: 15 });
    } else if (match && /^Ctrl\+(.+)$/i.test(match[1])) {
      const key = /^Ctrl\+(.+)$/i.exec(match[1])[1];
      const pressKey = key === "Space" ? " " : key;
      await page.keyboard.down("Control");
      await page.keyboard.press(pressKey);
      await page.keyboard.up("Control");
    } else if (match && /^Click:(\d+),(\d+)$/.test(match[1])) {
      const [, column, row] = /^Click:(\d+),(\d+)$/.exec(match[1]);
      const terminal = await page.$(".xterm");
      const box = await terminal.boundingBox();
      if (!box) throw new Error("terminal is not visible for click input");
      await page.mouse.click(
        box.x + ((Number(column) + 0.5) / cols) * box.width,
        box.y + ((Number(row) + 0.5) / rows) * box.height,
      );
    } else {
      await page.keyboard.type(raw, { delay: 10 });
    }
    if (keyDelayMs > 0) await new Promise((r) => setTimeout(r, keyDelayMs));
  }
}

async function screenshotTerminal(page) {
  const screen = await page.$(".xterm-screen");
  if (!screen) throw new Error("xterm screen is not visible for PNG capture");
  return screen.screenshot({ type: "png" });
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

async function captureLive({ command, cwd, cols, rows, inputs, dwellMs, frameMs, phaseOriginMs, keyDelayMs, preDwellMs, chromeBin, fromFile, redactStream, fontSize, fontFamily, terminalBackground }) {
  healSpawnHelper();
  const puppeteer = (await import("puppeteer-core")).default;
  const executablePath = chromeCandidates(chromeBin)[0];
  if (!executablePath) throw new Error("no Chrome/Chromium found; set --chrome-bin or CHROME_BIN");
  const resolvedFontSize = Number.isFinite(fontSize) && fontSize > 0 ? fontSize : 15;
  const resolvedFontFamily = (fontFamily && String(fontFamily).trim()) || DEFAULT_FONT_FAMILY;

  const html = buildPageHtml({
    xtermJs: resolveAsset("@xterm/xterm/lib/xterm.js"),
    xtermCss: resolveAsset("@xterm/xterm/css/xterm.css"),
    unicodeJs: resolveAsset("@xterm/addon-unicode11/lib/addon-unicode11.js"),
    cols, rows, fontSize: resolvedFontSize, fontFamily: resolvedFontFamily, terminalBackground,
  });

  const browser = await puppeteer.launch({
    executablePath, headless: "new",
    args: ["--no-sandbox", "--disable-gpu", "--hide-scrollbars", "--force-color-profile=srgb"],
    defaultViewport: { width: cols * 10 + 40, height: rows * 20 + 40, deviceScaleFactor: 2 },
  });

  let rawStream = "";
  let ptyProc;
  let ptyStartedAt;
  let writeChain = Promise.resolve();
  const frames = [];
  const cleanupParts = [];
  try {
    const page = await browser.newPage();
    await page.setContent(html, { waitUntil: "load" });
    await page.evaluate(() => document.fonts && document.fonts.ready);

    if (fromFile) {
      rawStream = fromFile;
      const shown = redactStream ? redactStream(rawStream) : rawStream;
      await page.evaluate((d) => window.__writeToTerm(d), shown);
      cleanupParts.push("no pty (replay)");
    } else {
      const pty = require("node-pty");
      // stty -ixon so Ctrl+s is delivered to the TUI (not software flow-control XOFF).
      const ptyCommand = `stty -ixon 2>/dev/null; ${command}`;
      // Crossterm treats any non-empty NO_COLOR as "disable ANSI colors", which
      // serializes SetColors as empty SGR (`ESC[;m`) and wipes prior bold. Force
      // colors on for visual parity captures regardless of parent agent env.
      const ptyEnv = {
        ...process.env,
        TERM: "xterm-256color",
        COLORTERM: "truecolor",
        FORCE_COLOR: "1",
      };
      delete ptyEnv.NO_COLOR;
      // stty first, then run the command under env -u so NO_COLOR cannot reappear.
      const colorSafeCommand = `stty -ixon 2>/dev/null; env -u NO_COLOR ${command}`;
      ptyStartedAt = Date.now();
      ptyProc = pty.spawn("/bin/bash", ["-c", colorSafeCommand], {
        name: "xterm-256color", cols, rows, cwd: cwd || process.cwd(),
        env: ptyEnv,
      });
      await page.exposeFunction("__ptyInput", (d) => { try { ptyProc.write(d); } catch {} });
      ptyProc.onData((d) => {
        rawStream += d;
        writeChain = writeChain
          .then(() => page.evaluate((chunk) => window.__writeToTerm(chunk), d))
          .catch(() => {});
      });
      const bootWait = Number.isFinite(preDwellMs) && preDwellMs > 0 ? preDwellMs : 400;
      await new Promise((r) => setTimeout(r, bootWait));
      await page.focus("#t");
      if (inputs.length) await driveInput(page, inputs, keyDelayMs, cols, rows);
      const requestedFrames = Array.isArray(frameMs)
        ? [...new Set(frameMs)].sort((a, b) => a - b)
        : [];
      if (requestedFrames.length) {
        if (!Number.isFinite(phaseOriginMs) || phaseOriginMs < 0) {
          throw new Error("--frame-ms requires --phase-origin-ms from the equivalent reference visual phase");
        }
        const sequenceStartedAt = ptyStartedAt + phaseOriginMs;
        for (const targetMs of requestedFrames) {
          const remainingMs = Math.max(0, sequenceStartedAt + targetMs - Date.now());
          await new Promise((r) => setTimeout(r, remainingMs));
          await writeChain;
          await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
          const screenText = await page.evaluate(() => window.__screenText());
          const pngBuffer = await screenshotTerminal(page);
          frames.push({ elapsedMs: targetMs, pngBuffer, screenText, rawStream });
        }
      } else {
        await new Promise((r) => setTimeout(r, dwellMs));
      }
      cleanupParts.push(`pty pid ${ptyProc.pid} killed`);
    }

    // When redactions are configured, re-render the masked stream so the PNG
    // never shows a secret that the interaction surfaced on screen.
    if (redactStream) {
      const masked = redactStream(rawStream);
      if (masked !== rawStream) await page.evaluate((d) => window.__resetAndWrite(d), masked);
    }
    await writeChain;
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));

    const screenText = await page.evaluate(() => window.__screenText());
    const pngBuffer = await screenshotTerminal(page);
    return { pngBuffer, screenText, rawStream, frames, connector: fromFile ? "xterm-replay" : "xterm-node-pty", cleanup: cleanupParts.join("; ") };
  } finally {
    try { ptyProc && ptyProc.kill(); } catch {}
    await browser.close();
  }
}

export { captureLive };

import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";

const require = createRequire(import.meta.url);
const xtermRoot = dirname(require.resolve("@xterm/xterm/package.json"));
export const fontPath = "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf";

export async function mountTerminal(page, settings) {
  const xtermCss = await readFile(join(xtermRoot, "css/xterm.css"), "utf8");
  const font = (await readFile(fontPath)).toString("base64");
  await page.setContent(`<!doctype html><html><head><meta charset="utf-8"><title>${escapeHtml(settings.title)}</title></head><body><main id="terminal" aria-label="Harness terminal"></main></body></html>`);
  await page.addStyleTag({ content: `${tokens()}\n${xtermCss}\n@font-face{font-family:"Harness QA Mono";src:url(data:font/ttf;base64,${font}) format("truetype");font-display:block}` });
  await page.addScriptTag({ path: join(xtermRoot, "lib/xterm.js") });
  await page.evaluate(async ({ cols, rows, initialTitle, hostWidth, hostHeight }) => {
    await document.fonts.ready;
    await document.fonts.load('16px "Harness QA Mono"');
    const terminal = new Terminal({
      allowProposedApi: true,
      cursorBlink: false,
      fontFamily: "Harness QA Mono",
      fontSize: 16,
      lineHeight: 1,
      letterSpacing: 0,
      scrollback: 10000,
      theme: {
        background: "rgb(20, 20, 20)",
        foreground: "rgb(225, 225, 225)",
        cursor: "rgb(187, 154, 247)",
        selectionBackground: "rgb(85, 87, 83)",
      },
    });
    const host = document.querySelector("#terminal");
    host.style.width = `${hostWidth}px`;
    host.style.height = `${hostHeight}px`;
    terminal.open(host);
    terminal.resize(cols, rows);
    terminal.onData((data) => window.qaPtyInput(data));
    let renderCount = 0;
    let parsedCount = 0;
    let lastRender = null;
    let latestTitle = initialTitle;
    const titleHistory = [initialTitle];
    terminal.onRender((range) => {
      renderCount += 1;
      lastRender = range;
    });
    terminal.onWriteParsed(() => { parsedCount += 1; });
    terminal.onTitleChange((title) => {
      latestTitle = title;
      titleHistory.push(title);
    });
    const rowsText = () => {
      const buffer = terminal.buffer.active;
      return Array.from({ length: terminal.rows }, (_, row) =>
        buffer.getLine(buffer.viewportY + row)?.translateToString(true) ?? ""
      );
    };
    window.qaTerminal = {
      text: () => rowsText().join("\n"),
      find: (needle) => {
        const rows = rowsText();
        for (let row = 0; row < rows.length; row += 1) {
          const column = rows[row].indexOf(needle);
          if (column >= 0) return { row, column };
        }
        return null;
      },
      waitForVisualSync: async (timeoutMs = 10000) => {
        await document.fonts.ready;
        await document.fonts.load('16px "Harness QA Mono"');
        await new Promise((resolve, reject) => {
          const deadline = window.setTimeout(
            () => reject(new Error("xterm repaint timed out")),
            timeoutMs,
          );
          const render = terminal.onRender((range) => {
            if (range.start > 0 || range.end < terminal.rows - 1) return;
            render.dispose();
            window.clearTimeout(deadline);
            resolve();
          });
          terminal.refresh(0, terminal.rows - 1);
        });
        await new Promise((resolve) => window.requestAnimationFrame(() => {
          window.requestAnimationFrame(resolve);
        }));
      },
      write: (base64) => new Promise((resolve, reject) => {
        const bytes = Uint8Array.from(atob(base64), (character) => character.charCodeAt(0));
        const deadline = window.setTimeout(() => reject(new Error("xterm write callback timed out")), 10000);
        terminal.write(bytes, () => {
          window.clearTimeout(deadline);
          window.qaTerminal.waitForVisualSync().then(() => resolve({ bytes: bytes.length, renderCount, parsedCount, lastRender }), reject);
        });
      }),
      title: () => latestTitle,
      titleHistory: () => [...titleHistory],
      snapshot: () => {
        const buffer = terminal.buffer.active;
        const rows = rowsText();
        return {
          cols: terminal.cols,
          rows: terminal.rows,
          title: latestTitle,
          titleHistory: [...titleHistory],
          activeBuffer: terminal.buffer.active === terminal.buffer.alternate ? "alternate" : "normal",
          cursor: { x: buffer.cursorX, y: buffer.cursorY, baseY: buffer.baseY, viewportY: buffer.viewportY },
          modes: terminal.modes,
          renderCount,
          parsedCount,
          lastRender,
          text: rows.join("\n"),
          lines: rows.map((text, row) => {
            const line = buffer.getLine(buffer.viewportY + row);
            const cells = [];
            for (let column = 0; column < terminal.cols; column += 1) {
              const cell = line?.getCell(column);
              const chars = cell?.getChars() ?? "";
              const width = cell?.getWidth() ?? 1;
              if (chars.trim().length > 0 || width !== 1) {
                const style = { invisible: cell.isInvisible(), fgColor: cell.getFgColor(), bgColor: cell.getBgColor(), fgColorMode: cell.getFgColorMode(), bgColorMode: cell.getBgColorMode(), fgRgb: cell.isFgRGB(), bgRgb: cell.isBgRGB(), fgPalette: cell.isFgPalette(), bgPalette: cell.isBgPalette(), fgDefault: cell.isFgDefault(), bgDefault: cell.isBgDefault() };
                cells.push({ column, chars, width, ...style });
              }
            }
            return { row, text, wrapped: line?.isWrapped ?? false, cells };
          }),
        };
      },
      renderDimensions: () => {
        const dimensions = terminal._core._renderService.dimensions;
        return {
          css: { cell: dimensions.css.cell, canvas: dimensions.css.canvas },
          device: { cell: dimensions.device.cell, canvas: dimensions.device.canvas },
          cellWidthTimesCols: dimensions.css.cell.width * terminal.cols,
        };
      },
      focus: () => terminal.focus(),
    };
    terminal.focus();
    document.documentElement.dataset.qaReady = "true";
  }, {
    cols: settings.cols,
    rows: settings.rows,
    initialTitle: settings.title,
    hostWidth: settings.hostWidth,
    hostHeight: settings.hostHeight,
  });
  await page.waitForFunction(() => document.documentElement.dataset.qaReady === "true", null, { timeout: settings.timeoutMs });
  await page.evaluate((timeoutMs) => window.qaTerminal.waitForVisualSync(timeoutMs), settings.timeoutMs);
  return page.locator(".xterm").evaluate((element) => {
    const bounds = element.getBoundingClientRect();
    return { width: bounds.width, height: bounds.height };
  });
}

function tokens() {
  return `:root{--qa-canvas:rgb(20 20 20);--qa-text:rgb(225 225 225);--qa-unit:1rem;--qa-zero:0}*{box-sizing:border-box}html,body{margin:var(--qa-zero);min-width:100%;min-height:100%;overflow:hidden;background:var(--qa-canvas);color:var(--qa-text)}body{padding:var(--qa-unit)}#terminal{display:inline-block}`;
}

function escapeHtml(value) {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

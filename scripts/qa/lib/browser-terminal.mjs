import { rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { chromium } from "playwright-core";
import { fontPath, mountTerminal } from "./browser-terminal-mount.mjs";
import { assertScreenshotSafe } from "./security.mjs";

const require = createRequire(import.meta.url);

export async function openBrowserTerminal(settings) {
  const profilePath = settings.profilePath;
  const initialViewport = {
    width: Math.max(1200, settings.cols * 24 + 64),
    height: Math.max(800, settings.rows * 24 + 64),
  };
  let context;
  let browser;
  let page;
  const consoleMessages = [];
  let pendingWrites = Promise.resolve();
  const waitForWrites = async () => {
    let current;
    do {
      current = pendingWrites;
      await current;
    } while (current !== pendingWrites);
  };
  try {
    context = await chromium.launchPersistentContext(profilePath, {
      executablePath: settings.browser,
      headless: true,
      viewport: initialViewport,
      deviceScaleFactor: 1,
      args: ["--disable-background-timer-throttling", "--force-device-scale-factor=1"],
    });
    browser = context.browser();
    page = context.pages()[0] ?? await context.newPage();
    page.on("console", (message) => consoleMessages.push({ type: message.type(), text: message.text() }));
    page.on("pageerror", (error) => consoleMessages.push({ type: "pageerror", text: error.message }));
    await page.exposeBinding("qaPtyInput", (_, data) => settings.onInput(data));
    const surface = await mountTerminal(page, {
      ...settings,
      hostWidth: initialViewport.width - 32,
      hostHeight: initialViewport.height - 32,
    });
    await page.setViewportSize({
      width: Math.max(initialViewport.width, Math.ceil(surface.width + 32)),
      height: Math.max(initialViewport.height, Math.ceil(surface.height + 32)),
    });
  } catch (error) {
    try {
      await closeBrowserResources({ context, browser, page, profilePath });
    } catch (cleanupError) {
      throw new AggregateError([error, cleanupError], "browser acquisition and cleanup failed");
    }
    throw error;
  }

  return {
    write(bytes) {
      const write = pendingWrites.then(() => page.evaluate(
        async (base64) => window.qaTerminal.write(base64),
        bytes.toString("base64"),
      ));
      pendingWrites = write;
      return write;
    },
    async waitForText(needle) {
      await waitForWrites();
      await page.waitForFunction(
        (text) => window.qaTerminal.text().includes(text),
        needle,
        { timeout: settings.timeoutMs },
      );
      await waitForWrites();
      return this.snapshot();
    },
    async waitForTextAbsent(needle) {
      await waitForWrites();
      await page.waitForFunction(
        (text) => !window.qaTerminal.text().includes(text),
        needle,
        { timeout: settings.timeoutMs },
      );
      await waitForWrites();
      return this.snapshot();
    },
    async waitForTextCount(needle, count) {
      await waitForWrites();
      await page.waitForFunction(
        ({ text, expected }) => window.qaTerminal.text().split(text).length - 1 >= expected,
        { text: needle, expected: count },
        { timeout: settings.timeoutMs },
      );
      await waitForWrites();
      return this.snapshot();
    },
    async waitForTitle(title) {
      await waitForWrites();
      await page.waitForFunction(
        (expected) => window.qaTerminal.titleHistory().includes(expected),
        title,
        { timeout: settings.timeoutMs },
      );
      await waitForWrites();
      return { ...await this.snapshot(), matchedTitle: title };
    },
    async type(text) {
      await page.evaluate(() => window.qaTerminal.focus());
      await page.keyboard.insertText(text);
    },
    async key(key) {
      await page.evaluate(() => window.qaTerminal.focus());
      await page.keyboard.press(key);
    },
    async resize(cols, rows) {
      await waitForWrites();
      if (!Number.isSafeInteger(cols) || cols <= 0 || !Number.isSafeInteger(rows) || rows <= 0) {
        throw new Error("xterm resize requires positive integer dimensions");
      }
      return page.evaluate(
        ({ nextCols, nextRows }) => window.qaTerminal.resize(nextCols, nextRows),
        { nextCols: cols, nextRows: rows },
      );
    },
    async clickText(needle) {
      const location = await page.evaluate((text) => window.qaTerminal.find(text), needle);
      if (!location) throw new Error(`visible click target not found: ${needle}`);
      await this.mouseCell("click", location.column + 1, location.row + 1);
      return location;
    },
    async mouseCell(kind, column, row) {
      const dimensions = await page.evaluate(() => {
        const snapshot = window.qaTerminal.snapshot();
        return { cols: snapshot.cols, rows: snapshot.rows };
      });
      if (!Number.isSafeInteger(column) || column < 1 || column > dimensions.cols
        || !Number.isSafeInteger(row) || row < 1 || row > dimensions.rows) {
        throw new Error(`terminal cell is outside ${dimensions.cols}x${dimensions.rows}: ${column},${row}`);
      }
      const bounds = await page.locator(".xterm-screen").boundingBox();
      if (!bounds) throw new Error("xterm screen has no browser bounds");
      const x = bounds.x + (column - 0.5) * bounds.width / dimensions.cols;
      const y = bounds.y + (row - 0.5) * bounds.height / dimensions.rows;
      if (kind === "click") await page.mouse.click(x, y);
      else if (kind === "down") {
        await page.mouse.move(x, y);
        await page.mouse.down();
      } else if (kind === "up") {
        await page.mouse.move(x, y);
        await page.mouse.up();
      } else throw new Error(`unsupported terminal mouse action: ${kind}`);
      return { kind, column, row };
    },
    async snapshot() {
      await waitForWrites();
      return page.evaluate(() => window.qaTerminal.snapshot());
    },
    async waitForFrame(frame) {
      await waitForWrites();
      const handle = await page.waitForFunction(
        ({ marker, left, top, right, bottom }) => {
          const current = window.qaTerminal.snapshot();
          const lines = current.text.split("\n");
          const cell = (row, column) => Array.from(lines[row - 1] ?? "")[column - 1];
          const verticalEdgesComplete = Array.from(
            { length: Math.max(0, bottom - top - 1) },
            (_, index) => top + index + 1,
          ).every((row) => cell(row, left) === "│" && cell(row, right) === "│");
          return current.text.includes(marker)
            && cell(top, left) === "┌"
            && cell(top, right) === "┐"
            && verticalEdgesComplete
            && cell(bottom, left) === "└"
            && cell(bottom, right) === "┘";
        },
        frame,
        { timeout: settings.timeoutMs },
      );
      await page.evaluate((timeoutMs) => window.qaTerminal.waitForVisualSync(timeoutMs), settings.timeoutMs);
      await handle.dispose();
      return this.snapshot();
    },
    async screenshot(path) {
      await waitForWrites();
      await page.evaluate((timeoutMs) => window.qaTerminal.waitForVisualSync(timeoutMs), settings.timeoutMs);
      assertScreenshotSafe(await this.snapshot());
      const bytes = await (await screenshotTarget(page)).screenshot({ animations: "disabled" });
      await writeFile(path, bytes, { mode: 0o600 });
      return bytes;
    },
    async capture(path) {
      await waitForWrites();
      await page.evaluate((timeoutMs) => window.qaTerminal.waitForVisualSync(timeoutMs), settings.timeoutMs);
      const snapshot = await this.snapshot();
      assertScreenshotSafe(snapshot);
      const bytes = await (await screenshotTarget(page)).screenshot({ animations: "disabled" });
      await writeFile(path, bytes, { mode: 0o600 });
      return snapshot;
    },
    async renderedCellHasText(column, row, text) {
      await waitForWrites();
      await page.evaluate((timeoutMs) => window.qaTerminal.waitForVisualSync(timeoutMs), settings.timeoutMs);
      return page.evaluate(({ column: cellColumn, row: cellRow, text: expectedText, cols }) => {
        const rowsElement = document.querySelector(".xterm-rows");
        const rowElement = rowsElement?.children[cellRow];
        const screen = document.querySelector(".xterm-screen");
        if (!rowsElement || !rowElement || !screen) return false;
        const bounds = screen.getBoundingClientRect();
        const left = bounds.left + cellColumn * bounds.width / cols;
        const right = bounds.left + (cellColumn + expectedText.length) * bounds.width / cols;
        const renderedText = Array.from(rowElement.querySelectorAll("span"))
          .filter((span) => {
            const spanBounds = span.getBoundingClientRect();
            return spanBounds.right > left && spanBounds.left < right;
          })
          .map((span) => span.textContent ?? "")
          .join("");
        return renderedText.includes(expectedText);
      }, { column, row, text, cols: settings.cols });
    },
    async metadata() {
      await waitForWrites();
      return {
        browserVersion: browser?.version() ?? "unknown",
        xtermVersion: require("@xterm/xterm/package.json").version,
        playwrightVersion: require("playwright-core/package.json").version,
        font: { family: "Harness QA Mono", source: fontPath, embedded: true },
        viewport: page.viewportSize(),
        renderSurface: {
          root: await page.locator(".xterm").boundingBox(),
          screen: await page.locator(".xterm-screen").boundingBox(),
        },
        renderDimensions: await page.evaluate(() => window.qaTerminal.renderDimensions()),
        terminal: await this.snapshot(),
        console: consoleMessages,
      };
    },
    async close() {
      return closeBrowserResources({ context, browser, page, profilePath });
    },
  };
}

export async function closeBrowserResources(settings) {
  const failures = [];
  let contextClosed = !settings.context;
  try {
    if (settings.context) await settings.context.close();
    contextClosed = true;
  } catch (error) {
    failures.push(error);
    try {
      if (settings.browser) await settings.browser.close();
    } catch (browserError) {
      failures.push(browserError);
    }
  }
  const connected = settings.browser?.isConnected() ?? false;
  let profileRemoved = false;
  try {
    const removeProfile = settings.removeProfile
      ?? ((path) => rm(path, { recursive: true, force: true }));
    await removeProfile(settings.profilePath);
    profileRemoved = true;
  } catch (error) {
    failures.push(error);
  }
  if (failures.length > 0) {
    throw new AggregateError(failures, "browser cleanup failed");
  }
  return {
    pageClosed: settings.page?.isClosed() ?? true,
    contextClosed,
    browserConnectedAfterClose: connected,
    profileRemoved,
    profilePath: settings.profilePath,
    boundPorts: [],
  };
}

async function screenshotTarget(page) {
  const screen = page.locator(".xterm-screen");
  if (!await screen.boundingBox()) throw new Error("xterm screenshot surface has no bounds");
  return screen;
}

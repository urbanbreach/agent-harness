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
  const context = await chromium.launchPersistentContext(profilePath, {
    executablePath: settings.browser,
    headless: true,
    viewport: initialViewport,
    deviceScaleFactor: 1,
    args: ["--disable-background-timer-throttling", "--force-device-scale-factor=1"],
  });
  const browser = context.browser();
  const page = context.pages()[0] ?? await context.newPage();
  const consoleMessages = [];
  let pendingWrites = Promise.resolve();
  page.on("console", (message) => consoleMessages.push({ type: message.type(), text: message.text() }));
  const waitForWrites = async () => {
    let current;
    do {
      current = pendingWrites;
      await current;
    } while (current !== pendingWrites);
  };
  page.on("pageerror", (error) => consoleMessages.push({ type: "pageerror", text: error.message }));
  try {
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
    await context.close();
    await rm(profilePath, { recursive: true, force: true });
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
    async clickText(needle) {
      const location = await page.evaluate((text) => window.qaTerminal.find(text), needle);
      if (!location) throw new Error(`visible click target not found: ${needle}`);
      const screen = page.locator(".xterm-screen");
      const bounds = await screen.boundingBox();
      if (!bounds) throw new Error("xterm screen has no browser bounds");
      const cellWidth = bounds.width / settings.cols;
      const cellHeight = bounds.height / settings.rows;
      await page.mouse.click(
        bounds.x + (location.column + 0.5) * cellWidth,
        bounds.y + (location.row + 0.5) * cellHeight,
      );
      return location;
    },
    async snapshot() {
      await waitForWrites();
      return page.evaluate(() => window.qaTerminal.snapshot());
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
      await context.close();
      const connected = browser?.isConnected() ?? false;
      await rm(profilePath, { recursive: true, force: true });
      return {
        pageClosed: page.isClosed(),
        contextClosed: true,
        browserConnectedAfterClose: connected,
        profileRemoved: true,
        profilePath,
        boundPorts: [],
      };
    },
  };
}

async function screenshotTarget(page) {
  const root = page.locator(".xterm");
  const screen = page.locator(".xterm-screen");
  const [rootBounds, screenBounds] = await Promise.all([root.boundingBox(), screen.boundingBox()]);
  if (!rootBounds || !screenBounds) throw new Error("xterm screenshot surfaces have no bounds");
  return screenBounds.width > rootBounds.width ? screen : root;
}

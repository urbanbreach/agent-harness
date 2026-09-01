import { chmod, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileReceipt, sha256 } from "./provenance.mjs";
import { assertSecretFree, redactEvidence } from "./security.mjs";

export async function prepareEvidence(path) {
  await rm(path, { recursive: true, force: true });
  await mkdir(path, { recursive: true, mode: 0o700 });
  await chmod(path, 0o700);
}

export async function writePassEvidence(settings) {
  assertCleanup(settings.cleanup);
  const generatedAt = new Date().toISOString();
  const safeRaw = redactEvidence(settings.raw);
  const safeCapture = redactEvidence(settings.capture);
  const safeInteractions = redactEvidence(settings.interactions);
  const files = {
    raw: join(settings.evidenceDir, "terminal.ansi"),
    ansiText: join(settings.evidenceDir, "terminal-ansi.txt"),
    transcript: join(settings.evidenceDir, "terminal.txt"),
    buffer: join(settings.evidenceDir, "buffer.json"),
    interactions: join(settings.evidenceDir, "interactions.json"),
    metadata: join(settings.evidenceDir, "metadata.json"),
    cleanup: join(settings.evidenceDir, "cleanup.json"),
    screenshot: join(settings.evidenceDir, "terminal.png"),
  };
  for (const path of [files.screenshot, ...(settings.captures ?? []).map(({ path }) => path)]) {
    await chmod(path, 0o600);
  }
  await Promise.all([
    writePrivate(files.raw, safeRaw),
    writePrivate(files.ansiText, safeRaw),
    writePrivate(files.transcript, `${safeCapture.text.replace(/[ ]+$/gm, "")}\n`),
    writeJson(files.buffer, safeCapture),
    writeJson(files.interactions, safeInteractions),
    writeJson(files.cleanup, { result: "PASS", ...settings.cleanup }),
  ]);
  const screenshot = await pngReceipt(files.screenshot, settings.evidenceDir);
  const captures = await Promise.all(
    (settings.captures ?? []).map(({ index, path }) => pngReceipt(path, settings.evidenceDir).then((receipt) => ({ index, ...receipt }))),
  );
  const metadata = redactEvidence({
    schemaVersion: "harness-xterm-visual-qa-v1",
    result: "PASS",
    generatedAt,
    scenario: settings.contract.name,
    title: settings.contract.title,
    command: settings.command,
    commandTemplate: settings.contract.command,
    argv: settings.argv,
    provenance: {
      repoRoot: settings.repoRoot,
      sourceTree: settings.sourceTree,
      scriptSha256: settings.scriptSha256,
      ptyProvider: "util-linux script",
      browserExecutable: settings.browser,
      browser: settings.browserMetadata,
    },
    terminal: {
      cols: settings.capture.cols,
      rows: settings.capture.rows,
      activeBuffer: settings.capture.activeBuffer,
      cursor: settings.capture.cursor,
      modes: settings.capture.modes,
      renderCount: settings.capture.renderCount,
      parsedCount: settings.capture.parsedCount,
    },
    assertions: settings.assertions,
    screenshot: {
      width: screenshot.width,
      height: screenshot.height,
      sha256: screenshot.sha256,
      pngSignatureValid: screenshot.pngSignatureValid,
      bytes: screenshot.bytes,
    },
    captures,
    cleanup: { result: "PASS", ...settings.cleanup },
  });
  const safeRawBytes = Buffer.isBuffer(safeRaw) ? safeRaw : Buffer.from(safeRaw);
  assertSecretFree(Buffer.concat([safeRawBytes, Buffer.from(JSON.stringify({
    capture: safeCapture,
    interactions: safeInteractions,
    metadata,
  }))]));
  await writeJson(files.metadata, metadata);
  const artifacts = {};
  for (const [name, path] of Object.entries(files)) {
    artifacts[name] = name === "screenshot"
      ? screenshot
      : await fileReceipt(path, settings.evidenceDir);
  }
  for (const capture of captures) {
    artifacts[`capture-${String(capture.index).padStart(3, "0")}`] = {
      ...capture,
      png: {
        width: capture.width,
        height: capture.height,
        pngSignatureValid: capture.pngSignatureValid,
      },
    };
  }
  const manifestPath = join(settings.evidenceDir, "artifact-manifest.json");
  await writeJson(manifestPath, {
    schemaVersion: "harness-xterm-artifacts-v1",
    result: "PASS",
    generatedAt,
    scenario: settings.contract.name,
    treeHash: settings.sourceTree.hash,
    artifacts,
  });
  const manifest = await readFile(manifestPath);
  await writeJson(join(settings.evidenceDir, "PASS.json"), {
    schemaVersion: "harness-xterm-pass-v1",
    result: "PASS",
    generatedAt,
    scenario: settings.contract.name,
    manifest: { path: "artifact-manifest.json", sha256: sha256(manifest) },
    cleanup: {
      pty: settings.cleanup.pty.childExited && !settings.cleanup.pty.processGroupAlive,
      browser: settings.cleanup.browser.contextClosed
        && !settings.cleanup.browser.browserConnectedAfterClose
        && settings.cleanup.browser.profileRemoved,
      tempRoot: settings.cleanup.tempRootRemoved,
    },
  });
  return manifestPath;
}

export async function writeFailureEvidence(settings) {
  await mkdir(settings.evidenceDir, { recursive: true, mode: 0o700 });
  await chmod(settings.evidenceDir, 0o700);
  const failure = redactEvidence({
    schemaVersion: "harness-xterm-failure-v1",
    result: "FAIL",
    generatedAt: new Date().toISOString(),
    scenario: settings.scenario ?? null,
    error: errorRecord(settings.error),
    cleanup: settings.cleanup ?? null,
    debug: settings.debug ?? null,
  });
  assertSecretFree(failure);
  await writeJson(join(settings.evidenceDir, "failure.json"), failure);
}

function errorRecord(error) {
  if (error instanceof Error) {
    return { name: error.name, message: error.message, stack: error.stack };
  }
  return { name: "UnknownError", message: String(error) };
}

async function writeJson(path, value) {
  const safe = redactEvidence(value);
  assertSecretFree(safe);
  await writePrivate(path, `${JSON.stringify(safe, null, 2)}\n`);
}

async function writePrivate(path, value) {
  await writeFile(path, value, { mode: 0o600 });
  await chmod(path, 0o600);
}

function assertCleanup(cleanup) {
  const pty = cleanup?.pty;
  const browser = cleanup?.browser;
  const valid = pty?.childExited
    && !pty.processGroupAlive
    && pty.stdinClosed
    && (pty.temporarySockets?.length ?? 0) === 0
    && browser?.pageClosed
    && browser.contextClosed
    && !browser.browserConnectedAfterClose
    && browser.profileRemoved
    && (browser.boundPorts?.length ?? 0) === 0
    && cleanup.tempRootRemoved;
  if (!valid) throw new Error("cleanup invariants failed; refusing PASS evidence");
}

async function pngReceipt(path, root) {
  const receipt = await fileReceipt(path, root);
  const bytes = await readFile(path);
  if (bytes.length < 24 || !bytes.subarray(0, 8).equals(Buffer.from("89504e470d0a1a0a", "hex"))) {
    throw new Error(`${receipt.path} is not a PNG`);
  }
  if (bytes.readUInt32BE(8) !== 13 || bytes.subarray(12, 16).toString("ascii") !== "IHDR") {
    throw new Error(`${receipt.path} has no PNG IHDR`);
  }
  return {
    ...receipt,
    width: bytes.readUInt32BE(16),
    height: bytes.readUInt32BE(20),
    pngSignatureValid: true,
  };
}

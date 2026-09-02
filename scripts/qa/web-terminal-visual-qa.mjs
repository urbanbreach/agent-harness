#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { access, chmod, copyFile, mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { openBrowserTerminal } from "./lib/browser-terminal.mjs";
import { helpText, parseArgs, scenarioContract } from "./lib/config.mjs";
import { prepareEvidence, writeFailureEvidence, writePassEvidence } from "./lib/evidence.mjs";
import {
  assertBuiltExecutable,
  currentTree,
  fileReceipt,
  sha256,
} from "./lib/provenance.mjs";
import { createCleanupOwner } from "./lib/cleanup.mjs";
import {
  prepareHarnessWorkspace,
  removeTempRoot,
  resolveCommand,
  spawnHarnessPty,
} from "./lib/pty-session.mjs";
import { validateEvidenceDir } from "./lib/security.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(scriptPath), "..", "..");
let failureEvidenceWritten = false;

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    process.stdout.write(helpText());
    return;
  }
  const evidenceDir = await validateEvidenceDir(options.evidenceDir, repoRoot);
  await prepareEvidence(evidenceDir);
  const contract = scenarioContract(options, process.env);
  await access(options.browser);
  const sourceTree = await currentTree(repoRoot);
  const scriptSha256 = (await fileReceipt(scriptPath, dirname(scriptPath))).sha256;
  const tempRoot = await mkdtemp(join(tmpdir(), "harness-xterm-qa-"));
  const interactions = [];
  const cleanup = {
    pty: { childExited: true, processGroupAlive: false, stdinClosed: true, temporarySockets: [] },
    browser: {
      pageClosed: true,
      contextClosed: true,
      browserConnectedAfterClose: false,
      profileRemoved: true,
      boundPorts: [],
    },
    tempRootRemoved: false,
  };
  const cleanupOwner = createCleanupOwner(cleanup, {
    beforeTempRootRemoval: async () => {
      if (sourceBefore && testedBinary && testedBefore) {
        binaryProvenance = assertBuiltExecutable({
          sourceTree,
          sourceTreeAfter: await currentTree(repoRoot),
          sourceBefore,
          testedBefore,
          testedAfter: await fileReceipt(testedBinary, tempRoot),
        });
      }
    },
    removeTempRoot,
  });
  cleanupOwner.ownTempRoot(tempRoot);
  const uninstallSignals = cleanupOwner.installSignalHandlers();
  let fixture;
  let command;
  let terminal;
  let pty;
  let capture;
  let captures = [];
  let raw = Buffer.alloc(0);
  let browserMetadata;
  let runError;
  let exitResult;
  let binaryProvenance = null;
  let sourceBinary = null;
  let sourceBefore = null;
  let testedBinary = null;
  let testedBefore = null;

  try {
    fixture = await prepareHarnessWorkspace(tempRoot);
    if (contract.command.startsWith("harness ")) {
      if (sourceTree.dirty) {
        throw new Error("refusing shipped-binary QA from a dirty source tree");
      }
      execFileSync("cargo", ["build", "-p", "harness"], {
        cwd: repoRoot,
        stdio: "inherit",
      });
      sourceBinary = resolve(repoRoot, "target/debug/harness");
      sourceBefore = await fileReceipt(sourceBinary, repoRoot);
      testedBinary = join(tempRoot, "harness-under-test");
      await copyFile(sourceBinary, testedBinary);
      await chmod(testedBinary, 0o700);
      testedBefore = await fileReceipt(testedBinary, tempRoot);
    }
    command = await resolveCommand(contract.command, repoRoot, testedBinary);
    terminal = await openBrowserTerminal({
      browser: options.browser,
      cols: options.cols,
      rows: options.rows,
      timeoutMs: options.timeoutMs,
      title: contract.title,
      profilePath: join(tempRoot, "browser-profile"),
      onInput: (data) => pty?.write(data) ?? false,
    });
    cleanupOwner.ownBrowser(terminal);
    pty = spawnHarnessPty({
      command,
      cols: options.cols,
      rows: options.rows,
      cwd: fixture.workspace,
      sessionDir: fixture.sessionDir,
      tempRoot,
      onOutput: (bytes) => terminal.write(bytes),
    });
    cleanupOwner.ownPty(pty);
    const actionResult = await executeActions({
      actions: contract.actions,
      terminal,
      pty,
      interactions,
      evidenceDir,
    });
    capture = actionResult.capture;
    captures = actionResult.captures;
    const assertions = contract.assertions.map((marker) => ({
      marker,
      visible: capture.text.includes(marker),
    }));
    const failed = assertions.filter(({ visible }) => !visible);
    if (failed.length > 0) {
      throw new Error(`capture is missing required marker(s): ${failed.map(({ marker }) => marker).join(", ")}`);
    }
    if (contract.expectNaturalExit) {
      exitResult = await pty.waitForExit(options.timeoutMs);
      if (exitResult.code !== 0) throw new Error(`Harness PTY exited with code ${exitResult.code}`);
    }
    browserMetadata = await terminal.metadata();
  } catch (error) {
    runError = error;
  } finally {
    if (terminal) {
      try {
        browserMetadata ??= await terminal.metadata();
      } catch (error) {
        runError ??= error;
      }
    }
    try {
      await cleanupOwner.cleanup();
    } catch (error) {
      runError ??= error;
    }
    if (pty) raw = pty.raw();
    uninstallSignals();
  }

  if (runError) {
    await writeFailureEvidence({
      evidenceDir,
      scenario: options.scenario,
      error: runError,
      cleanup,
      debug: {
        interactions,
        ptyBytes: raw.length,
        ptyTail: raw.subarray(Math.max(0, raw.length - 4000)).toString("utf8"),
        terminalText: browserMetadata?.terminal?.text ?? null,
      },
    });
    failureEvidenceWritten = true;
    throw runError;
  }
  const assertions = contract.assertions.map((marker) => ({ marker, visible: true }));
  const manifest = await writePassEvidence({
    evidenceDir,
    contract,
    command,
    argv: process.argv.slice(2),
    repoRoot,
    browser: options.browser,
    browserMetadata,
    sourceTree,
    scriptSha256,
    binaryProvenance,
    raw,
    capture,
    interactions,
    captures,
    assertions,
    cleanup: { ...cleanup, naturalExit: exitResult ?? null },
  });
  process.stdout.write(`PASS scenario=${contract.name} evidence=${evidenceDir} manifest=${manifest}\n`);
}

export async function executeActions(settings) {
  const captures = [];
  let capture;
  let captureIndex = 0;
  for (const [index, action] of settings.actions.entries()) {
    const startedAt = new Date().toISOString();
    try {
      let result;
      if (action.kind === "wait") result = await settings.terminal.waitForText(action.value);
      else if (action.kind === "waitAbsent") result = await settings.terminal.waitForTextAbsent(action.value);
      else if (action.kind === "waitTitle") result = await settings.terminal.waitForTitle(action.value);
      else if (action.kind === "waitCount") {
        result = await settings.terminal.waitForTextCount(action.value, action.count);
      }
      else if (action.kind === "waitFrame") {
        result = await settings.terminal.waitForFrame(action);
      }
      else if (action.kind === "assertCount") {
        result = await settings.terminal.snapshot();
        const actual = result.text.split(action.value).length - 1;
        if (actual !== action.count) {
          throw new Error(`expected ${action.count} occurrence(s) of ${action.value}, found ${actual}`);
        }
      }
      else if (action.kind === "assertTitleCount") {
        result = await settings.terminal.snapshot();
        const actual = result.titleHistory.filter((title) => title === action.value).length;
        if (actual !== action.count) {
          throw new Error(`expected ${action.count} title occurrence(s) of ${action.value}, found ${actual}`);
        }
      }
      else if (action.kind === "type") await settings.terminal.type(action.value);
      else if (action.kind === "key") await settings.terminal.key(action.value);
      else if (action.kind === "click") result = await settings.terminal.clickText(action.value);
      else if (action.kind === "clickCell") {
        result = await settings.terminal.mouseCell("click", action.column, action.row);
      }
      else if (action.kind === "mouseDown") {
        result = await settings.terminal.mouseCell("down", action.column, action.row);
      }
      else if (action.kind === "mouseUp") {
        result = await settings.terminal.mouseCell("up", action.column, action.row);
      }
      else if (action.kind === "capture") {
        await settings.pty.flush();
        captureIndex += 1;
        const capturePath = join(
          settings.evidenceDir,
          `capture-${String(captureIndex).padStart(3, "0")}.png`,
        );
        capture = await settings.terminal.capture(capturePath);
        captures.push({ index: captureIndex, path: capturePath });
        result = { screenshot: `capture-${String(captureIndex).padStart(3, "0")}.png` };
      } else throw new Error(`unsupported action kind: ${action.kind}`);
      const after = capture && action.kind === "capture" ? capture : await settings.terminal.snapshot();
      const interaction = {
        index,
        startedAt,
        completedAt: new Date().toISOString(),
        action,
        result,
        bufferSha256: sha256(after.text),
        cursor: after.cursor,
      };
      if (action.kind === "capture") interaction.bufferSnapshot = after;
      settings.interactions.push(interaction);
    } catch (error) {
      throw new Error(`action ${index} (${action.kind}:${action.value ?? ""}) failed`, { cause: error });
    }
  }
  await settings.pty.flush();
  if (captures.length === 0) {
    capture = await settings.terminal.capture(join(settings.evidenceDir, "terminal.png"));
  } else {
    await copyFile(captures.at(-1).path, join(settings.evidenceDir, "terminal.png"));
  }
  return { capture, captures };
}

if (resolve(process.argv[1] ?? "") === scriptPath) {
  main().catch(async (error) => {
    try {
      await persistTopLevelFailureEvidence(
        process.argv.slice(2),
        error,
        failureEvidenceWritten,
      );
    } catch (evidenceError) {
      if (!(evidenceError instanceof Error && evidenceError.message.startsWith("unsafe evidence directory:"))) {
        process.stderr.write(`failure evidence error: ${evidenceError instanceof Error ? evidenceError.message : String(evidenceError)}\n`);
      }
    }
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}

export async function persistTopLevelFailureEvidence(argv, error, alreadyWritten) {
  if (alreadyWritten) return;
  const evidenceArgument = argumentValue(argv, "--evidence-dir");
  if (!evidenceArgument) return;
  const evidenceDir = await validateEvidenceDir(evidenceArgument, repoRoot);
  await prepareEvidence(evidenceDir);
  await writeFailureEvidence({
    evidenceDir,
    scenario: argumentValue(argv, "--scenario"),
    error,
  });
}

function argumentValue(argv, option) {
  const index = argv.indexOf(option);
  return index >= 0 ? argv[index + 1] : undefined;
}

import { execFileSync, spawn } from "node:child_process";
import { access, mkdir, rm, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { safePtyEnvironment } from "./security.mjs";

export async function prepareHarnessWorkspace(tempRoot) {
  const workspace = join(tempRoot, "workspace");
  const sessionDir = join(tempRoot, "sessions");
  await mkdir(workspace, { recursive: true });
  await mkdir(sessionDir, { recursive: true });
  await writeFile(join(workspace, "README.md"), "# Harness xterm QA fixture\n");
  run("git", ["init", "-q", "-b", "main"], workspace);
  run("git", ["config", "user.email", "xterm-qa@example.invalid"], workspace);
  run("git", ["config", "user.name", "Harness xterm QA"], workspace);
  run("git", ["add", "README.md"], workspace);
  run("git", ["commit", "-q", "-m", "seed fixture"], workspace);
  return { workspace, sessionDir };
}

export async function resolveCommand(command, repoRoot, harnessBinary = null) {
  const rootedCommand = command.replaceAll("$HARNESS_QA_REPO_ROOT", quote(repoRoot));
  if (!rootedCommand.startsWith("harness ")) return rootedCommand;
  const binary = harnessBinary ?? resolve(repoRoot, "target/debug/harness");
  try {
    await access(binary);
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") {
      throw new Error(`Harness binary is missing at ${binary}; run cargo build -p harness`);
    }
    throw error;
  }
  return `${quote(binary)}${rootedCommand.slice("harness".length)}`;
}

export function spawnHarnessPty(settings) {
  const actions = [];
  const chunks = [];
  const stderr = [];
  const shellCommand = `stty cols ${settings.cols} rows ${settings.rows}; exec ${settings.command}`;
  const environment = safePtyEnvironment(process.env, {
    HARNESS_DATA_HOME: join(settings.tempRoot, "data"),
    HARNESS_DETERMINISTIC: "1",
    HARNESS_QA_SESSION_DIR: settings.sessionDir,
    HARNESS_SEED: "42",
    LANG: "C.UTF-8",
    LC_ALL: "C.UTF-8",
    TERM: "xterm-256color",
    TZ: "UTC",
  });
  if (settings.disableAnimations !== false) environment.HARNESS_DISABLE_ANIMATIONS = "1";
  if (settings.environment) {
    for (const key of ["TERM", "TERM_PROGRAM", "COLORTERM", "NO_COLOR", "HARNESS_TUI_REDUCED_MOTION"]) {
      delete environment[key];
    }
    Object.assign(environment, settings.environment);
  }
  const child = spawn("script", ["-qefc", shellCommand, "/dev/null"], {
    cwd: settings.cwd,
    detached: true,
    env: environment,
    stdio: ["pipe", "pipe", "pipe"],
  });
  const startedAt = new Date().toISOString();
  let dimensions = { cols: settings.cols, rows: settings.rows };
  actions.push({ at: startedAt, action: "spawn", pid: child.pid, command: settings.command });
  let outputChain = Promise.resolve();
  let outputFailure;
  child.stdout.on("data", (chunk) => {
    const bytes = Buffer.from(chunk);
    chunks.push(bytes);
    actions.push({ at: new Date().toISOString(), action: "pty-output", bytes: bytes.length });
    outputChain = outputChain.then(() => settings.onOutput(bytes)).catch((error) => {
      outputFailure = error;
    });
  });
  child.stderr.on("data", (chunk) => stderr.push(Buffer.from(chunk)));
  const exit = new Promise((resolveExit) => {
    child.once("exit", (code, signal) => resolveExit({ code, signal, at: new Date().toISOString() }));
  });

  return {
    pid: child.pid,
    command: settings.command,
    actions,
    write(data) {
      if (child.stdin.destroyed || !child.stdin.writable) return false;
      child.stdin.write(data);
      actions.push({ at: new Date().toISOString(), action: "pty-input", bytes: Buffer.byteLength(data) });
      return true;
    },
    async resize(cols, rows) {
      if (!Number.isSafeInteger(cols) || cols <= 0 || !Number.isSafeInteger(rows) || rows <= 0) {
        throw new Error("PTY resize requires positive integer dimensions");
      }
      const tty = childTty(child.pid);
      const before = { ...dimensions };
      execFileSync("stty", ["--file", tty, "cols", String(cols), "rows", String(rows)]);
      const after = ttySize(tty);
      if (after.cols !== cols || after.rows !== rows) {
        throw new Error(`PTY resize did not apply ${cols}x${rows}; got ${after.cols}x${after.rows}`);
      }
      dimensions = after;
      const receipt = { before, after, mechanism: "TIOCSWINSZ", tty };
      actions.push({ at: new Date().toISOString(), action: "pty-resize", ...receipt });
      return receipt;
    },
    async flush() {
      await outputChain;
      if (outputFailure) throw outputFailure;
    },
    async waitForExit(timeoutMs) {
      const result = await bounded(exit, timeoutMs, "Harness PTY exit");
      await this.flush();
      return result;
    },
    raw() {
      return Buffer.concat(chunks);
    },
    stderr() {
      return Buffer.concat(stderr).toString("utf8");
    },
    async cleanup() {
      child.stdin.end();
      let terminatedByCleanup = false;
      if (alive(child.pid)) {
        terminatedByCleanup = true;
        signalGroup(child.pid, "SIGTERM");
        try {
          await bounded(exit, 3000, "Harness PTY SIGTERM");
        } catch (error) {
          if (!(error instanceof TimeoutError)) throw error;
          signalGroup(child.pid, "SIGKILL");
          await bounded(exit, 3000, "Harness PTY SIGKILL");
        }
      }
      await this.flush();
      return {
        at: new Date().toISOString(),
        childPid: child.pid,
        childExited: !alive(child.pid),
        processGroupAlive: aliveGroup(child.pid),
        stdinClosed: child.stdin.destroyed || child.stdin.writableEnded,
        terminatedByCleanup,
        temporarySockets: [],
      };
    },
  };
}

export async function removeTempRoot(path) {
  await rm(path, { recursive: true, force: true });
  return true;
}

class TimeoutError extends Error {
  constructor(label) {
    super(`${label} timed out`);
    this.name = "TimeoutError";
  }
}

function bounded(promise, timeoutMs, label) {
  return new Promise((resolvePromise, rejectPromise) => {
    const timer = setTimeout(() => rejectPromise(new TimeoutError(label)), timeoutMs);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolvePromise(value);
      },
      (error) => {
        clearTimeout(timer);
        rejectPromise(error);
      },
    );
  });
}

function run(command, args, cwd) {
  execFileSync(command, args, { cwd, stdio: "ignore" });
}

function childTty(pid) {
  const entries = execFileSync("ps", ["-o", "tty=", "--ppid", String(pid)], { encoding: "utf8" })
    .split("\n")
    .map((value) => value.trim())
    .filter((value) => value && value !== "?");
  const tty = entries.find((value) => value.startsWith("pts/")) ?? entries[0];
  if (!tty || !/^[A-Za-z0-9/_-]+$/.test(tty)) {
    throw new Error(`unable to resolve live PTY for process ${pid}`);
  }
  return `/dev/${tty}`;
}

function ttySize(tty) {
  const output = execFileSync("stty", ["--file", tty, "size"], { encoding: "utf8" }).trim();
  const match = /^(\d+)\s+(\d+)$/.exec(output);
  if (!match) throw new Error(`invalid PTY size receipt: ${output}`);
  return { cols: Number(match[2]), rows: Number(match[1]) };
}

function quote(value) {
  return `'${value.replaceAll("'", "'\\''")}'`;
}

function alive(pid) {
  if (!pid) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ESRCH") return false;
    throw error;
  }
}

function aliveGroup(pid) {
  if (!pid) return false;
  try {
    process.kill(-pid, 0);
    return true;
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ESRCH") return false;
    throw error;
  }
}

function signalGroup(pid, signal) {
  try {
    process.kill(-pid, signal);
  } catch (error) {
    if (!(error instanceof Error && "code" in error && error.code === "ESRCH")) throw error;
  }
}

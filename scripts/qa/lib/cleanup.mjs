export function createCleanupOwner(state, dependencies = {}) {
  const removeTempRoot = dependencies.removeTempRoot ?? (async () => true);
  let pty;
  let browser;
  let tempRoot;
  let cleanupPromise;
  let cleanupStarted = false;

  const owner = {
    ownPty(resource) {
      rejectLateResource("PTY", async () => {
        state.pty = await resource.cleanup();
      });
      pty = resource;
    },
    ownBrowser(resource) {
      rejectLateResource("browser", async () => {
        state.browser = await resource.close();
      });
      browser = resource;
    },
    ownTempRoot(path) {
      rejectLateResource("temporary root", async () => {
        state.tempRootRemoved = await removeTempRoot(path);
      });
      tempRoot = path;
    },
    cleanup() {
      cleanupStarted = true;
      cleanupPromise ??= cleanupResources();
      return cleanupPromise;
    },
    async handleSignal(signal, target = process) {
      target.exitCode = signal === "SIGINT" ? 130 : 143;
      await owner.cleanup();
    },
    installSignalHandlers(target = process) {
      const onInterrupt = () => void owner.handleSignal("SIGINT", target).catch(reportCleanupFailure);
      const onTerminate = () => void owner.handleSignal("SIGTERM", target).catch(reportCleanupFailure);
      target.once("SIGINT", onInterrupt);
      target.once("SIGTERM", onTerminate);
      return () => {
        target.off("SIGINT", onInterrupt);
        target.off("SIGTERM", onTerminate);
      };
    },
  };

  async function cleanupResources() {
    const failures = [];
    if (pty) {
      try {
        state.pty = await pty.cleanup();
      } catch (error) {
        failures.push(error);
      }
    }
    if (browser) {
      try {
        state.browser = await browser.close();
      } catch (error) {
        failures.push(error);
      }
    }
    if (tempRoot) {
      try {
        state.tempRootRemoved = await removeTempRoot(tempRoot);
      } catch (error) {
        failures.push(error);
      }
    }
    if (failures.length > 0) throw new AggregateError(failures, "xterm QA cleanup failed");
    return state;
  }

  function rejectLateResource(label, close) {
    if (!cleanupStarted) return;
    cleanupPromise = (cleanupPromise ?? Promise.resolve()).then(close, close);
    throw new Error(`cannot acquire ${label} after xterm QA cleanup started`);
  }

  return owner;
}

function reportCleanupFailure(error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
}

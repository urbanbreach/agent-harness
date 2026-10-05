(() => {
  const { core, internals } = __bootstrap;
  for (const name of ["node:util", "node:fs/promises", "node:path", "node:process", "node:async_hooks"]) {
    core.createLazyLoader(name)();
  }
  // Snapshot compilation runs before runtime arguments exist. Initialize the
  // preloaded process module once the worker has bootstrapped its real environment.
  globalThis.__harness_bootstrap_node = () => {
    const args = internals.__nodeBootstrapArgs;
    if (args) internals.__bootstrapNodeProcess(args.argv0, args.denoArgs, args.denoVersion, args.nodeDebug ?? "", false, args.runningOnMainThread);
  };
})();

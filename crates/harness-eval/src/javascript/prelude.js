(async () => {
  globalThis.__harness_bootstrap_node();
  delete globalThis.__harness_bootstrap_node;
  const native = globalThis.__harness_ops;
  delete globalThis.__harness_ops;
  const registry = globalThis.__harness_create_registry(native);
  delete globalThis.__harness_create_registry;
  const { inspect } = await import("node:util");
  const fs = await import("node:fs/promises");
  const path = await import("node:path");
  const process = (await import("node:process")).default;
  const { AsyncLocalStorage } = await import("node:async_hooks");
  const context = new AsyncLocalStorage();
  let active = null;
  const running = new Set(), scopes = new Map();
  const emit = event => {
    const cellId = context.getStore();
    if (cellId && running.has(cellId)) native.emit({ ...event, cellId });
  };
  const call = (operation, args) => {
    const cellId = context.getStore();
    if (!cellId || !running.has(cellId)) return Promise.reject(new Error("eval cell is no longer active"));
    const scope = scopes.get(cellId), policy = scope?.tools;
    if (scope && (operation === "agent" || operation === "workpool" && args.op === "create")) return Promise.reject(Object.assign(new Error(`Kernel tools may not invoke ${operation}()`), { name: "KernelToolError", code: "kernel_tool_recursion" }));
    const name = operation === "tool" ? args.name : ({ agent: "__agent__", output: "__output__", schema: "__schema__", workpool: "workpool" })[operation];
    const valid = value => Array.isArray(value) && value.every(name => typeof name === "string");
    let reason;
    if (name && policy && typeof policy === "object") {
      if (policy.deny !== undefined && (!valid(policy.deny) || policy.deny.includes(name))) reason = "deny";
      else if (policy.allow !== undefined && (!valid(policy.allow) || !policy.allow.includes(name))) reason = "allow";
    }
    if (reason) return Promise.reject(Object.assign(new Error(`Host tool is outside this kernel tool call's scope: ${name} (${reason})`), { name: "KernelToolError", code: "kernel_tool_host_denied", details: { tool: name, call_id: scope.call_id, reason } }));
    const pending = native.call(operation, { ...args, cellId }).then(reply => {
      if (reply.cancelled) throw Object.assign(new Error("JS cell interrupted"), scope ? {code:"kernel_tool_stale"} : {});
      if (Array.isArray(reply.tools)) registry.refresh(reply.tools);
      if (reply.error !== undefined) throw Object.assign(new Error(reply.error.message ?? String(reply.error)), typeof reply.error === "object" ? reply.error : {});
      return reply.result;
    });
    pending.catch(() => {});
    return pending;
  };
  const encode = bytes => {
    let text = "";
    for (let i = 0; i < bytes.length; i += 8192) text += String.fromCharCode(...bytes.subarray(i, i + 8192));
    return btoa(text);
  };
  const format = value => typeof value === "string" ? value : inspect(value, { colors: false, depth: 5 });
  const outputText = (data, stream = "stdout") => { emit({ type: "text", stream, data }); };
  const status = event => { emit({ type: "status", event }); };
  let pendingDisplays = [], localRoot = "", width = 4;
  const environment = new Map();
  const contributed = new Set();
  let baseline;
  const image = (mimeType, dataBase64) => { emit({ type: "display", mimeType, dataBase64 }); };
  const bytesOf = value => value instanceof ArrayBuffer ? new Uint8Array(value) : ArrayBuffer.isView(value) ? new Uint8Array(value.buffer,value.byteOffset,value.byteLength) : undefined;
  function imageData(value) {
    const bytes = bytesOf(value);
    if (bytes) return encode(bytes);
    if (value?.type === "Buffer" && Array.isArray(value.data)) value = value.data;
    if (typeof value === "string") {
      value = value.replace(/^data:[^,]*;base64,/, "");
      if (/^\d{1,3}(,\d{1,3})+$/.test(value)) value = value.split(",").map(Number);
      else {
        const compact = value.replace(/\s/g, "").replaceAll("-", "+").replaceAll("_", "/");
        const padded = compact.padEnd(Math.ceil(compact.length / 4) * 4, "=");
        if (padded && /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(padded)) return padded;
      }
    }
    if (Array.isArray(value) && value.every(n=>Number.isInteger(n) && n >= 0 && n < 256)) return encode(Uint8Array.from(value));
    return undefined;
  }
  function binaryMime(bytes) {
    if (bytes[0] === 137 && bytes[1] === 80 && bytes[2] === 78 && bytes[3] === 71) return "image/png";
    if (bytes[0] === 255 && bytes[1] === 216) return "image/jpeg";
    if (bytes[0] === 71 && bytes[1] === 73 && bytes[2] === 70) return "image/gif";
    if (bytes[0] === 82 && bytes[1] === 73 && bytes[8] === 87 && bytes[9] === 69) return "image/webp";
    if (bytes[0] === 66 && bytes[1] === 77) return "image/bmp";
    return undefined;
  }
  function display(value) {
    if (value && typeof value === "object") {
      if (value.type === "markdown" && typeof value.text === "string") {
        return image("text/markdown", encode(new TextEncoder().encode(value.text)));
      }
      if (value.type === "image" && typeof value.mimeType === "string") {
        const data = imageData(value.data);
        return data === undefined ? outputText("[display: image dropped — unsupported image data]\n") : image(value.mimeType,data);
      }
      if (value.mimeType?.startsWith("image/") && typeof (value.dataBase64 ?? value.data) === "string") {
        return image(value.mimeType, value.dataBase64 ?? value.data);
      }
      if (ArrayBuffer.isView(value) || value instanceof ArrayBuffer) {
        const bytes = value instanceof ArrayBuffer ? new Uint8Array(value) : new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
        return image(binaryMime(bytes) ?? "application/octet-stream", encode(bytes));
      }
      if (typeof value.arrayBuffer === "function" || typeof value.bytes === "function") {
        const pending = Promise.resolve().then(()=>typeof value.bytes === "function" ? value.bytes() : value.arrayBuffer()).then(raw=>{
          const bytes = bytesOf(raw), mime = typeof value.type === "string" && value.type.startsWith("image/") ? value.type : bytes && binaryMime(bytes);
          if (!bytes || !mime) return outputText("[display: image dropped — bytes carry no recognizable image signature]\n");
          image(mime,encode(bytes));
        }).catch(error=>outputText(`[display: image dropped — encoding failed: ${error.message}]\n`));
        pendingDisplays.push(pending);
        return pending;
      }
      if (Array.isArray(value.images)) {
        if (value.text) outputText(`${value.text}\n`);
        for (const frame of value.images) display(frame);
        return;
      }
      try { return image("application/json", encode(new TextEncoder().encode(JSON.stringify(value)))); }
      catch (error) { if (!(error instanceof TypeError)) throw error; }
    }
    if (typeof value === "string" && /^data:image\/[\w.+-]+;base64,/.test(value)) {
      const comma = value.indexOf(",");
      return image(value.slice(5, value.indexOf(";")), value.slice(comma + 1));
    }
    outputText(`${format(value)}\n`);
  }
  function resolvePath(value) {
    if (typeof value !== "string") value = String(value);
    const scheme = /^([a-z][\w+.-]*):\/\//i.exec(value);
    if (!scheme) return path.resolve(value);
    if (scheme[1].toLowerCase() !== "local") throw new Error(`unsupported file protocol: ${scheme[1]}`);
    const relative = decodeURIComponent(value.slice(scheme[0].length)).replaceAll("\\", "/");
    if (path.isAbsolute(relative) || relative.split("/").includes("..")) throw new Error("local:// path escapes the session directory");
    const resolved = path.resolve(localRoot, relative);
    if (resolved !== localRoot && !resolved.startsWith(`${localRoot}${path.sep}`)) throw new Error("local:// path escapes the session directory");
    return resolved;
  }
  function options(name, values, keys) {
    const [first, ...rest] = values;
    if (first !== null && typeof first === "object" && !Array.isArray(first)) {
      if (rest.some(value => value != null)) throw new TypeError(`${name}() cannot mix object and positional options`);
      return first;
    }
    if (values.slice(keys.length).some(value => value != null)) throw new TypeError(`${name}() received too many options`);
    return Object.fromEntries(keys.flatMap((key, index) => values[index] == null ? [] : [[key, values[index]]]));
  }
  async function read(name, ...values) {
    const opts = options("read", values, ["offset", "limit"]);
    const target = resolvePath(name);
    let text = await fs.readFile(target, "utf8");
    const offset = opts.offset ?? 1, limit = opts.limit;
    if (!Number.isInteger(offset) || offset < 1 || (limit !== undefined && (!Number.isInteger(limit) || limit < 1))) throw new TypeError("read() offset and limit must be positive integers");
    if (offset !== 1 || limit !== undefined) text = text.split(/\r?\n/).slice(offset - 1, limit === undefined ? undefined : offset - 1 + limit).join("\n");
    status({ op: "read", path: target });
    return text;
  }
  async function write(name, content) {
    const target = resolvePath(name);
    if (content instanceof Blob) content = new Uint8Array(await content.arrayBuffer());
    else if (content instanceof ArrayBuffer) content = new Uint8Array(content);
    else if (ArrayBuffer.isView(content)) content = new Uint8Array(content.buffer, content.byteOffset, content.byteLength);
    if (typeof content !== "string" && !(content instanceof Uint8Array)) throw new TypeError("write() expects string, Blob, ArrayBuffer, or TypedArray data");
    await fs.mkdir(path.dirname(target), { recursive: true });
    await fs.writeFile(target, content);
    status({ op: "write", path: target });
    return target;
  }
  async function parallel(iterable) {
    const thunks = Array.from(iterable ?? []), results = new Array(thunks.length), errors = new Map();
    let next = 0;
    async function worker() {
      while (next < thunks.length) {
        const index = next++;
        try {
          if (typeof thunks[index] !== "function") throw new TypeError("parallel() expects an iterable of functions");
          results[index] = await thunks[index](index);
        } catch (error) { errors.set(index, error); }
      }
    }
    await Promise.all(Array.from({ length: Math.min(width, thunks.length) }, worker));
    if (errors.size) throw errors.get(Math.min(...errors.keys()));
    return results;
  }
  const namespace = new Proxy((fn, metadata) => registry.define(fn, metadata), {
    get(_target, name) {
      if (name === "then" || typeof name !== "string") return undefined;
      return parameters => call("tool", { name, parameters: parameters ?? {} });
    },
  });
  Object.assign(globalThis, {
    print: (...values) => outputText(`${values.map(format).join(" ")}\n`),
    display, read, write, parallel, tool: namespace, tools: namespace,
    log: message => { emit({ type: "log", message: String(message) }); },
    phase: title => { emit({ type: "phase", title: String(title) }); },
    env(key, value) {
      if (key === undefined || key === null || key === "") {
        const values = Object.fromEntries(Object.entries({ ...process.env, ...Object.fromEntries(environment) }).sort());
        status({op:"env", count:Object.keys(values).length, keys:Object.keys(values).slice(0,20)});
        return values;
      }
      key = String(key);
      if (value !== undefined) { value = String(value); environment.set(key, value); process.env[key] = value; }
      const result = environment.get(key) ?? process.env[key];
      status({ op: "env", key, value: result, action: value === undefined ? "get" : "set" });
      return result;
    },
    pipeline: async (items, ...stages) => {
      let results = Array.from(items ?? []);
      for (const stage of stages) {
        if (typeof stage !== "function") throw new TypeError("pipeline() stages must be functions");
        results = await parallel(results.map(value => () => stage(value)));
      }
      return results;
    },
    tool_schema: name => call("schema", name == null ? {} : { name: String(name) }),
    completion: (prompt, opts) => call("completion", { prompt, opts }),
    agent: (prompt, ...values) => call("agent", { prompt: String(prompt), ...options("agent", values, ["agent", "model", "label", "schema", "isolated", "apply", "merge"]) }),
    output: (...args) => {
      const options = args.length && typeof args.at(-1) === "object" && !Array.isArray(args.at(-1)) ? args.pop() : {};
      const ids = args.flat().map(value => typeof value === "object" ? value.id ?? value.handle : value).map(value => String(value).replace(/^agent:\/\//, ""));
      return call("output", { ids, ...options });
    },
    workpool: async (agent, name, options = {}) => {
      if (options === null || typeof options !== "object" || Array.isArray(options) || Object.keys(options).some(key => key !== "mode")) throw new TypeError("workpool() options only accept mode");
      const created = await call("workpool", { op: "create", agent, name, ...options });
      const pool_id = created.details?.pool_id;
      if (created.hasError || typeof pool_id !== "string") throw new Error(created.text || "workpool creation failed");
      return Object.freeze({ pool_id,
        push: items => call("workpool", { op: "push", pool_id, items }),
        close: () => call("workpool", { op: "close", pool_id }),
        inspect: () => call("workpool", { op: "inspect", pool_id }),
        cancel: () => call("workpool", { op: "cancel", pool_id }),
      });
    },
    __harness_begin(options) {
      active = options.cellId;
      running.add(active);
      registry.configure(options);
      localRoot = options.localRoot ?? path.resolve(".");
      width = Math.max(1, Math.trunc(options.parallelPoolWidth ?? 4));
      pendingDisplays = [];
      const preludes = options.preludes ?? [];
      const names = new Set(preludes.flatMap(prelude => prelude.exports));
      for (const name of contributed) if (!names.has(name)) { delete globalThis[name]; contributed.delete(name); }
      for (const prelude of preludes) {
        if (prelude.exports.some(name => !(name in globalThis))) (0, eval)(prelude.javascript);
        for (const name of prelude.exports) contributed.add(name);
      }
    },
    async __harness_drain() {
      while (pendingDisplays.length) { const pending = pendingDisplays; pendingDisplays = []; await Promise.all(pending); }
    },
    __harness_run: callback => context.run(active, callback),
    __harness_end() { running.delete(active); active = null; },
    __harness_globals() {
      let visited = 0;
      const estimate = (value, seen, depth = 0) => {
        if (typeof value === "string") return value.length * 2;
        if (!value || typeof value !== "object") return 8;
        if (seen.has(value) || visited++ > 50000 || depth > 5) return 0;
        seen.add(value);
        if (ArrayBuffer.isView(value) || value instanceof ArrayBuffer) return value.byteLength;
        let bytes = 32;
        for (const descriptor of Object.values(Object.getOwnPropertyDescriptors(value))) {
          if ("value" in descriptor) bytes += estimate(descriptor.value, seen, depth + 1);
          if (visited > 50000) break;
        }
        return bytes;
      };
      return Object.entries(Object.getOwnPropertyDescriptors(globalThis))
        .filter(([name, descriptor]) => !baseline.has(name) && "value" in descriptor)
        .map(([name, descriptor]) => ({ name, bytes: estimate(descriptor.value, new Set()), approximate: true }))
        .sort((a, b) => b.bytes - a.bytes).slice(0, 5);
    },
  });
  console.log = console.info = globalThis.print;
  console.error = console.warn = (...values) => outputText(`${values.map(format).join(" ")}\n`, "stderr");
  for (const stream of ["stdout", "stderr"]) process[stream].write = (chunk, encoding, callback) => {
    outputText(typeof chunk === "string" ? chunk : new TextDecoder(typeof encoding === "string" ? encoding : undefined).decode(chunk), stream);
    if (typeof encoding === "function") encoding(); else if (typeof callback === "function") callback();
    return true;
  };
  baseline = new Set(Object.getOwnPropertyNames(globalThis));
  const commands = async () => {
    for (;;) {
      const command = await native.command();
      if (!command) return;
      const id = command.id;
      running.add(id);
      scopes.set(id, { ...command.scope, call_id: command.request?.call_id });
      context.run(id, async () => {
        try {
          const value = command.type === "describe" ? registry.describe(command.names) : await registry.invoke(command.request);
          emit({ type: "result", ok: true, value });
        } catch (error) {
          emit({ type: "result", ok: false, error: { code: error.code ?? "kernel_tool_failed", message: error.message ?? String(error), details: error.details } });
        } finally { running.delete(id); scopes.delete(id); native.leave(id); }
      }).catch(() => {});
    }
  };
  commands().catch(() => {});
})()

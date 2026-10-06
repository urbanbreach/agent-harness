const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { registerHooks } = require('node:module');
const { pathToFileURL } = require('node:url');
const transform = require('./transform.cjs');
const createMemory = require('./memory.cjs');
const createCapture = require('./stdio.cjs');
const send = process.send.bind(process);
const pending = new Map(), contexts = new Map(), commands = [];
let active = '', sequence = 0, commandWaiter, options, memory, capture, protectedNames, queuedBytes = 0;
let managedPath;
registerHooks({resolve(specifier, context, nextResolve) {
  try { return nextResolve(specifier, context); }
  catch (error) {
    if (!managedPath || error.code !== 'ERR_MODULE_NOT_FOUND' || /^[./]|^[a-z]+:/i.test(specifier)) throw error;
    return nextResolve(specifier, {...context, parentURL: pathToFileURL(path.join(managedPath, 'cell.mjs')).href});
  }
}});
const limit = 32 * 1024 * 1024;
const scriptOptions = {
  filename: path.join(process.cwd(), '.harness-eval-cell.js'),
  importModuleDynamically: vm.constants.USE_MAIN_CONTEXT_DEFAULT_LOADER,
};
const evaluate = source => vm.runInThisContext(source, scriptOptions);
function transmit(event) {
  event.cellId ??= active;
  if (event.cellId && !contexts.has(event.cellId)) return;
  const size = Buffer.byteLength(JSON.stringify(event));
  if (size > limit || queuedBytes + size > limit) throw new Error('eval output exceeds the 32 MiB transport buffer');
  queuedBytes += size;
  send(event, error => {
    queuedBytes -= size;
    if (error) process.exit(1);
  });
}
function flush() {
  capture?.drain((stream, data) => {
    if (active) transmit({type: 'text', cellId: active, stream, data});
  });
}
function emit(event) {
  // Drain in the executor before each event so native writes cannot overtake print().
  flush();
  transmit(event);
}
function cancel(id) {
  for (const key of contexts.keys()) if (key === id || id === active) contexts.set(key, false);
  for (const [key, call] of pending) if (contexts.get(call.parent) === false) {
    pending.delete(key);
    call.resolve({cancelled: true});
  }
}
function leave(id) {
  contexts.delete(id);
  for (const [key, call] of pending) if (call.parent === id) {
    pending.delete(key);
    call.resolve({cancelled: true});
  }
}
const native = {
  emit, leave, parseFunction: transform.parseFunction,
  command: () => commands.length ? Promise.resolve(commands.shift()) : new Promise(resolve => { commandWaiter = resolve; }),
  call(operation, args) {
    const {cellId: parent, ...parameters} = args;
    if (!contexts.get(parent)) return Promise.resolve({cancelled: true});
    const id = String(sequence++);
    return new Promise((resolve, reject) => {
      pending.set(id, {parent, resolve});
      try { emit({type: 'call', id, cellId: parent, operation, args: parameters}); }
      catch (error) { pending.delete(id); reject(error); }
    });
  },
};
async function initialize(message) {
  options = message;
  const pointer = path.join(options.localRoot, 'environments/js/current.json');
  if (fs.existsSync(pointer)) managedPath = JSON.parse(fs.readFileSync(pointer, 'utf8'));
  memory = createMemory(options.memory);
  capture = createCapture(options.captureRoot);
  globalThis.__harness_ops = native;
  evaluate(fs.readFileSync(path.join(__dirname, 'tools.js'), 'utf8'));
  await evaluate(fs.readFileSync(path.join(__dirname, 'prelude.js'), 'utf8'));
  protectedNames = new Set(Object.getOwnPropertyNames(globalThis));
  emit({type: 'ready', runtime: {name: 'Node.js', version: process.versions.node, pid: process.pid}});
}
async function run(request) {
  if (!protectedNames || active) throw new Error('JavaScript kernel is not ready for a cell');
  active = request.id;
  contexts.set(active, true);
  capture.clear();
  memory.begin();
  const flusher = setInterval(() => {
    try { flush(); } catch (error) { send({type: 'fatal', error: error.message}); }
  }, 50);
  flusher.unref();
  const started = performance.now();
  let result;
  try {
    __harness_begin({...options, cellId: active, preludes: request.preludes, tools: request.tools});
    const value = await __harness_run(async () => {
      let code = request.code, filename = scriptOptions.filename;
      if (/^\s*%(?:load|npm)\b/.test(code)) {
        const command = code.trim();
        if (command.includes('\n') || command.includes('\r')) throw new Error('%load and %npm install require their own cell');
        if (command.startsWith('%load ')) {
          let name = command.slice(6).trim();
          if (name.startsWith('"')) name = JSON.parse(name);
          else if (name.startsWith("'") && name.endsWith("'")) name = name.slice(1, -1);
          if (typeof name !== 'string' || !name || /^[a-z]+:\/\//i.test(name)) throw new Error('%load requires a local path');
          filename = path.resolve(name);
          const descriptor = fs.openSync(filename, fs.constants.O_RDONLY | (fs.constants.O_NONBLOCK ?? 0));
          try {
            const stat = fs.fstatSync(descriptor);
            if (!stat.isFile() || stat.size > 8 * 1024 * 1024) throw new Error('%load requires a regular file of at most 8 MiB');
            const buffer = Buffer.alloc(8 * 1024 * 1024 + 1);
            let size = 0, count;
            while (size < buffer.length && (count = fs.readSync(descriptor, buffer, size, buffer.length - size, null))) size += count;
            if (size > 8 * 1024 * 1024) throw new Error('%load file exceeds 8 MiB');
            code = new TextDecoder('utf-8', {fatal:true}).decode(buffer.subarray(0, size));
          } finally { fs.closeSync(descriptor); }
        } else {
          const match = /^%npm (?:install|add) (.+)$/.exec(command);
          if (!match) throw new Error('use %npm install <packages>');
          const installed = await __harness_install(match[1]);
          managedPath = installed.path;
          print(installed.text);
          return;
        }
      }
      const source = transform.cell(code, protectedNames);
      const value = await vm.runInThisContext(source, {...scriptOptions, filename});
      await __harness_drain();
      return JSON.stringify(value);
    });
    result = {ok: true, valueRepr: value ?? null};
  } catch (error) {
    result = {ok: false, error: {message: error.stack ?? String(error)}};
  } finally { clearInterval(flusher); __harness_end(); }
  emit({type: 'result', ...result, durationMs: performance.now() - started, memory: memory.afterCell()});
  leave(active);
  active = '';
}
process.on('message', message => {
  const handle = async () => {
    switch (message.type) {
      case 'init': await initialize(message); break;
      case 'run': await run(message); break;
      case 'reply': {
        const call = pending.get(message.id);
        pending.delete(message.id);
        call?.resolve(message);
        break;
      }
      case 'cancel': cancel(message.id); break;
      case 'describe': case 'invoke':
        contexts.set(message.id, true);
        if (commandWaiter) { const resolve = commandWaiter; commandWaiter = null; resolve(message); }
        else if (commands.length < 32) commands.push(message);
        else throw new Error('eval kernel command queue is full');
        break;
      default: throw new Error('unknown eval worker message');
    }
  };
  handle().catch(error => send({type: 'fatal', error: error.stack ?? String(error)}));
});
process.on('disconnect', () => process.exit());

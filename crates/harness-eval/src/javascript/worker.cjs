// Keep the control pipe responsive while user code blocks, and keep native
// stdout/stderr writes out of the JSON protocol. Both processes live per kernel.
const fs = require('node:fs');
const path = require('node:path');
const { fork } = require('node:child_process');
const limit = 32 * 1024 * 1024;
let child, closing = false, pendingBytes = 0, input = '';

function stop(code = 0) {
  if (closing) return;
  closing = true;
  if (process.platform !== 'win32') {
    // Rust starts this supervisor as the process-group leader.
    try { process.kill(-process.pid, 'SIGKILL'); } catch {}
  }
  child?.kill('SIGKILL');
  process.exit(code);
}
function fail(error) {
  fs.writeSync(2, `eval Node.js worker: ${error.message ?? error}\n`);
  stop(1);
}
function write(message) {
  const frame = JSON.stringify(message) + '\n', size = Buffer.byteLength(frame);
  if (size > limit || pendingBytes + size > limit) throw new Error('eval output exceeds the 32 MiB transport buffer');
  pendingBytes += size;
  process.stdout.write(frame, () => { pendingBytes -= size; });
}
function receive(message) {
  if (message.type === 'shutdown') return stop();
  if (!child) {
    if (message.type !== 'init') throw new Error('eval worker requires initialization');
    const streams = ['stdout', 'stderr'].map(name => fs.openSync(path.join(message.captureRoot, name), 'a', 0o600));
    child = fork(path.join(__dirname, 'runtime.cjs'), [], {
      execArgv: ['--expose-gc', '--max-old-space-size=4096'],
      stdio: ['ignore', ...streams, 'ipc'],
    });
    streams.forEach(fd => fs.closeSync(fd));
    child.on('error', fail);
    child.on('exit', (code, signal) => fail(new Error(`JavaScript process exited (${signal ?? code})`)));
    child.on('message', event => {
      try {
        if (event.type === 'fatal') return fail(new Error(event.error));
        write(event);
      } catch (error) { fail(error); }
    });
  }
  child.send(message, error => { if (error) fail(error); });
}

if (Number(process.versions.node.split('.')[0]) < 24) {
  fail(new Error(`JavaScript eval requires Node.js 24 or newer; found ${process.version}`));
}
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => {
  try {
    input += chunk;
    let end;
    while ((end = input.indexOf('\n')) >= 0) {
      const line = input.slice(0, end);
      if (Buffer.byteLength(line) > limit) throw new Error('eval message exceeds 32 MiB');
      input = input.slice(end + 1);
      receive(JSON.parse(line));
    }
    if (Buffer.byteLength(input) > limit) throw new Error('eval message exceeds 32 MiB');
  } catch (error) { fail(error); }
});
process.stdin.on('end', () => stop());
process.stdin.on('error', fail);
process.stdout.on('error', () => stop());

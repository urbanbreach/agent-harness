import { expect, test } from 'bun:test';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

test('backend smoke requires durable run completion and rejects failed or interrupted runs', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'harness-smoke-test-'));
  const peer = join(directory, 'backend');
  await writeFile(
    peer,
    `#!${process.execPath}
const send = (message) => process.stdout.write(JSON.stringify(message) + '\\n');
const update = (value, sessionId = 'target') => send({ method: 'session.update', params: { sessionId, update: value } });
const event = (event_type, delivery = 'durable', sessionId = 'target') => update({ kind: 'event', delivery, event: { payload: { event_type, data: { error: 'mock run failed', reason: 'mock task failed' } } } }, sessionId);
let buffer = '';
for await (const chunk of Bun.stdin.stream()) {
  buffer += new TextDecoder().decode(chunk);
  while (buffer.includes('\\n')) {
    const end = buffer.indexOf('\\n');
    const request = JSON.parse(buffer.slice(0, end));
    buffer = buffer.slice(end + 1);
    const result = request.method === 'app.initialize'
      ? { home: process.env.HARNESS_HOME, mock: true }
      : request.method === 'session.create' ? { sessionId: 'target' } : null;
    send({ id: request.id, result });
    if (request.method === 'session.intent') {
      const scenario = process.env.SMOKE_SCENARIO;
      if (scenario === 'success') {
        event('task_completed', 'live');
        event('run_failed', 'durable', 'another-session');
        event('assistant_message_finished');
        event('task_completed');
      } else if (scenario === 'failed') {
        event('assistant_message_finished');
        event('task_cancelled');
      } else if (scenario === 'notice') {
        update({ kind: 'notice', level: 'error', message: 'mock error notice' });
        event('task_completed');
      } else if (scenario === 'exited' || scenario === 'error-exit') {
        update({ kind: 'exited', error: scenario === 'error-exit' ? 'mock session exit' : undefined });
      } else if (scenario === 'backend-exit') {
        process.exit(7);
      }
    }
  }
}
`,
    { mode: 0o700 },
  );
  try {
    for (const [scenario, error] of [
      ['success', undefined],
      ['failed', 'mock task failed'],
      ['notice', 'mock error notice'],
      ['exited', 'Session exited before durable run completion'],
      ['error-exit', 'mock session exit'],
      ['backend-exit', 'Backend exited (7)'],
    ] as const) {
      const child = Bun.spawn([process.execPath, join(import.meta.dir, 'backend-smoke.ts')], {
        env: { ...process.env, HARNESS_BIN: peer, SMOKE_SCENARIO: scenario },
        stdout: 'pipe',
        stderr: 'pipe',
      });
      try {
        const [code, stdout, stderr] = await Promise.all([
          child.exited,
          new Response(child.stdout).text(),
          new Response(child.stderr).text(),
        ]);
        if (error) {
          expect(code).not.toBe(0);
          expect(stderr).toContain(error);
          expect(stdout).not.toContain('updates:');
        } else {
          expect(code).toBe(0);
          expect(stdout).toContain('durable:task_completed');
        }
      } finally {
        child.kill();
      }
    }
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}, 40000);

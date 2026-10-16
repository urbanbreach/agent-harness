import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { BackendClient } from '../src/backend/client';

const cwd = await mkdtemp(join(tmpdir(), 'harness-gui-smoke-'));
const home = await mkdtemp(join(tmpdir(), 'harness-gui-home-'));
const previousHome = process.env.HARNESS_HOME;
process.env.HARNESS_HOME = home;
const client = new BackendClient([process.env.HARNESS_BIN || 'harness', 'gui-server', '--mock']);
const kinds: string[] = [];
const completed = Promise.withResolvers<void>();
// RPC setup may fail before we await completion; keep its rejection handled until then.
void completed.promise.catch(() => {});
let target: string | undefined;
const timer = setTimeout(
  () =>
    completed.reject(
      new Error(`Timed out waiting for mock completion; received ${kinds.join(', ')}`),
    ),
  30000,
);
client.subscribe((message) => {
  if (message.method !== 'session.update' || message.params.sessionId !== target) return;
  const update = message.params.update;
  kinds.push(
    update.kind === 'event' ? `${update.delivery}:${update.event.payload.event_type}` : update.kind,
  );
  if (update.kind === 'event' && update.delivery === 'durable') {
    if (update.event.payload.event_type === 'task_completed') completed.resolve();
    else if (update.event.payload.event_type === 'task_cancelled')
      completed.reject(new Error(update.event.payload.data.reason));
    else if (update.event.payload.event_type === 'run_failed')
      completed.reject(new Error(update.event.payload.data.error));
  } else if (update.kind === 'notice' && update.level === 'error') {
    completed.reject(new Error(update.message));
  } else if (update.kind === 'exited') {
    completed.reject(new Error(update.error || 'Session exited before durable run completion'));
  }
});
client.subscribeStatus(() => {
  const status = client.getStatus();
  if (status.state === 'failed' || status.state === 'closed')
    completed.reject(new Error(status.error || 'Backend closed before durable run completion'));
});
try {
  const initialized = await client.call('app.initialize', { cwd });
  console.log('initialize:', JSON.stringify(initialized));
  if (initialized.home !== home || !initialized.mock)
    throw new Error('Smoke backend did not use the isolated mock home');
  console.log('sessions:', JSON.stringify(await client.call('sessions.list', { cwd })));
  const session = await client.call('session.create', { cwd });
  target = session.sessionId;
  console.log('created:', JSON.stringify(session));
  await client.call('session.intent', {
    sessionId: target,
    intent: { kind: 'submit', text: 'Say hello in one sentence.' },
  });
  await completed.promise;
  console.log('updates:', kinds.join(', '));
  await client.call('session.close', { sessionId: target });
} finally {
  clearTimeout(timer);
  client.close();
  if (previousHome === undefined) delete process.env.HARNESS_HOME;
  else process.env.HARNESS_HOME = previousHome;
  await rm(cwd, { recursive: true, force: true });
  await rm(home, { recursive: true, force: true });
}

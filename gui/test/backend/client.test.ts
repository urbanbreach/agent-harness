import { expect, test } from 'bun:test';
import { BackendClient, BackendError, JsonLines } from '../../src/backend/client';

test('JSON lines survives split UTF-8 and merged frames and rejects truncated input', () => {
  const messages: unknown[] = [];
  const framing = new JsonLines((message) => messages.push(message));
  const bytes = new TextEncoder().encode('{"text":"hé"}\n{"id":2,"result":null}\n');
  const split = bytes.indexOf(195) + 1;
  framing.push(bytes.slice(0, split));
  framing.push(bytes.slice(split, split + 2));
  framing.push(bytes.slice(split + 2));
  framing.end();
  expect(messages).toEqual([{ text: 'hé' }, { id: 2, result: null }]);
  const truncated = new JsonLines(() => {});
  truncated.push(new TextEncoder().encode('{"id":1'));
  expect(() => truncated.end()).toThrow('incomplete');
});

test('child transport correlates out-of-order responses, delivers notifications, and propagates RPC errors and exit', async () => {
  const program = `
    const lines = new TextDecoderStream(); process.stdin.toWeb().pipeTo(lines.writable);
    let buffer = '', requests = [];
    for await (const chunk of lines.readable) {
      buffer += chunk;
      while (buffer.includes('\\n')) {
        const end = buffer.indexOf('\\n'); const request = JSON.parse(buffer.slice(0,end)); buffer = buffer.slice(end+1);
        requests.push(request);
        if (requests.length === 2) {
          process.stdout.write(JSON.stringify({id:requests[1].id,result:[process.env.HARNESS_GUI_TRANSPORT_TEST]})+'\\n'+JSON.stringify({method:'session.update',params:{sessionId:'only-this',update:{kind:'status',text:'ready'}}})+'\\n');
          process.stdout.write(JSON.stringify({id:requests[0].id,error:{code:'not_found',message:'missing session'}})+'\\n');
        }
        if (requests.length === 3) { process.stderr.write('synthetic diagnostic'); process.exit(9); }
      }
    }
  `;
  // Bun's stdin is a stream rather than Node's ReadStream.
  const source = program.replace('process.stdin.toWeb()', 'Bun.stdin.stream()');
  const previous = process.env.HARNESS_GUI_TRANSPORT_TEST;
  process.env.HARNESS_GUI_TRANSPORT_TEST = 'second';
  const client = new BackendClient([process.execPath, '-e', source]);
  if (previous === undefined) delete process.env.HARNESS_GUI_TRANSPORT_TEST;
  else process.env.HARNESS_GUI_TRANSPORT_TEST = previous;
  const notifications: string[] = [];
  client.subscribe((notification) => {
    if (notification.method === 'session.update') notifications.push(notification.params.sessionId);
  });
  try {
    const first = client.call('sessions.events', { runDir: '/missing' });
    const second = client.call('sessions.pins', { cwd: '/demo' });
    await expect(first).rejects.toBeInstanceOf(BackendError);
    expect(await second).toEqual(['second']);
    expect(notifications).toEqual(['only-this']);
    await expect(client.call('sessions.list', { cwd: '/demo' })).rejects.toThrow(
      'Backend exited (9)',
    );
    expect(client.getStatus()).toMatchObject({ state: 'failed', reconnectable: true });
    expect(client.getStderr()).toContain('synthetic diagnostic');
  } finally {
    client.close();
  }
});

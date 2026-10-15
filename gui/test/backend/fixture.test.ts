import { expect, test } from 'bun:test';
import { createBackend, FixtureBackend, type Scenario } from '../../src/backend/fixture';

test('fixture instances are deterministic and mutations never modify the authored scenario', async () => {
  const scenario = (await Bun.file(
    new URL('../fixtures/scenarios/conversation.json', import.meta.url),
  ).json()) as Scenario;
  const a = new FixtureBackend(scenario),
    b = new FixtureBackend(scenario);
  try {
    const rows = await a.call('sessions.list', { cwd: '/fixture/demo' });
    expect(rows.map((row) => row.runId)).toEqual(['chat', 'notes', 'draft']);
    await a.call('sessions.rename', { runDir: rows[0]!.runDir, title: 'Changed' });
    expect((await b.call('sessions.list', { cwd: '/fixture/demo' }))[0]!.catalog.run_name).toBe(
      'A short conversation',
    );
    const first = await a.call('session.create', { cwd: '/fixture/demo' });
    const second = await b.call('session.create', { cwd: '/fixture/demo' });
    expect(first).toEqual(second);
    const skill = (await a.call('skills.list', {})).skills[0]!;
    await a.call('skills.setEnabled', { id: skill.id, enabled: false });
    expect((await a.call('skills.list', {})).skills[0]!.enabled).toBe(false);
    expect((await b.call('skills.list', {})).skills[0]!.enabled).toBe(true);
    const blob = { mime: 'text/plain', dataBase64: Buffer.from('abc').toString('base64') };
    const { digest } = await a.call('blobs.put', blob);
    expect(digest).toBe(
      'attachment:blake3:6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85',
    );
    expect(await a.call('blobs.get', { digest })).toEqual(blob);
    expect(await b.call('blobs.get', { digest })).toBeNull();
    const updates: string[] = [];
    const delivered = Promise.withResolvers<void>();
    b.subscribe((message) => {
      if (message.method === 'session.update') {
        updates.push(`${message.params.sessionId}:${message.params.update.kind}`);
        delivered.resolve();
      }
    });
    await b.call('session.continue', { runDir: '/fixture/sessions/chat' });
    await delivered.promise;
    expect(updates).toEqual(['chat:status']);
  } finally {
    a.close();
    b.close();
  }
});

test('fixture environment selects each packaged scenario without spawning Harness', async () => {
  const previous = process.env.HARNESS_GUI_FIXTURE;
  try {
    for (const [name, count] of [
      ['empty', 0],
      ['project', 1],
      ['conversation', 1],
    ] as const) {
      process.env.HARNESS_GUI_FIXTURE = name;
      const backend = await createBackend();
      try {
        expect((await backend.call('app.initialize', {})).version).toBe('fixture');
        expect(await backend.call('state.read', { name: 'projects' })).toHaveLength(count);
      } finally {
        backend.close();
      }
    }
  } finally {
    if (previous === undefined) delete process.env.HARNESS_GUI_FIXTURE;
    else process.env.HARNESS_GUI_FIXTURE = previous;
  }
});

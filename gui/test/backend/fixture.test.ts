import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
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
    expect(await a.call('sessions.attachment', { runDir: rows[0]!.runDir, ref: digest })).toEqual({
      dataBase64: blob.dataBase64,
    });
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

test('fixture Markdown export writes visible session content and reports actual UTF-8 bytes', async () => {
  const scenario = (await Bun.file(
    new URL('../fixtures/scenarios/conversation.json', import.meta.url),
  ).json()) as Scenario;
  scenario.extensions = {
    ...scenario.extensions,
    'sessions.exportMarkdown': { dest: '/unused/authored-result.md', bytes: 512 },
  };
  const backend = new FixtureBackend(scenario);
  const directory = await mkdtemp(join(tmpdir(), 'harness-fixture-export-'));
  const dest = join(directory, 'nested', 'session.md');
  try {
    const result = await backend.call('sessions.exportMarkdown', {
      runDir: '/fixture/sessions/chat',
      dest,
    });
    const markdown = await readFile(dest, 'utf8');
    expect(markdown).toBe(
      '## User\n\nWhat does this project do?\n\n## Assistant\n\nIt is a small, synthetic desktop demo.\n\n',
    );
    expect(result).toEqual({ dest, bytes: Buffer.byteLength(markdown) });
    await expect(
      backend.call('sessions.exportMarkdown', { runDir: '/fixture/sessions/missing', dest }),
    ).rejects.toThrow('Fixture session not found');
  } finally {
    backend.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test('fixture environment selects each packaged scenario without spawning Harness', async () => {
  const previous = process.env.HARNESS_GUI_FIXTURE;
  const atlasSessions = [
    'release-review',
    'search-relevance',
    'health-checks',
    'onboarding-copy',
    'migration-checklist',
    'api-errors',
  ];
  try {
    for (const [name, count, cwd, sessionIds, theme] of [
      ['atlas-transcript-light', 2, '/tmp/harness-panel-repos/atlas', atlasSessions, 'light'],
      ['atlas-transcript', 2, '/tmp/harness-panel-repos/atlas', atlasSessions, 'dark'],
      ['composer-attachments', 2, '/tmp/composer-atlas', atlasSessions, 'dark'],
      ['composer-completions', 1, '/tmp/composer-atlas', [], null],
      ['composer-files', 2, '/tmp/composer-atlas', atlasSessions, 'dark'],
      ['composer-multiline', 2, '/tmp/composer-atlas', atlasSessions, 'dark'],
      ['composer-no-providers', 2, '/tmp/composer-atlas', atlasSessions, 'dark'],
      ['composer-queue', 2, '/tmp/composer-atlas', atlasSessions, 'dark'],
      ['composer-readonly', 1, '/tmp/composerlive-workspace', ['readonly'], 'dark'],
      ['composer-slash', 2, '/tmp/composer-atlas', atlasSessions, 'dark'],
      ['composer', 1, '/tmp/composer-atlas', [], 'dark'],
      ['conversation', 1, '/fixture/demo', ['chat', 'notes', 'draft'], null],
      ['empty', 0, '/fixture', [], null],
      ['palette-light', 2, '/tmp/harness-panel-repos/atlas', atlasSessions, 'light'],
      ['palette-query', 2, '/tmp/harness-panel-repos/atlas', atlasSessions, 'dark'],
      ['palette', 2, '/tmp/harness-panel-repos/atlas', atlasSessions, 'dark'],
      ['panel-dark', 2, '/tmp/harness-panel-repos/atlas', atlasSessions, 'dark'],
      ['panel-light', 2, '/tmp/harness-panel-repos/atlas', atlasSessions, 'light'],
      ['project', 1, '/fixture/demo', [], null],
      ['settings-accounts', 2, '/fixture/atlas', atlasSessions, null],
      ['settings-auth', 1, '/fixture/auth', [], null],
      ['settings-light', 2, '/fixture/atlas', atlasSessions, 'light'],
      ['settings', 2, '/fixture/atlas', atlasSessions, 'dark'],
      ['shell-blank', 2, '/tmp/harness-panel-repos/atlas', [], 'dark'],
      ['shellparity-blank-dark', 2, '/tmp/shellparity-projects/atlas', [], 'dark'],
      ['shellparity-blank-light', 2, '/tmp/shellparity-projects/atlas', [], 'light'],
      [
        'shellparity-branch',
        2,
        '/tmp/shellparity-projects/atlas',
        ['release-review', 'health-checks', 'migration-checklist', 'api-errors'],
        'dark',
      ],
      ['shellparity-narrow', 2, '/tmp/shellparity-projects/atlas', atlasSessions, 'dark'],
      ['shellparity-sidebar-dark', 2, '/tmp/shellparity-projects/atlas', atlasSessions, 'dark'],
      ['shellparity-sidebar-light', 2, '/tmp/shellparity-projects/atlas', atlasSessions, 'light'],
      ['shellparity-sidebar-seven', 2, '/tmp/shellparity-projects/atlas', atlasSessions, 'dark'],
      ['shellparity-sidebar-ten', 2, '/tmp/shellparity-projects/atlas', atlasSessions, 'dark'],
      [
        'shellparity-toast',
        2,
        '/tmp/shellparity-projects/atlas',
        ['release-review', 'health-checks', 'migration-checklist', 'api-errors'],
        'dark',
      ],
      ['shellparity-welcome-dark', 0, '/fixture', [], 'dark'],
      ['shellparity-welcome-light', 0, '/fixture', [], 'light'],
      [
        'shellparity-working',
        2,
        '/tmp/shellparity-projects/atlas',
        [
          'question-only',
          'release-review',
          'search-relevance',
          'health-checks',
          'onboarding-copy',
          'migration-checklist',
          'api-errors',
        ],
        'dark',
      ],
      ['sidebar', 2, '/tmp/harness-panel-repos/atlas', atlasSessions, null],
    ] as const) {
      process.env.HARNESS_GUI_FIXTURE = name;
      const backend = await createBackend();
      try {
        const initialized = await backend.call('app.initialize', {});
        expect(initialized.version).toBe('fixture');
        expect(initialized.defaultCwd).toBe(cwd);
        const projects = await backend.call('state.read', { name: 'projects' });
        expect(projects).toHaveLength(count);
        if (count) expect(projects).toContainEqual(expect.objectContaining({ cwd }));
        const sessions = await backend.call('sessions.list', { cwd });
        expect(sessions.map((session) => session.runId)).toEqual([...sessionIds]);
        if (theme !== null)
          expect(await backend.call('state.read', { name: 'ui' })).toMatchObject({
            appearance: { preference: theme },
          });
      } finally {
        backend.close();
      }
    }
  } finally {
    if (previous === undefined) delete process.env.HARNESS_GUI_FIXTURE;
    else process.env.HARNESS_GUI_FIXTURE = previous;
  }
});

import { expect, test } from 'bun:test';
import { FixtureBackend, type Scenario } from '../../src/backend/fixture';
import { AppStore, draftKey } from '../../src/store/store';
import { createBackendPersist } from '../../src/store/persist-backend';
import type { Method, Methods } from '../../src/backend/protocol';
import { registerBeforeSubmit } from '../../src/app/turn-hooks';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gitCommand } from '../../src/workspace/git';

async function conversation() {
  const scenario = (await Bun.file(
    new URL('../fixtures/scenarios/conversation.json', import.meta.url),
  ).json()) as Scenario;
  scenario.updates = [];
  return scenario;
}

class GatedBackend extends FixtureBackend {
  gate?: (method: Method, params: unknown) => Promise<void>;
  responseGate?: (method: Method, params: unknown) => Promise<void>;
  override async call<M extends Method>(
    method: M,
    params: Methods[M]['params'],
  ): Promise<Methods[M]['result']> {
    await this.gate?.(method, params);
    const result = await super.call(method, params);
    await this.responseGate?.(method, params);
    return result;
  }
}

test('store routes live updates by session, retains unrelated views and persists UI state', async () => {
  const scenario = await conversation();
  const backend = new FixtureBackend(scenario),
    store = new AppStore(backend);
  try {
    await store.initialize();
    await store.loadSession('chat');
    await backend.call('session.continue', { runDir: '/fixture/sessions/notes' });
    await backend.call('session.intent', {
      sessionId: 'notes',
      intent: { kind: 'setYolo', enabled: true },
    });
    await store.loadSession('notes');
    expect(store.getSnapshot().views.notes).toMatchObject({
      statusText: 'Fixture accepted setYolo',
    });
    const chat = store.getSnapshot().views.chat;
    await store.continueSession('notes');
    await store.sendIntent('notes', { kind: 'submit', text: 'Synthetic request' });
    expect(store.getSnapshot().views.chat).toBe(chat);
    expect(store.getSnapshot().views.notes).toMatchObject({
      statusText: 'Fixture accepted submit',
    });
    store.setPreferences({ locale: 'en' });
    store.setPreferences({ locale: 'fi' });
    store.setLayout({ sidebarWidth: 250 });
    await store.flushPersistence();
    expect(await backend.call('state.read', { name: 'preferences' })).toEqual({ locale: 'fi' });
    expect(await backend.call('state.read', { name: 'layout' })).toEqual({ sidebarWidth: 250 });
    const persist = createBackendPersist(backend);
    await Promise.all([
      persist.write('layout/sidebar', 240),
      persist.write('palette', { recent: ['one'] }),
    ]);
    expect(await createBackendPersist(backend).read<number>('layout/sidebar')).toBe(240);
    expect(await persist.read<{ recent: string[] }>('palette')).toEqual({ recent: ['one'] });
    await store.closeSession('notes');
    expect(store.getSnapshot().live.notes).toBeUndefined();
  } finally {
    await store.dispose();
  }
});

test('new task carries the current draft without changing the saved session draft', async () => {
  const store = new AppStore(new FixtureBackend(await conversation()));
  try {
    await store.initialize();
    await store.selectTask('chat');
    const previous = store.getSnapshot().selection;
    store.setDraft(previous, {
      text: 'Continue with the release checklist',
      worktree: { name: 'release-checklist' },
    });
    store.newTask();
    expect(store.getSnapshot().selectedTask).toBeUndefined();
    expect(store.getDraft(store.getSnapshot().selection)).toEqual(store.getDraft(previous));
    expect(store.getDraft(previous)?.text).toBe('Continue with the release checklist');
  } finally {
    await store.dispose();
  }
});

test('populated replay sessions remain selectable with their read-only catalog notice', async () => {
  const scenario = await conversation();
  const chat = scenario.sessions.find((session) => session.row.runId === 'chat')!;
  chat.row.catalog.is_resumable = false;
  chat.row.catalog.resume_disabled_reason = 'Imported history is read-only';
  const store = new AppStore(new FixtureBackend(scenario));
  try {
    await store.initialize();
    await store.selectTask('chat');
    const state = store.getSnapshot();
    expect(state.selectedTask).toBe('chat');
    expect(state.views.chat?.entries.length).toBeGreaterThan(0);
    expect(state.views.chat?.meta).toMatchObject({
      is_resumable: false,
      resume_disabled_reason: 'Imported history is read-only',
    });
    expect(state.live.chat).toBeUndefined();
  } finally {
    await store.dispose();
  }
});

test('hydration leaves the previous transcript visible and failed selection does not replace it', async () => {
  const backend = new GatedBackend(await conversation());
  const store = new AppStore(backend);
  let release!: () => void;
  const blocked = new Promise<void>((resolve) => {
    release = resolve;
  });
  let requested!: () => void;
  const started = new Promise<void>((resolve) => {
    requested = resolve;
  });
  try {
    await store.initialize();
    await store.selectTask('chat');
    backend.gate = async (method) => {
      if (method !== 'sessions.events') return;
      requested();
      await blocked;
      throw new Error('History unavailable');
    };
    const selection = store.selectTask('notes');
    const result = selection.then(
      () => undefined,
      (error: unknown) => error,
    );
    await started;
    expect(store.getSnapshot().selectedTask).toBe('chat');
    expect(store.getSnapshot().provisionalSelection).toMatchObject({ sessionId: 'notes' });
    release();
    expect(await result).toMatchObject({ message: 'History unavailable' });
    expect(store.getSnapshot().selectedTask).toBe('chat');
    expect(store.getSnapshot().provisionalSelection).toBeUndefined();
    backend.gate = undefined;
    await store.selectTask('notes');
    expect(store.getSnapshot().selectedTask).toBe('notes');
  } finally {
    release();
    backend.gate = undefined;
    await store.dispose();
  }
});

test('fork selection and continuation await the parent catalog refresh and child transcript', async () => {
  const backend = new GatedBackend(await conversation());
  const store = new AppStore(backend);
  let releaseCatalog!: () => void;
  const catalogBlocked = new Promise<void>((resolve) => {
    releaseCatalog = resolve;
  });
  let catalogRequested!: (params: unknown) => void;
  const catalogStarted = new Promise<unknown>((resolve) => {
    catalogRequested = resolve;
  });
  let releaseTranscript!: () => void;
  const transcriptBlocked = new Promise<void>((resolve) => {
    releaseTranscript = resolve;
  });
  let transcriptRequested!: () => void;
  const transcriptStarted = new Promise<void>((resolve) => {
    transcriptRequested = resolve;
  });
  let stop: (() => void) | undefined;
  try {
    await store.initialize();
    await store.selectTask('chat');
    await store.continueSession('chat');
    const parent = store.getSnapshot().selection;
    backend.responseGate = async (method, params) => {
      if (method === 'sessions.list') {
        catalogRequested(params);
        await catalogBlocked;
      } else if (method === 'sessions.events') {
        transcriptRequested();
        await transcriptBlocked;
      }
    };
    const forked = new Promise<string>((resolve) => {
      stop = backend.subscribe((notification) => {
        if (
          notification.method === 'session.update' &&
          notification.params.sessionId === 'chat' &&
          notification.params.update.kind === 'forked'
        )
          resolve(notification.params.update.runId);
      });
    });
    await store.sendIntent('chat', { kind: 'fork' });
    const childId = await forked;
    expect(store.getSnapshot().selection).toEqual(parent);
    expect(
      store.getSnapshot().sessions['/fixture/demo']?.some((row) => row.runId === childId),
    ).toBe(false);
    const opened = Promise.all([
      store.selectTask(childId),
      store.loadSession(childId),
      store.continueSession(childId),
    ]);
    void opened.catch(() => {});
    expect(await Promise.race([catalogStarted, opened])).toEqual({ cwd: '/fixture/demo' });
    expect(store.getSnapshot().selectedTask).toBe('chat');
    releaseCatalog();
    await Promise.race([transcriptStarted, opened]);
    expect(store.getSnapshot().selectedTask).toBe('chat');
    expect(store.getSnapshot().views[childId]).toBeUndefined();
    releaseTranscript();
    const [, , handle] = await opened;
    expect(store.getSnapshot().selectedTask).toBe(childId);
    expect(store.getSnapshot().views[childId]?.entries).toMatchObject([
      { kind: 'user', text: 'What does this project do?' },
      { kind: 'text', text: 'It is a small, synthetic desktop demo.' },
    ]);
    expect(handle).toMatchObject({ sessionId: childId, cwd: '/fixture/demo' });
    expect(store.getSnapshot().live[childId]).toEqual(handle);
  } finally {
    releaseCatalog();
    releaseTranscript();
    stop?.();
    backend.responseGate = undefined;
    await store.dispose();
  }
});

test('submission consumes persisted draft before create and a create failure preserves newer edits', async () => {
  const backend = new GatedBackend(await conversation());
  const store = new AppStore(backend);
  let release!: () => void;
  const blocked = new Promise<void>((resolve) => {
    release = resolve;
  });
  let requested!: () => void;
  const started = new Promise<void>((resolve) => {
    requested = resolve;
  });
  try {
    await store.initialize();
    const projectId = store.getSnapshot().projects[0]!.id;
    const target = { kind: 'project' as const, projectId };
    store.setDraft(target, { text: 'first prompt' });
    await store.flushDrafts();
    backend.gate = async (method) => {
      if (method !== 'session.create') return;
      requested();
      await blocked;
      throw new Error('Create failed');
    };
    const submission = store.submit(target, { text: 'first prompt' });
    const result = submission.then(
      () => undefined,
      (error: unknown) => error,
    );
    await started;
    expect(store.getDraft(target)).toBeUndefined();
    expect(await backend.call('state.read', { name: 'drafts' })).toEqual({});
    store.setDraft(target, { text: 'newer unsent edit' });
    release();
    expect(await result).toMatchObject({ message: 'Create failed' });
    expect(store.getDraft(target)?.text).toBe('newer unsent edit');
    backend.gate = undefined;
    await store.flush();
    expect(await backend.call('state.read', { name: 'drafts' })).toEqual({
      [`project:${projectId}`]: { text: 'newer unsent edit' },
    });
    backend.gate = async (method) => {
      if (method === 'session.create') throw new Error('Still unavailable');
    };
    await expect(store.submit(target, { text: 'newer unsent edit' })).rejects.toThrow(
      'Still unavailable',
    );
    expect(store.getDraft(target)?.text).toBe('newer unsent edit');
  } finally {
    release();
    backend.gate = undefined;
    await store.dispose();
  }
});

test('deleting a worktree selection removes its draft and parent-project pin then falls back', async () => {
  const scenario = await conversation();
  scenario.sessions = scenario.sessions.filter((session) => session.row.runId !== 'draft');
  const chat = scenario.sessions.find((session) => session.row.runId === 'chat')!;
  const notes = scenario.sessions.find((session) => session.row.runId === 'notes')!;
  notes.row.cwd = chat.row.cwd;
  const projectCwd = chat.row.cwd;
  chat.row.projectCwd = projectCwd;
  chat.row.cwd = '/fixture/worktrees/release';
  chat.row.branch = 'release';
  chat.row.worktree = true;
  const backend = new FixtureBackend(scenario);
  const store = new AppStore(backend);
  try {
    await store.initialize();
    await store.selectTask('chat');
    const target = store.getSnapshot().selection;
    expect(target.projectId).toBe(
      store.getSnapshot().projects.find((project) => project.cwd === projectCwd)?.id,
    );
    await store.renameTask('chat', 'Release worktree');
    expect(
      store.getSnapshot().sessions[projectCwd]?.find((row) => row.runId === 'chat')?.catalog
        .run_name,
    ).toBe('Release worktree');
    store.setDraft(target, { text: 'unsent' });
    await store.pinTask('chat', true);
    await store.removeTask('chat');
    expect(store.getSnapshot().selectedTask).toBe('notes');
    expect(store.getDraft(target)).toBeUndefined();
    expect(store.getSnapshot().pins[projectCwd]).not.toContain('chat');
    await store.removeTask('notes');
    expect(store.getSnapshot().selectedTask).toBeUndefined();
    expect(store.getSnapshot().selectedProject).toBe(target.projectId);
    expect(store.getSnapshot().selection.kind).toBe('project');
  } finally {
    await store.dispose();
  }
});

test('queued submit preserves the editor draft and runs hooks with the newly created session', async () => {
  const backend = new FixtureBackend(await conversation());
  const store = new AppStore(backend);
  let observed: { sessionId: string; cwd: string } | undefined;
  const unregister = registerBeforeSubmit(async (context) => {
    observed = context;
  });
  try {
    await store.initialize();
    const project = store.getSnapshot().projects[0]!;
    const target = { kind: 'project' as const, projectId: project.id };
    store.setDraft(target, { text: 'unrelated editor draft' });
    const id = await store.submit(target, { text: 'queued prompt', consumeDraft: false });
    expect(observed).toEqual({ sessionId: id, cwd: project.cwd });
    expect(store.getDraft(target)?.text).toBe('unrelated editor draft');
    expect(store.getSnapshot().selectedTask).toBe(id);
  } finally {
    unregister();
    await store.dispose();
  }
});

test('draft images persist as blob references and initialization restores preview bytes', async () => {
  const backend = new FixtureBackend(await conversation());
  const store = new AppStore(backend);
  const dataBase64 =
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aOZkAAAAASUVORK5CYII=';
  try {
    await store.initialize();
    const target = { kind: 'project' as const, projectId: store.getSnapshot().projects[0]!.id };
    store.setDraft(target, {
      text: 'image draft',
      attachments: [{ name: 'pixel.png', mime: 'image/png', dataBase64 }],
    });
    await store.flush();
    const stored = (await backend.call('state.read', { name: 'drafts' })) as Record<
      string,
      { attachments: { digest: string; dataBase64?: string }[] }
    >;
    const attachment = stored[`project:${target.projectId}`]!.attachments[0]!;
    expect(attachment.digest).toMatch(/^attachment:blake3:[0-9a-f]{64}$/);
    expect(attachment.dataBase64).toBeUndefined();
    expect(store.getDraft(target)?.attachments?.[0]?.dataBase64).toBe(dataBase64);
    await store.initialize();
    expect(store.getDraft(target)?.attachments?.[0]).toMatchObject({
      digest: attachment.digest,
      name: 'pixel.png',
      mime: 'image/png',
      dataBase64,
    });
  } finally {
    await store.dispose();
  }
});

test('old catalog responses cannot resurrect a deleted session', async () => {
  const backend = new GatedBackend(await conversation());
  const store = new AppStore(backend);
  const blocked = Promise.withResolvers<void>();
  const started = Promise.withResolvers<void>();
  try {
    await store.initialize();
    const cwd = store.getSnapshot().projects[0]!.cwd;
    let first = true;
    backend.responseGate = async (method) => {
      if (method !== 'sessions.list' || !first) return;
      first = false;
      started.resolve();
      await blocked.promise;
    };
    const old = store.loadSessions(cwd);
    await started.promise;
    await store.removeTask('notes');
    blocked.resolve();
    await old;
    expect(store.getSnapshot().sessions[cwd]?.map((row) => row.runId)).not.toContain('notes');
    expect(store.getSnapshot().views.notes).toBeUndefined();
  } finally {
    blocked.resolve();
    await store.dispose();
  }
});

test('delayed creation finishes in the background after newer navigation', async () => {
  const backend = new GatedBackend(await conversation());
  const store = new AppStore(backend);
  const blocked = Promise.withResolvers<void>();
  const started = Promise.withResolvers<void>();
  try {
    await store.initialize();
    backend.responseGate = async (method) => {
      if (method !== 'session.create') return;
      started.resolve();
      await blocked.promise;
    };
    const creating = store.createSession({ cwd: store.getSnapshot().projects[0]!.cwd });
    await started.promise;
    await store.selectTask('notes');
    blocked.resolve();
    const handle = await creating;
    expect(store.getSnapshot().selectedTask).toBe('notes');
    expect(store.getSnapshot().live[handle.sessionId]).toEqual(handle);
  } finally {
    blocked.resolve();
    await store.dispose();
  }
});

test('asynchronous rejection restores durable input without overwriting newer edits', async () => {
  const backend = new GatedBackend(await conversation());
  const listeners = new Set<Parameters<typeof backend.subscribe>[0]>();
  const subscribe = backend.subscribe;
  backend.subscribe = (listener) => {
    listeners.add(listener);
    return subscribe(listener);
  };
  const store = new AppStore(backend);
  try {
    await store.initialize();
    await store.selectTask('notes');
    const target = store.getSnapshot().selection;
    for (const newer of [false, true]) {
      store.setDraft(target, { text: 'refused input' });
      await store.submit(target, { text: 'refused input' });
      if (newer) store.setDraft(target, { text: 'newer edit' });
      for (const listener of listeners)
        listener({
          method: 'session.update',
          params: {
            sessionId: 'notes',
            update: { kind: 'restoreQueued', texts: ['refused input'] },
          },
        });
      await store.flush();
      const drafts = await backend.call('state.read', { name: 'drafts' });
      expect(drafts).toMatchObject({
        'session:notes': { text: newer ? 'newer edit' : 'refused input' },
      });
      if (newer)
        expect(await backend.call('state.read', { name: 'preferences' })).toMatchObject({
          composerQueues: { [JSON.stringify(target)]: [{ draft: { text: 'refused input' } }] },
        });
    }
  } finally {
    await store.dispose();
  }
});

test('shutdown waits for an in-flight attachment save and the newest debounced draft', async () => {
  const backend = new GatedBackend(await conversation());
  const store = new AppStore(backend);
  const blocked = Promise.withResolvers<void>();
  const started = Promise.withResolvers<void>();
  await store.initialize();
  const target = store.getSnapshot().selection;
  let persisted: unknown;
  backend.responseGate = async (method, params) => {
    if (method === 'state.write' && (params as Methods['state.write']['params']).name === 'drafts')
      persisted = await backend.call('state.read', { name: 'drafts' });
  };
  backend.gate = async (method) => {
    if (method !== 'blobs.put') return;
    started.resolve();
    await blocked.promise;
  };
  store.setDraft(target, {
    text: 'less than 250ms',
    attachments: [{ mime: 'image/png', dataBase64: 'aGVsbG8=' }],
  });
  const shutdown = store.dispose();
  await started.promise;
  expect(backend.getStatus().state).toBe('connected');
  store.setDraft(target, { text: 'newest draft' });
  blocked.resolve();
  await shutdown;
  expect(backend.getStatus().state).toBe('closed');
  expect(persisted).toMatchObject({
    [draftKey(target)]: { text: 'newest draft' },
  });
});

test('offscreen submissions capture checkpoints while steering leaves the next baseline untouched', async () => {
  const cwd = await mkdtemp(join(tmpdir(), 'harness-offscreen-'));
  const scenario = await conversation();
  scenario.projects[0]!.cwd = cwd;
  for (const session of scenario.sessions) session.row.cwd = cwd;
  const store = new AppStore(new FixtureBackend(scenario));
  try {
    await gitCommand(cwd, ['init', '--quiet']);
    await writeFile(join(cwd, 'note.txt'), 'before steering\n');
    await store.initialize();
    await store.loadSession('notes');
    await store.selectTask('chat');
    const target = {
      kind: 'session' as const,
      sessionId: 'notes',
      projectId: scenario.projects[0]!.id,
    };
    await store.submit(target, { text: 'steer', mode: 'steer', consumeDraft: false });
    expect(
      (await gitCommand(cwd, ['for-each-ref', '--format=%(refname)', 'refs/harness-gui/notes/']))
        .stdout,
    ).toBe('');
    await writeFile(join(cwd, 'note.txt'), 'after steering\n');
    await store.submit(target, { text: 'queued', mode: 'submit', consumeDraft: false });
    expect(store.getSnapshot().selectedTask).toBe('chat');
    expect(
      (await gitCommand(cwd, ['show', 'refs/harness-gui/notes/1-start:note.txt'])).stdout,
    ).toBe('after steering\n');
    expect(await store.backend.call('state.read', { name: 'turn-checkpoints' })).toMatchObject({
      notes: { pending: [{ checkpoint: { turn: 1 } }] },
    });
  } finally {
    await store.dispose();
    await rm(cwd, { recursive: true, force: true });
  }
});

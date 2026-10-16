import { expect, test } from 'bun:test';
import { FixtureBackend, type Scenario } from '../../src/backend/fixture';
import { AppStore } from '../../src/store/store';
import { createBackendPersist } from '../../src/store/persist-backend';
import type { Method, Methods } from '../../src/backend/protocol';
import { registerBeforeSubmit } from '../../src/app/turn-hooks';

async function conversation() {
  const scenario = (await Bun.file(
    new URL('../fixtures/scenarios/conversation.json', import.meta.url),
  ).json()) as Scenario;
  scenario.updates = [];
  return scenario;
}

class GatedBackend extends FixtureBackend {
  gate?: (method: Method, params: unknown) => Promise<void>;
  override async call<M extends Method>(
    method: M,
    params: Methods[M]['params'],
  ): Promise<Methods[M]['result']> {
    await this.gate?.(method, params);
    return super.call(method, params);
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

test('deleting selection removes draft and pin then falls back within the same project', async () => {
  const scenario = await conversation();
  scenario.sessions = scenario.sessions.filter((session) => session.row.runId !== 'draft');
  const chat = scenario.sessions.find((session) => session.row.runId === 'chat')!;
  const notes = scenario.sessions.find((session) => session.row.runId === 'notes')!;
  notes.row.cwd = chat.row.cwd;
  const backend = new FixtureBackend(scenario);
  const store = new AppStore(backend);
  try {
    await store.initialize();
    await store.selectTask('chat');
    const target = store.getSnapshot().selection;
    store.setDraft(target, { text: 'unsent' });
    await store.pinTask('chat', true);
    await store.removeTask('chat');
    expect(store.getSnapshot().selectedTask).toBe('notes');
    expect(store.getDraft(target)).toBeUndefined();
    expect(store.getSnapshot().pins[chat.row.cwd]).not.toContain('chat');
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

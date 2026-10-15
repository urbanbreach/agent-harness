import { expect, test } from 'bun:test';
import { FixtureBackend, type Scenario } from '../../src/backend/fixture';
import { AppStore } from '../../src/store/store';
import { createBackendPersist } from '../../src/store/persist-backend';

test('store routes live updates by session, retains unrelated views and persists UI state', async () => {
  const scenario = (await Bun.file(
    new URL('../fixtures/scenarios/conversation.json', import.meta.url),
  ).json()) as Scenario;
  scenario.updates = [];
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

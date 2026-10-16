import { expect, test } from 'bun:test';
import { FixtureBackend, type Scenario } from '../../src/backend/fixture';
import { AppStore, setAppStore, type Draft } from '../../src/store';
import { restoreStash } from '../../src/app/palette/stash';

test.each([0, 1])(
  'restoring stash %i preserves the destination and attachments after persistence',
  async (index) => {
    const scenario = (await Bun.file(
      new URL('../fixtures/scenarios/conversation.json', import.meta.url),
    ).json()) as Scenario;
    scenario.updates = [];
    const backend = new FixtureBackend(scenario);
    const store = new AppStore(backend);
    setAppStore(store);
    await store.initialize();
    const target = store.getSnapshot().selection;
    const a: Draft = { text: 'Draft A' };
    const b: Draft = {
      text: 'Draft B',
      attachments: [{ name: 'note.txt', mime: 'text/plain', dataBase64: 'bm90ZQ==' }],
    };
    store.setPreferences({ stashedDrafts: (index ? [{ text: 'Other' }, a] : [a]) as never });
    store.setDraft(target, b);
    restoreStash(index);
    expect(store.getDraft(target)).toEqual(a);
    await store.flushPersistence();
    const preferences = await backend.call('state.read', { name: 'preferences' });
    await store.dispose();
    const reopened = new AppStore(
      new FixtureBackend({
        ...scenario,
        state: { ...scenario.state, preferences: preferences! },
      }),
    );
    try {
      await reopened.initialize();
      setAppStore(reopened);
      expect((reopened.getSnapshot().preferences.stashedDrafts as unknown as Draft[])[0]).toEqual(
        b,
      );
      restoreStash(0);
      expect(reopened.getDraft(reopened.getSnapshot().selection)).toEqual(b);
    } finally {
      await reopened.dispose();
    }
  },
);

import { expect, test } from 'bun:test';
import { FixtureBackend, type Scenario } from '../../src/backend/fixture';
import type { Intent, Method, Methods } from '../../src/backend/protocol';
import { AppStore, type Selection } from '../../src/store/store';
import {
  queuedDrafts,
  setQueuedDrafts,
  startQueueDelivery,
} from '../../src/app/composer/queue-store';

test('delivers offscreen queued input without consuming a newer draft or racing admission', async () => {
  const scenario = (await Bun.file(
    new URL('../fixtures/scenarios/conversation.json', import.meta.url),
  ).json()) as Scenario;
  scenario.updates = [];
  const sent: Intent[] = [];
  class RecordingBackend extends FixtureBackend {
    override async call<M extends Method>(
      method: M,
      params: Methods[M]['params'],
    ): Promise<Methods[M]['result']> {
      if (method === 'session.intent')
        sent.push((params as Methods['session.intent']['params']).intent);
      return super.call(method, params);
    }
  }
  const store = new AppStore(new RecordingBackend(scenario));
  try {
    await store.initialize();
    await store.loadSession('notes');
    const target: Selection = { kind: 'session', sessionId: 'notes', projectId: 'demo' };
    const key = JSON.stringify(target);
    store.setDraft(target, { text: 'Keep my unsent draft' });
    setQueuedDrafts(store, key, [
      { id: 'first', draft: { text: 'First @notes.txt' } },
      { id: 'second', draft: { text: 'Second' } },
    ]);
    const delivered = new Promise<void>((resolve) => {
      const unsubscribe = store.subscribe(() => {
        if (queuedDrafts(store)[key]?.length === 1) {
          unsubscribe();
          resolve();
        }
      });
    });
    startQueueDelivery(store);
    await delivered;
    expect(sent.flatMap((intent) => (intent.kind === 'submit' ? [intent.text] : []))).toEqual([
      'First @notes.txt',
    ]);
    expect(queuedDrafts(store)[key]?.[0]?.id).toBe('second');
    expect(sent.find((intent) => intent.kind === 'submit')).toMatchObject({
      fileTags: [{ path: '/fixture/demo/notes.txt' }],
    });
    expect(store.getDraft(target)?.text).toBe('Keep my unsent draft');
  } finally {
    await store.dispose();
  }
});

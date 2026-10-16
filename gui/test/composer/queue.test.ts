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
  scenario.updates = [
    {
      sessionId: 'notes',
      delayMs: 10,
      update: {
        kind: 'event',
        delivery: 'durable',
        event: {
          schema_version: 1,
          event_id: 'notes-start',
          seq: 1,
          run_id: 'notes',
          mono_ms: 0,
          actor: { kind: 'supervisor' },
          payload: {
            event_type: 'task_scheduled',
            data: { task_id: 'current-turn', state: 'started', queue_key: null, metadata: null },
          },
        },
      },
    },
    {
      sessionId: 'notes',
      delayMs: 20,
      update: {
        kind: 'event',
        delivery: 'durable',
        event: {
          schema_version: 1,
          event_id: 'notes-end',
          seq: 2,
          run_id: 'notes',
          mono_ms: 1,
          actor: { kind: 'supervisor' },
          payload: {
            event_type: 'task_completed',
            data: {
              task_id: 'current-turn',
              result_summary: 'Finished',
              result_digest: 'finished',
              metadata: null,
            },
          },
        },
      },
    },
  ];
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
      {
        id: 'first',
        draft: {
          text: 'First @notes.txt @agent:reviewer @resource:release',
          agentTags: [{ name: 'reviewer', source: { start: 0, end: 0, value: '@agent:reviewer' } }],
          resourceTags: [
            {
              name: 'release',
              uri: 'release://current',
              mime: 'text/plain',
              source: { start: 0, end: 0, value: '@resource:release' },
            },
            {
              name: 'removed',
              uri: 'release://removed',
              mime: 'text/plain',
              source: { start: 0, end: 0, value: '@resource:removed' },
            },
          ],
        },
      },
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
    expect(sent).toHaveLength(0);
    expect(queuedDrafts(store)[key]).toHaveLength(2);
    await store.continueSession('notes');
    await delivered;
    expect(sent.flatMap((intent) => (intent.kind === 'submit' ? [intent.text] : []))).toEqual([
      'First @notes.txt @agent:reviewer @resource:release',
    ]);
    expect(queuedDrafts(store)[key]?.[0]?.id).toBe('second');
    expect(sent.find((intent) => intent.kind === 'submit')).toMatchObject({
      fileTags: [{ path: '/fixture/demo/notes.txt' }],
    });
    expect(store.getDraft(target)?.text).toBe('Keep my unsent draft');
    const submission = sent.find((intent) => intent.kind === 'submit');
    expect(submission?.kind === 'submit' && submission.agentTags).toEqual([
      { name: 'reviewer', source: { start: 17, end: 32, value: '@agent:reviewer' } },
    ]);
    expect(submission?.kind === 'submit' && submission.resourceTags).toEqual([
      {
        name: 'release',
        uri: 'release://current',
        mime: 'text/plain',
        source: { start: 33, end: 50, value: '@resource:release' },
      },
    ]);
    expect(submission?.kind === 'submit' && submission.fileTags).toHaveLength(1);
  } finally {
    await store.dispose();
  }
});

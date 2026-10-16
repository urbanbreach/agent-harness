import { expect, test } from 'bun:test';
import { emptySessionView, type TranscriptEntry } from '../../src/model/session';
import { transcriptRows } from '../../src/app/transcript/rows';

test('settled work folds only interim content and keeps the complete trailing answer for copy', () => {
  const view = emptySessionView();
  view.turns = [
    {
      id: 'turn',
      requestIds: ['provider'],
      userIds: ['prompt'],
      status: 'completed',
      startedMs: 0,
      endedMs: 8000,
    },
  ];
  const base = { turnId: 'turn', startedMs: 0, requestId: 'provider' };
  view.entries = [
    {
      ...base,
      id: 'prompt',
      kind: 'user',
      text: 'Review',
      attachments: [],
      fileTags: [],
      agentTags: [],
      resourceTags: [],
      steering: false,
    },
    { ...base, id: 'interim', kind: 'text', text: 'I will inspect it.', streaming: false },
    { ...base, id: 'tool', kind: 'tool', toolCallId: 'tool' },
    { ...base, id: 'answer-a', kind: 'text', text: 'First part.', streaming: false },
    { ...base, id: 'answer-b', kind: 'text', text: 'Second part.', streaming: false },
  ] satisfies TranscriptEntry[];
  const collapsed = transcriptRows(view, new Set());
  expect(collapsed.map((row) => row.id)).toEqual([
    'prompt',
    'fold:turn',
    'answer-a',
    'answer-b',
    'footer:turn',
  ]);
  expect(collapsed.at(-1)).toMatchObject({ kind: 'footer', text: 'First part.\n\nSecond part.' });
  const expanded = transcriptRows(view, new Set(['turn']));
  expect(expanded.map((row) => row.id)).toEqual([
    'prompt',
    'fold:turn',
    'interim',
    'activity:tool',
    'answer-a',
    'answer-b',
    'footer:turn',
  ]);
  view.turns[0]!.status = 'working';
  view.status = 'working';
  expect(transcriptRows(view, new Set()).map((row) => row.id)).toEqual([
    'prompt',
    'interim',
    'activity:tool',
    'answer-a',
    'answer-b',
    'working',
  ]);
});

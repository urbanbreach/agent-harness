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
  view.entries = [
    view.entries[0]!,
    { ...base, id: 'thought', kind: 'reasoning', text: 'Private thought', streaming: false },
    view.entries[3]!,
  ];
  expect(transcriptRows(view, new Set(), false).map((row) => row.id)).toEqual([
    'prompt',
    'answer-a',
    'working',
  ]);
  view.turns[0]!.status = 'completed';
  view.status = 'idle';
  expect(transcriptRows(view, new Set(), false).map((row) => row.id)).toEqual([
    'prompt',
    'answer-a',
    'footer:turn',
  ]);
});

test('interleaved turns retain first-seen ordering without losing loose entries or reasoning visibility', () => {
  const view = emptySessionView();
  view.turns = ['second', 'first'].map((id) => ({
    id,
    requestIds: [id],
    userIds: [],
    status: 'completed' as const,
    startedMs: 0,
  }));
  view.entries = [
    {
      id: 'a',
      turnId: 'first',
      kind: 'text',
      text: 'A',
      streaming: false,
      startedMs: 0,
      requestId: 'first',
    },
    { id: 'loose', kind: 'notice', message: 'Notice', level: 'info', startedMs: 0 },
    {
      id: 'b',
      turnId: 'second',
      kind: 'text',
      text: 'B',
      streaming: false,
      startedMs: 0,
      requestId: 'second',
    },
    {
      id: 'thought',
      turnId: 'first',
      kind: 'reasoning',
      text: 'Thinking',
      streaming: false,
      startedMs: 0,
      requestId: 'first',
    },
    {
      id: 'c',
      turnId: 'first',
      kind: 'text',
      text: 'C',
      streaming: false,
      startedMs: 0,
      requestId: 'first',
    },
  ];
  expect(transcriptRows(view, new Set(['first']), false).map((row) => row.id)).toEqual([
    'a',
    'c',
    'footer:first',
    'activity:loose',
    'b',
    'footer:second',
  ]);
  expect(transcriptRows(view, new Set(['first'])).map((row) => row.id)).toEqual([
    'fold:first',
    'a',
    'activity:thought',
    'c',
    'footer:first',
    'activity:loose',
    'b',
    'footer:second',
  ]);
});

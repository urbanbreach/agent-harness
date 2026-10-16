import { expect, test } from 'bun:test';
import { emptySessionView, type UserEntry } from '../../src/model/session';
import { taskTitle } from '../../src/store/title';

test('native placeholder titles yield a bounded first-prompt label without replacing authored titles', () => {
  const view = emptySessionView();
  view.title = 'New session - 2026-10-16T00:00:00.000Z';
  const prompt: UserEntry = {
    kind: 'user',
    id: 'prompt',
    requestId: 'request',
    startedMs: 0,
    text: '  Improve search relevance with a careful measurable rollout today  ',
    attachments: [],
    steering: false,
  };
  view.entries = [prompt];
  expect(taskTitle(undefined, view)).toBe('Improve search relevance with a careful measurable');
  prompt.text = 'A'.repeat(80);
  expect(taskTitle(undefined, view)).toBe('A'.repeat(54));
  // A later prompt must not drop the task back to the untitled label.
  view.entries = [prompt, { ...prompt, id: 'later', text: 'Follow up' }];
  expect(taskTitle(undefined, view)).toBe('A'.repeat(54));
  view.title = '  Search rollout review  ';
  expect(taskTitle(undefined, view)).toBe('Search rollout review');
});

import { describe, expect, test } from 'bun:test';
import { fileTag, fileTags, suggestions, triggerAt } from '../../src/app/composer/completion';
import { prepareDraft } from '../../src/app/composer/queue-store';

describe('composer completion', () => {
  test('completes only line commands and whitespace-delimited file mentions, replacing whole token', () => {
    expect(triggerAt('Discuss /compact', 16)).toBeUndefined();
    expect(triggerAt('mail a@example.com', 18)).toBeUndefined();
    expect(triggerAt('notes\n/compact', 10)).toEqual({
      kind: '/',
      start: 6,
      end: 14,
      query: 'com',
    });
    expect(triggerAt('Read @src/main.ts now', 10)).toEqual({
      kind: '@',
      start: 5,
      end: 17,
      query: 'src/',
    });
  });
  test('prefers project command overrides, ranks fuzzy paths and keeps insertion formatting', () => {
    const commands = suggestions(
      { kind: '/', start: 0, end: 3, query: 'rev' },
      [
        { name: 'review', description: 'global', scope: 'user' },
        { name: 'review', description: 'project rules', scope: 'project' },
      ],
      [],
    );
    expect(commands[0]).toMatchObject({
      value: '/review',
      description: 'project rules',
      scope: 'project',
    });
    expect(commands.filter((row) => row.value === '/review')).toHaveLength(1);
    const files = suggestions(
      { kind: '@', start: 0, end: 3, query: 'main' },
      [],
      [
        { path: 'domain/readme.md', dir: false },
        { path: 'src/main.ts', dir: false },
        { path: 'main', dir: true },
      ],
    );
    expect(files[0]?.value).toBe('@main/');
    expect(files[1]?.value).toBe('@src/main.ts');
  });
  test('turns file mentions into absolute Harness file tags with source spans', () => {
    const tags = fileTags('Read @src/main.ts and @docs/', '/project');
    expect(tags).toEqual([
      {
        path: '/project/src/main.ts',
        filename: 'main.ts',
        url: 'file:///project/src/main.ts',
        mime: 'application/octet-stream',
        source: { start: 5, end: 17, value: '@src/main.ts' },
      },
      {
        path: '/project/docs',
        filename: 'docs',
        url: 'file:///project/docs',
        mime: 'application/x-directory',
        source: { start: 22, end: 28, value: '@docs/' },
      },
    ]);
  });
  test('submits selected paths whole and all tag spans in Unicode scalar offsets', () => {
    const text = '  😀 @docs/release notes.md @agent:review @resource:docs  ';
    const prepared = prepareDraft(
      {
        text,
        fileTags: [fileTag('docs/release notes.md', '/project', 5)],
        agentTags: [{ name: 'review', source: { start: 0, end: 0, value: '@agent:review' } }],
        resourceTags: [
          {
            name: 'docs',
            uri: 'docs://index',
            mime: 'text/plain',
            source: { start: 0, end: 0, value: '@resource:docs' },
          },
        ],
        attachments: [{ path: '/project/report 😀.txt', mime: 'text/plain', dataBase64: '' }],
      },
      '/project',
    );
    expect(prepared.fileTags?.map((tag) => tag.path)).toEqual([
      '/project/docs/release notes.md',
      '/project/report 😀.txt',
    ]);
    expect(prepared.fileTags?.[0]?.source.start).toBe(2);
    for (const tag of [...prepared.fileTags!, ...prepared.agentTags!, ...prepared.resourceTags!])
      expect([...prepared.text].slice(tag.source.start, tag.source.end).join('')).toBe(
        tag.source.value,
      );
    expect(fileTags('😀 @README.md', '/project')[0]?.source.start).toBe(3);
  });
});

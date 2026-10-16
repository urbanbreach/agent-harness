import { afterEach, expect, test } from 'bun:test';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { collectReviewDiff, expandReviewContext, gitCommand } from '../../src/workspace/git';
import { foldContext, parseReviewPatch, reviewTree } from '../../src/app/panel/review-model';

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function repository(): Promise<string> {
  const cwd = await mkdtemp(join(tmpdir(), 'harness-review-'));
  roots.push(cwd);
  await gitCommand(cwd, ['init', '-b', 'main']);
  await gitCommand(cwd, ['config', 'user.name', 'Review Test']);
  await gitCommand(cwd, ['config', 'user.email', 'review@example.invalid']);
  await gitCommand(cwd, ['config', 'commit.gpgsign', 'false']);
  return cwd;
}

// Protect header-based status, line numbering, and snapshot-backed context expansion together.
test('review preserves immutable content, A/M/D/B status and line numbers through folded context expansion', async () => {
  const cwd = await repository();
  const original = Array.from({ length: 240 }, (_, index) => `line ${index + 1}`);
  await Bun.write(join(cwd, 'src/main.ts'), `${original.join('\n')}\n`);
  await Bun.write(join(cwd, 'removed.txt'), 'old\n');
  await Bun.write(join(cwd, 'binary.bin'), new Uint8Array([0, 1]));
  await gitCommand(cwd, ['add', '.']);
  await gitCommand(cwd, ['commit', '-m', 'Initial']);
  const changed = [...original];
  changed[120] = 'const changed = true;';
  await Bun.write(join(cwd, 'src/main.ts'), `${changed.join('\n')}\n`);
  await Bun.write(join(cwd, 'new.txt'), 'new\n');
  await rm(join(cwd, 'removed.txt'));
  await Bun.write(join(cwd, 'binary.bin'), new Uint8Array([0, 2]));
  const captured = await collectReviewDiff(cwd, 'uncommitted');
  const partial = parseReviewPatch(captured).find((file) => file.path === 'src/main.ts')!;
  expect(
    partial.lines.some(
      (line) => line.kind === 'gap' && line.lines.length === 0 && line.text === '117',
    ),
  ).toBe(true);
  await Bun.write(join(cwd, 'src/main.ts'), 'newer external content\n');
  const snapshot = await expandReviewContext(captured, 2_147_483_647);
  const files = parseReviewPatch(snapshot);
  expect(Object.fromEntries(files.map((file) => [file.path, file.status]))).toEqual({
    'binary.bin': 'B',
    'new.txt': 'A',
    'removed.txt': 'D',
    'src/main.ts': 'M',
  });
  const source = files.find((file) => file.path === 'src/main.ts')!;
  expect(source.lines.find((line) => line.kind === 'added')).toEqual({
    kind: 'added',
    text: 'const changed = true;',
    next: 121,
  });
  expect(source.lines.find((line) => line.kind === 'removed')).toEqual({
    kind: 'removed',
    text: 'line 121',
    old: 121,
  });
  expect(source.lines.some((line) => line.text.includes('newer external'))).toBe(false);
  const folded = foldContext(source.lines);
  const gaps = folded.filter((line) => line.kind === 'gap');
  expect(gaps.map((gap) => gap.lines.length)).toEqual([117, 116]);
  expect(folded.filter((line) => line.kind === 'context').length).toBe(6);
  const leading = gaps[0]!;
  const expanded = foldContext(source.lines, { [leading.id]: { above: 0, below: 100 } });
  expect(expanded.filter((line) => line.kind === 'context').length).toBe(106);
  const all = foldContext(
    source.lines,
    Object.fromEntries(
      gaps.map((gap) => [gap.id, { above: gap.lines.length, below: gap.lines.length }]),
    ),
  );
  expect(all).toEqual(source.lines);
});

test('tree filtering expands matching ancestors without changing stored collapse state or diff files', () => {
  const files = [
    {
      path: 'src/Nested/Main.ts',
      additions: 2,
      deletions: 1,
      binary: false,
      status: 'M' as const,
      lines: [],
    },
    {
      path: 'src/other.ts',
      additions: 1,
      deletions: 0,
      binary: false,
      status: 'A' as const,
      lines: [],
    },
    {
      path: 'README.md',
      additions: 0,
      deletions: 1,
      binary: false,
      status: 'D' as const,
      lines: [],
    },
  ];
  const collapsed = new Set(['src']);
  expect(reviewTree(files, '', collapsed).map((row) => row.path)).toEqual(['src', 'README.md']);
  expect(reviewTree(files, '  NESTED/main  ', collapsed).map((row) => row.path)).toEqual([
    'src',
    'src/Nested',
    'src/Nested/Main.ts',
  ]);
  expect(reviewTree(files, '', collapsed).map((row) => row.path)).toEqual(['src', 'README.md']);
  expect(files.length).toBe(3);
  expect(reviewTree(files, 'does-not-exist', collapsed)).toEqual([]);
});

test('context hydration preserves late changes when an unchanged prefix exceeds the line cap', async () => {
  const cwd = await repository();
  await Bun.write(join(cwd, 'a-large.txt'), `${'unchanged\n'.repeat(50_100)}before\n`);
  await Bun.write(join(cwd, 'z-last.txt'), 'before\n');
  await gitCommand(cwd, ['add', '.']);
  await gitCommand(cwd, ['commit', '-m', 'Initial']);
  await Bun.write(join(cwd, 'a-large.txt'), `${'unchanged\n'.repeat(50_100)}after\n`);
  await Bun.write(join(cwd, 'z-last.txt'), 'after\n');
  const captured = await collectReviewDiff(cwd, 'uncommitted');
  await Bun.write(join(cwd, 'z-last.txt'), 'later external edit\n');
  const expanded = await expandReviewContext(captured, 2_147_483_647);
  expect(expanded.completeContext).toBe(false);
  expect(expanded.truncated).toBe(false);
  const files = parseReviewPatch(expanded);
  expect(files.map((file) => file.path)).toEqual(['a-large.txt', 'z-last.txt']);
  expect(files.map((file) => file.lines.find((line) => line.kind === 'added'))).toEqual([
    { kind: 'added', text: 'after', next: 50_101 },
    { kind: 'added', text: 'after', next: 1 },
  ]);
});

test('file and directory replacements retain independently navigable tree rows', async () => {
  const cwd = await repository();
  await Bun.write(join(cwd, 'src'), 'old file\n');
  await gitCommand(cwd, ['add', '.']);
  await gitCommand(cwd, ['commit', '-m', 'Initial']);
  await rm(join(cwd, 'src'));
  await Bun.write(join(cwd, 'src/main.ts'), 'new child\n');
  const files = parseReviewPatch(await collectReviewDiff(cwd, 'uncommitted'));
  for (const ordered of [files, [...files].reverse()]) {
    const rows = reviewTree(ordered, '', new Set());
    expect(rows.map((row) => [row.path, row.directory, row.file?.status])).toEqual([
      ['src', true, undefined],
      ['src/main.ts', false, 'A'],
      ['src', false, 'D'],
    ]);
    expect(new Set(rows.map((row) => row.id)).size).toBe(3);
    expect(
      reviewTree(ordered, '', new Set(['src'])).map((row) => [row.path, row.directory]),
    ).toEqual([
      ['src', true],
      ['src', false],
    ]);
  }
});

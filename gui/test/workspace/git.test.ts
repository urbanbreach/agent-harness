import { afterEach, expect, test } from 'bun:test';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  collectReviewDiff,
  commit,
  commitState,
  expandReviewContext,
  gitCommand,
  inspectBranches,
  push,
  switchBranch,
  type ReviewSource,
} from '../../src/workspace/git';

const directories: string[] = [];
afterEach(async () => {
  await Promise.all(
    directories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })),
  );
});
async function repository(initialCommit = true): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), 'harness-git-test-'));
  directories.push(root);
  await gitCommand(root, ['init', '-b', 'main']);
  await gitCommand(root, ['config', 'user.name', 'Workspace Test']);
  await gitCommand(root, ['config', 'user.email', 'workspace@example.invalid']);
  await gitCommand(root, ['config', 'commit.gpgsign', 'false']);
  if (initialCommit) {
    await Bun.write(join(root, 'base.txt'), 'base\n');
    await gitCommand(root, ['add', '.']);
    await gitCommand(root, ['commit', '-m', 'Initial']);
  }
  return root;
}

test('review sources use distinct stable trees, include untracked files, and preserve HEAD and index', async () => {
  const root = await repository();
  await switchBranch(root, 'feature', true);
  await Bun.write(join(root, 'committed.txt'), 'committed\n');
  await commit(root, { message: 'Feature', stageAll: true });
  await gitCommand(root, ['update-ref', 'refs/harness-gui/session/1-start', 'HEAD~1']);
  await gitCommand(root, ['update-ref', 'refs/harness-gui/session/1-end', 'HEAD']);
  await Bun.write(join(root, 'staged.txt'), 'staged\n');
  await gitCommand(root, ['add', 'staged.txt']);
  await Bun.write(join(root, 'base.txt'), 'manual\n');
  await Bun.write(join(root, 'untracked.txt'), 'untracked\n');
  const beforeIndex = await readFile(join(root, '.git/index'));
  const beforeHead = (await gitCommand(root, ['rev-parse', 'HEAD'])).stdout;
  const cases: { source: ReviewSource; paths: string[] }[] = [
    { source: 'uncommitted', paths: ['base.txt', 'staged.txt', 'untracked.txt'] },
    { source: 'unstaged', paths: ['base.txt', 'untracked.txt'] },
    { source: 'staged', paths: ['staged.txt'] },
    { source: 'committed', paths: ['committed.txt'] },
    { source: 'branch', paths: ['base.txt', 'committed.txt', 'staged.txt', 'untracked.txt'] },
    { source: { kind: 'lastTurn', session: 'session', turn: 1 }, paths: ['committed.txt'] },
  ];
  for (const { source, paths } of cases) {
    const snapshot = await collectReviewDiff(root, source);
    expect(snapshot.files.map((file) => file.path)).toEqual(paths);
    expect(snapshot.patch).toContain('diff --git');
  }
  const stable = await collectReviewDiff(root, 'uncommitted');
  await Bun.write(join(root, 'base.txt'), 'later edits\n');
  const expanded = await expandReviewContext(stable, 2_147_483_647);
  expect(expanded.patch).toContain('+manual');
  expect(expanded.patch).not.toContain('later edits');
  expect(expanded.completeContext).toBe(true);
  expect(await readFile(join(root, '.git/index'))).toEqual(beforeIndex);
  expect((await gitCommand(root, ['rev-parse', 'HEAD'])).stdout).toBe(beforeHead);
});

test('unborn repositories use an empty baseline and missing turn snapshots have exact errors', async () => {
  const root = await repository(false);
  await Bun.write(join(root, 'new.txt'), 'new\n');
  expect((await collectReviewDiff(root, 'uncommitted')).additions).toBe(1);
  expect((await collectReviewDiff(root, 'committed')).files).toEqual([]);
  expect((await collectReviewDiff(root, 'branch')).files.map((file) => file.path)).toEqual([
    'new.txt',
  ]);
  await expect(
    collectReviewDiff(root, { kind: 'lastTurn', session: 's', turn: 0 }),
  ).rejects.toThrow('the first checkpoint is a baseline, not a completed turn');
  await expect(
    collectReviewDiff(root, { kind: 'lastTurn', session: 's', turn: 1 }),
  ).rejects.toThrow("the turn's starting checkpoint is unavailable");
  await commit(root, { message: 'Initial', stageAll: true });
  await gitCommand(root, ['update-ref', 'refs/harness-gui/s/1-start', 'HEAD']);
  await expect(
    collectReviewDiff(root, { kind: 'lastTurn', session: 's', turn: 1 }),
  ).rejects.toThrow("the turn's ending checkpoint is unavailable");
});

test('review caps patch lines without dropping full summary and supports cancellation', async () => {
  const root = await repository();
  await Bun.write(join(root, 'large.txt'), 'new\n'.repeat(50_100));
  await Bun.write(join(root, 'z-last.txt'), 'last\n');
  const snapshot = await collectReviewDiff(root, 'uncommitted');
  expect(snapshot.truncated).toBe(true);
  expect(snapshot.patch.split('\n').length - 1).toBe(50_000);
  expect(snapshot.additions).toBe(50_101);
  expect(snapshot.files.map((file) => file.path)).toEqual(['large.txt', 'z-last.txt']);
  const controller = new AbortController();
  controller.abort();
  await expect(collectReviewDiff(root, 'branch', controller.signal)).rejects.toThrow();
});

test('branch switching creates real branches, reports detached and worktree occupancy, and surfaces conflicts', async () => {
  const root = await repository();
  const feature = await switchBranch(root, 'feature', true);
  expect(feature.current).toBe('feature');
  expect(feature.defaultBranch).toBe('main');
  await Bun.write(join(root, 'base.txt'), 'feature\n');
  await commit(root, { message: 'Feature', stageAll: true });
  await switchBranch(root, 'main');
  await Bun.write(join(root, 'base.txt'), 'conflict\n');
  await expect(switchBranch(root, 'feature')).rejects.toThrow();
  await expect(switchBranch(root, '')).rejects.toThrow('enter a branch name');
  await expect(switchBranch(root, 'bad..branch', true)).rejects.toThrow();
  const other = join(root, 'other-worktree');
  await gitCommand(root, ['worktree', 'add', other, 'feature']);
  expect(
    (await inspectBranches(root))?.branches.find((branch) => branch.name === 'feature')
      ?.checkedOutElsewhere,
  ).toBe(true);
  await expect(switchBranch(root, 'feature')).rejects.toThrow();
  await gitCommand(root, ['checkout', '--detach', 'HEAD']);
  expect((await inspectBranches(root))?.detached).toBe(true);
  await expect(push(root)).rejects.toThrow('cannot push a detached HEAD');
});

test('commit normalizes the subject, respects staged-only versus stageAll, and exposes partial push success', async () => {
  const root = await repository();
  await Bun.write(join(root, 'base.txt'), 'staged\n');
  await gitCommand(root, ['add', 'base.txt']);
  await Bun.write(join(root, 'base.txt'), 'unstaged\nextra\n');
  await Bun.write(join(root, 'untracked.txt'), 'new\n');
  const state = await commitState(root);
  expect(state).toMatchObject({
    additions: 3,
    deletions: 1,
    stagedAdditions: 1,
    stagedDeletions: 1,
    hasStaged: true,
    hasUnstaged: true,
    canPush: false,
  });
  const first = await commit(root, { message: 'Explanation\nSubject: `Keep staged only.`' });
  expect(first.subject).toBe('Keep staged only');
  expect((await gitCommand(root, ['show', 'HEAD:base.txt'])).stdout).toBe('staged\n');
  expect((await commitState(root)).hasUnstaged).toBe(true);
  await expect(commit(root, { message: 'Nothing staged' })).rejects.toThrow(
    'there are no staged changes to commit',
  );
  await expect(commit(root, { message: ' \n ' })).rejects.toThrow('enter a commit message');
  const second = await commit(root, { message: 'Include everything', stageAll: true, push: true });
  expect(second.pushError).toBe('no Git remote is configured for this branch');
  expect(second.pushed).toBe(false);
  expect((await gitCommand(root, ['rev-parse', 'HEAD'])).stdout.trim()).toBe(second.commit);
  expect((await gitCommand(root, ['show', 'HEAD:untracked.txt'])).stdout).toBe('new\n');
  expect(await commitState(root)).toMatchObject({ hasStaged: false, hasUnstaged: false });
});

test('push establishes upstream against a real local remote', async () => {
  const root = await repository();
  const remote = await mkdtemp(join(tmpdir(), 'harness-remote-test-'));
  directories.push(remote);
  await gitCommand(remote, ['init', '--bare']);
  await gitCommand(root, ['remote', 'add', 'origin', remote]);
  expect((await commitState(root)).canPush).toBe(true);
  await push(root);
  expect((await gitCommand(root, ['rev-parse', '--abbrev-ref', '@{upstream}'])).stdout.trim()).toBe(
    'origin/main',
  );
  await Bun.write(join(root, 'base.txt'), 'pushed\n');
  const result = await commit(root, { message: 'Push commit', stageAll: true, push: true });
  expect(result.pushed).toBe(true);
  expect((await gitCommand(remote, ['rev-parse', 'refs/heads/main'])).stdout.trim()).toBe(
    result.commit,
  );
});

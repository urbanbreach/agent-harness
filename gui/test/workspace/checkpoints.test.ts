import { afterEach, expect, test } from 'bun:test';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  captureNamedSnapshot,
  captureTurnEnd,
  captureTurnStart,
  checkpointRef,
  copySessionSnapshots,
  deleteSessionSnapshots,
  deleteSnapshot,
  deleteSnapshotsAfter,
  hasSnapshot,
  listSessionSnapshots,
  restoreNamedSnapshot,
} from '../../src/workspace/checkpoints';
import { gitCommand } from '../../src/workspace/git';

const directories: string[] = [];

afterEach(async () => {
  await Promise.all(
    directories.splice(0).map((path) => rm(path, { recursive: true, force: true })),
  );
});

async function directory(): Promise<string> {
  const path = await mkdtemp(join(tmpdir(), 'harness-checkpoint-test-'));
  directories.push(path);
  return path;
}

async function git(cwd: string, ...args: string[]): Promise<string> {
  return (await gitCommand(cwd, args)).stdout.trim();
}

async function repository(): Promise<string> {
  const cwd = await directory();
  await git(cwd, 'init', '--quiet');
  await writeFile(join(cwd, '.gitignore'), 'ignored/\n');
  await writeFile(join(cwd, 'tracked.txt'), 'original\n');
  await writeFile(join(cwd, 'remove.txt'), 'remove me\n');
  await git(cwd, 'add', '-A');
  await git(
    cwd,
    '-c',
    'user.name=Test',
    '-c',
    'user.email=test@localhost',
    'commit',
    '--quiet',
    '-m',
    'initial',
  );
  return cwd;
}

test('capture and restore roundtrip preserves HEAD, captures untracked files, resets staging, and retains ignored files', async () => {
  const cwd = await repository();
  await writeFile(join(cwd, 'tracked.txt'), 'staged\n');
  await git(cwd, 'add', 'tracked.txt');
  await writeFile(join(cwd, 'tracked.txt'), 'manual before turn\n');
  await writeFile(join(cwd, 'captured.txt'), 'captured untracked\n');
  await mkdir(join(cwd, 'ignored'));
  await writeFile(join(cwd, 'ignored', 'cache'), 'keep cache\n');
  const originalHead = await readFile(join(cwd, '.git', 'HEAD'));
  const originalCommit = await git(cwd, 'rev-parse', 'HEAD');
  const originalIndex = await readFile(join(cwd, '.git', 'index'));
  const start = checkpointRef('session', 1, 'start');
  await captureTurnStart(cwd, 'session', 1);
  await writeFile(join(cwd, 'tracked.txt'), 'turn change\n');
  await rm(join(cwd, 'remove.txt'));
  const unusualPath = 'new\tfile\nname.txt';
  await writeFile(join(cwd, unusualPath), 'new turn file\n');
  await writeFile(join(cwd, 'binary.dat'), new Uint8Array([0, 1, 2, 3]));
  // A repeated preparation call must not replace the immutable turn-start state.
  await captureTurnStart(cwd, 'session', 1);
  const end = await captureTurnEnd(cwd, 'session', 1);
  expect(end.state).toBe('ready');
  expect(end.files).toContainEqual({
    path: 'tracked.txt',
    status: 'modified',
    additions: 1,
    deletions: 1,
  });
  expect(end.files).toContainEqual({
    path: 'remove.txt',
    status: 'deleted',
    additions: 0,
    deletions: 1,
  });
  expect(end.files).toContainEqual({
    path: unusualPath,
    status: 'added',
    additions: 1,
    deletions: 0,
  });
  expect(end.files).toContainEqual({ path: 'binary.dat', status: 'added' });
  expect(
    end.files.some((file) => file.path === 'captured.txt' || file.path.startsWith('ignored/')),
  ).toBe(false);
  expect(end.additions).toBe(2);
  expect(end.deletions).toBe(2);
  expect(await readFile(join(cwd, '.git', 'index'))).toEqual(originalIndex);
  expect(await readFile(join(cwd, '.git', 'HEAD'))).toEqual(originalHead);
  expect(await git(cwd, 'rev-parse', 'HEAD')).toBe(originalCommit);
  expect(await readFile(join(cwd, 'tracked.txt'), 'utf8')).toBe('turn change\n');
  expect(await readFile(join(cwd, unusualPath), 'utf8')).toBe('new turn file\n');

  await writeFile(join(cwd, 'tracked.txt'), 'unsaved later edit\n');
  await rm(join(cwd, 'captured.txt'));
  await writeFile(join(cwd, 'new-after.txt'), 'safety must retain me\n');
  await writeFile(join(cwd, 'ignored', 'later'), 'ignored after capture\n');
  const { safetyRef } = await restoreNamedSnapshot(cwd, start);
  expect(await readFile(join(cwd, 'tracked.txt'), 'utf8')).toBe('manual before turn\n');
  expect(await readFile(join(cwd, 'captured.txt'), 'utf8')).toBe('captured untracked\n');
  expect(await readFile(join(cwd, 'remove.txt'), 'utf8')).toBe('remove me\n');
  for (const path of ['new-after.txt', unusualPath, 'binary.dat']) {
    expect(await Bun.file(join(cwd, path)).exists()).toBe(false);
  }
  expect(await readFile(join(cwd, 'ignored', 'cache'), 'utf8')).toBe('keep cache\n');
  expect(await readFile(join(cwd, 'ignored', 'later'), 'utf8')).toBe('ignored after capture\n');
  expect(await git(cwd, 'diff', '--cached', '--name-only')).toBe('');
  expect(await readFile(join(cwd, '.git', 'HEAD'))).toEqual(originalHead);
  expect(await git(cwd, 'rev-parse', 'HEAD')).toBe(originalCommit);
  expect(await hasSnapshot(cwd, safetyRef)).toBe(true);
  expect(await git(cwd, 'show', `${safetyRef}-index:tracked.txt`)).toBe('staged');
  expect(await git(cwd, 'rev-parse', `${safetyRef}-index`)).not.toBe(
    await git(cwd, 'rev-parse', `${safetyRef}^{tree}`),
  );
  await restoreNamedSnapshot(cwd, safetyRef);
  expect(await readFile(join(cwd, 'tracked.txt'), 'utf8')).toBe('unsaved later edit\n');
  expect(await readFile(join(cwd, 'new-after.txt'), 'utf8')).toBe('safety must retain me\n');
  expect(await Bun.file(join(cwd, 'captured.txt')).exists()).toBe(false);
}, 30_000);

test('snapshots exclude untracked env secrets without changing tracked env files or examples', async () => {
  const cwd = await repository();
  await writeFile(join(cwd, '.env.tracked'), 'tracked before\n');
  await git(cwd, 'add', '.env.tracked');
  await git(
    cwd,
    '-c',
    'user.name=Test',
    '-c',
    'user.email=test@localhost',
    'commit',
    '-m',
    'track env',
  );
  await mkdir(join(cwd, 'nested'));
  const secrets = ['.env', '.env.local', 'nested/.env.production'];
  for (const path of [...secrets, '.env.example']) await writeFile(join(cwd, path), 'before\n');
  await writeFile(join(cwd, '.env.tracked'), 'tracked snapshot\n');
  const ref = 'refs/harness-gui/secrets/named';
  await captureNamedSnapshot(cwd, ref);
  const paths = (await git(cwd, 'ls-tree', '-r', '--name-only', ref)).split('\n');
  for (const path of secrets) {
    expect(paths).not.toContain(path);
    await writeFile(join(cwd, path), 'secret after\n');
  }
  expect(paths).toContain('.env.example');
  expect(paths).toContain('.env.tracked');
  await writeFile(join(cwd, '.env.example'), 'example after\n');
  await writeFile(join(cwd, '.env.tracked'), 'tracked after\n');
  const { safetyRef } = await restoreNamedSnapshot(cwd, ref);
  const safetyPaths = (await git(cwd, 'ls-tree', '-r', '--name-only', safetyRef)).split('\n');
  for (const path of secrets) {
    expect(safetyPaths).not.toContain(path);
    expect(await readFile(join(cwd, path), 'utf8')).toBe('secret after\n');
  }
  expect(await readFile(join(cwd, '.env.example'), 'utf8')).toBe('before\n');
  expect(await readFile(join(cwd, '.env.tracked'), 'utf8')).toBe('tracked snapshot\n');
}, 30_000);

test('restore refuses an unmerged real index before changing workspace files', async () => {
  const cwd = await repository();
  const ref = 'refs/harness-gui/conflict/named';
  await captureNamedSnapshot(cwd, ref);
  const blob = await git(cwd, 'rev-parse', 'HEAD:tracked.txt');
  await gitCommand(cwd, ['update-index', '--index-info'], {
    input: `0 ${'0'.repeat(blob.length)}\ttracked.txt\n100644 ${blob} 1\ttracked.txt\n100644 ${blob} 2\ttracked.txt\n100644 ${blob} 3\ttracked.txt\n`,
  });
  await writeFile(join(cwd, 'tracked.txt'), 'unresolved work\n');
  const index = await readFile(join(cwd, '.git', 'index'));
  await expect(restoreNamedSnapshot(cwd, ref)).rejects.toThrow(
    'resolve unmerged index entries first',
  );
  expect(await readFile(join(cwd, '.git', 'index'))).toEqual(index);
  expect(await readFile(join(cwd, 'tracked.txt'), 'utf8')).toBe('unresolved work\n');
}, 30_000);

test('named snapshots and fork refs survive independently and clean up only selected turns', async () => {
  const cwd = await repository();
  const named = 'refs/harness-gui/source/named';
  const initialIndex = await readFile(join(cwd, '.git', 'index'));
  await captureNamedSnapshot(cwd, named);
  expect(await readFile(join(cwd, '.git', 'index'))).toEqual(initialIndex);
  for (const turn of [0, 2, 10]) {
    await captureTurnStart(cwd, 'source', turn);
    await writeFile(join(cwd, 'tracked.txt'), `turn ${turn}\n`);
    await captureTurnEnd(cwd, 'source', turn);
  }
  await copySessionSnapshots(cwd, 'source', 'fork', 2);
  expect(await listSessionSnapshots(cwd, 'fork')).toEqual([0, 2]);
  expect(await git(cwd, 'rev-parse', checkpointRef('fork', 2, 'start'))).toBe(
    await git(cwd, 'rev-parse', checkpointRef('source', 2, 'start')),
  );
  await deleteSnapshotsAfter(cwd, 'source', 2);
  expect(await listSessionSnapshots(cwd, 'source')).toEqual([0, 2]);
  expect(await hasSnapshot(cwd, checkpointRef('source', 10, 'start'))).toBe(false);
  await deleteSessionSnapshots(cwd, 'source');
  expect(await hasSnapshot(cwd, named)).toBe(false);
  expect(await listSessionSnapshots(cwd, 'fork')).toEqual([0, 2]);
  await restoreNamedSnapshot(cwd, checkpointRef('fork', 2, 'end'));
  expect(await readFile(join(cwd, 'tracked.txt'), 'utf8')).toBe('turn 2\n');
  await deleteSnapshot(cwd, checkpointRef('fork', 2, 'end'));
  expect(await hasSnapshot(cwd, checkpointRef('fork', 2, 'end'))).toBe(false);
}, 30_000);

test('plain directories distinguish unavailable turn checkpoints from explicit snapshot errors and reject unsafe refs', async () => {
  const cwd = await directory();
  await writeFile(join(cwd, 'keep.txt'), 'untouched\n');
  expect(await captureTurnStart(cwd, 'plain', 1)).toBeNull();
  expect((await captureTurnEnd(cwd, 'plain', 1)).state).toBe('unavailable');
  await expect(captureNamedSnapshot(cwd, 'refs/harness-gui/plain/named')).rejects.toThrow(
    'checkpoints require a Git repository',
  );
  expect(await hasSnapshot(cwd, 'refs/harness-gui/plain/named')).toBe(false);
  for (const ref of [
    'HEAD',
    'refs/heads/main',
    'refs/harness-gui/session/../../heads/main',
    'refs/harness-gui/session/name.lock',
    'refs/harness-gui/session/name\n',
  ]) {
    await expect(captureNamedSnapshot(cwd, ref)).rejects.toThrow('invalid checkpoint reference');
  }
  for (const session of ['../escape', '', 'other/session']) {
    await expect(captureTurnStart(cwd, session, 1)).rejects.toThrow('invalid checkpoint session');
  }
  expect(() => checkpointRef('safe', -1, 'end')).toThrow('invalid checkpoint turn');
  expect(await readFile(join(cwd, 'keep.txt'), 'utf8')).toBe('untouched\n');
});

test('unborn repositories capture and restore without inventing HEAD and cancellation leaves no snapshot', async () => {
  const cwd = await directory();
  await git(cwd, 'init', '--quiet');
  await writeFile(join(cwd, 'first.txt'), 'unborn snapshot\n');
  const head = await readFile(join(cwd, '.git', 'HEAD'));
  const ref = 'refs/harness-gui/unborn/named';
  const controller = new AbortController();
  controller.abort();
  await expect(captureNamedSnapshot(cwd, ref, controller.signal)).rejects.toThrow();
  expect(await hasSnapshot(cwd, ref)).toBe(false);
  await captureNamedSnapshot(cwd, ref);
  expect(await Bun.file(join(cwd, '.git', 'index')).exists()).toBe(false);
  await writeFile(join(cwd, 'first.txt'), 'later\n');
  await writeFile(join(cwd, 'later.txt'), 'delete\n');
  await restoreNamedSnapshot(cwd, ref);
  expect(await readFile(join(cwd, 'first.txt'), 'utf8')).toBe('unborn snapshot\n');
  expect(await Bun.file(join(cwd, 'later.txt')).exists()).toBe(false);
  expect(await readFile(join(cwd, '.git', 'HEAD'))).toEqual(head);
  expect(await Bun.file(join(cwd, '.git', 'index')).exists()).toBe(false);
}, 30_000);

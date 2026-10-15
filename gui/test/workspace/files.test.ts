import { test, expect } from 'bun:test';
import { mkdtemp, mkdir, writeFile, rm, symlink } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import {
  listTree,
  readText,
  writeText,
  buildFileIndex,
  searchFiles,
} from '../../src/workspace/files';

test('visible tree expansion differs intentionally from ignored-file completion', async () => {
  const root = await mkdtemp(join(tmpdir(), 'harness-files-'));
  try {
    await mkdir(join(root, 'Beta'));
    await mkdir(join(root, '.git'));
    await writeFile(join(root, 'Beta', 'child.ts'), 'child');
    await writeFile(join(root, '.gitignore'), 'ignored.txt\n');
    await writeFile(join(root, 'ignored.txt'), 'ignored');
    await writeFile(join(root, 'alpha.ts'), 'alpha');
    const init = Bun.spawn(['git', 'init', root], { stdout: 'ignore', stderr: 'ignore' });
    expect(await init.exited).toBe(0);
    expect((await listTree(root)).map((entry) => entry.path)).toEqual([
      'Beta',
      '.gitignore',
      'alpha.ts',
      'ignored.txt',
    ]);
    const expanded = await listTree(root, new Set([join(root, 'Beta')]));
    expect(expanded[1]).toMatchObject({ path: 'Beta/child.ts', depth: 1, directory: false });
    const index = await buildFileIndex(root);
    expect(index.entries.map((entry) => entry.path)).toEqual([
      '.gitignore',
      'alpha.ts',
      'Beta',
      'Beta/child.ts',
    ]);
    expect(searchFiles(index, 'bct').map((entry) => entry.path)).toEqual(['Beta/child.ts']);
    expect(await buildFileIndex(root, { cap: 1 })).toMatchObject({
      truncated: true,
      entries: [{ path: '.gitignore', directory: false }],
    });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('UTF-8 file operations reject traversal, escaping symlinks and invalid bytes', async () => {
  const root = await mkdtemp(join(tmpdir(), 'harness-paths-'));
  const outside = await mkdtemp(join(tmpdir(), 'harness-outside-'));
  try {
    await writeText(root, 'safe.txt', 'héllo\n');
    expect(await readText(root, 'safe.txt')).toBe('héllo\n');
    await symlink(outside, join(root, 'escape'));
    await symlink(join(outside, 'missing.txt'), join(root, 'dangling'));
    for (const path of [
      '',
      '/etc/passwd',
      '../escape',
      'a/../safe.txt',
      'escape/out.txt',
      'dangling',
    ]) {
      await expect(writeText(root, path, 'unsafe')).rejects.toThrow();
      await expect(readText(root, path)).rejects.toThrow();
    }
    await writeFile(join(root, 'binary'), new Uint8Array([0xff, 0xfe]));
    await expect(readText(root, 'binary')).rejects.toThrow();
    const controller = new AbortController();
    controller.abort();
    await expect(buildFileIndex(root, { signal: controller.signal })).rejects.toThrow();
  } finally {
    await rm(root, { recursive: true, force: true });
    await rm(outside, { recursive: true, force: true });
  }
});

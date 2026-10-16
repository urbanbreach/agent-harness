import { expect, test } from 'bun:test';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createFilesState, saveFileState } from '../../src/app/panel/files';
import { readText } from '../../src/workspace/files';

test('editor saves in its worktree and preserves external changes and unsaved edits', async () => {
  const root = await mkdtemp(join(tmpdir(), 'harness-editor-'));
  const parent = join(root, 'parent');
  const worktree = join(root, 'worktree');
  try {
    await mkdir(parent);
    await mkdir(worktree);
    await writeFile(join(parent, 'example.txt'), 'parent');
    await writeFile(join(worktree, 'example.txt'), 'baseline');
    const state = createFilesState();
    state.selected = 'example.txt';
    const buffer = { text: 'edited', saved: 'baseline', loaded: true };
    state.buffers.set(state.selected, buffer);
    await saveFileState(worktree, state);
    expect(await readText(parent, state.selected)).toBe('parent');
    expect(await readText(worktree, state.selected)).toBe('edited');
    expect(buffer.saved).toBe('edited');
    await writeFile(join(worktree, state.selected), 'external');
    await saveFileState(worktree, state);
    expect(await readText(worktree, state.selected)).toBe('external');
    buffer.text = 'unsaved';
    await expect(saveFileState(worktree, state)).rejects.toThrow('changed on disk');
    expect(await readText(worktree, state.selected)).toBe('external');
    expect(buffer).toMatchObject({ text: 'unsaved', saved: 'edited' });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

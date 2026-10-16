import { expect, test } from 'bun:test';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { openUrl } from '../../src/workspace/os';

test('external URLs allow only web and mail schemes', async () => {
  const root = await mkdtemp(join(tmpdir(), 'harness-open-url-'));
  const previousPath = process.env.PATH;
  try {
    await writeFile(join(root, 'xdg-open'), '#!/bin/sh\nprintf "%s" "$1" > "$0.result"\n', {
      mode: 0o700,
    });
    process.env.PATH = `${root}:${previousPath ?? ''}`;
    for (const [url, allowed] of [
      ['http://example.invalid/', true],
      ['https://example.invalid/', true],
      ['mailto:person@example.invalid', true],
      ['file:///tmp/example', false],
      ['ssh://example.invalid/', false],
      ['smb://example.invalid/share', false],
      ['vscode://example.invalid/', false],
      ['steam://example.invalid/', false],
      ['javascript:alert(1)', false],
      ['data:text/plain,example', false],
      ['vbscript:example', false],
    ] as const) {
      await rm(join(root, 'xdg-open.result'), { force: true });
      if (allowed) {
        await openUrl(url);
        expect(await readFile(join(root, 'xdg-open.result'), 'utf8')).toBe(url);
      } else {
        await expect(openUrl(url)).rejects.toThrow('Unsupported external URL scheme');
        expect(await Bun.file(join(root, 'xdg-open.result')).exists()).toBe(false);
      }
    }
  } finally {
    if (previousPath === undefined) delete process.env.PATH;
    else process.env.PATH = previousPath;
    await rm(root, { recursive: true, force: true });
  }
});

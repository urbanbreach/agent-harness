import { expect, test } from 'bun:test';
import type { Backend } from '../../src/backend/client';
import { resolveToolImage } from '../../src/app/transcript/attachments';

test('tool images resolve session attachments with metadata MIME and retain direct image sources', async () => {
  const requests: unknown[] = [];
  const backend = {
    async call(method: string, params: unknown) {
      requests.push({ method, params });
      if (method !== 'sessions.attachment') throw new Error('Not a session attachment request');
      return { dataBase64: 'aW1hZ2U=' };
    },
  } as unknown as Backend;
  const image = { ref: `attachment:blake3:${'a'.repeat(64)}`, mime: 'image/webp' };
  expect(await resolveToolImage(backend, '/sessions/run', image)).toBe(
    'data:image/webp;base64,aW1hZ2U=',
  );
  expect(requests).toEqual([
    { method: 'sessions.attachment', params: { runDir: '/sessions/run', ref: image.ref } },
  ]);
  expect(
    await resolveToolImage(backend, undefined, {
      ref: 'data:image/png;base64,eA==',
      mime: 'image/png',
    }),
  ).toBe('data:image/png;base64,eA==');
  expect(await resolveToolImage(backend, undefined, image)).toBeUndefined();
  expect(
    await resolveToolImage(backend, '/sessions/run', { ...image, mime: 'text/plain' }),
  ).toBeUndefined();
  expect(requests).toHaveLength(1);
  const failing = {
    call: async () => {
      throw new Error('Missing attachment');
    },
  } as unknown as Backend;
  await expect(resolveToolImage(failing, '/sessions/run', image)).rejects.toThrow(
    'Missing attachment',
  );
});

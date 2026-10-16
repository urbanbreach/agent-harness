import { t } from '../../i18n';
import { lstat, readdir } from 'node:fs/promises';
import { basename, extname, join } from 'node:path';
import type { DraftAttachment } from '../../store/store';

export const maxAttachmentBytes = 33_554_432;
const imageMime: Record<string, string> = {
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.bmp': 'image/bmp',
  '.svg': 'image/svg+xml',
  '.tif': 'image/tiff',
  '.tiff': 'image/tiff',
  '.ico': 'image/x-icon',
  '.pnm': 'image/x-portable-anymap',
  '.pbm': 'image/x-portable-bitmap',
  '.pgm': 'image/x-portable-graymap',
  '.ppm': 'image/x-portable-pixmap',
};
export async function importPath(path: string): Promise<DraftAttachment> {
  const info = await lstat(path);
  if (!info.isFile() && !info.isDirectory()) throw new Error(t('composer.attachment_type_error'));
  let bytes = 0;
  let count = 0;
  async function inspect(current: string): Promise<void> {
    const entry = await lstat(current);
    if (entry.isFile()) {
      bytes += entry.size;
      count += 1;
      if (bytes > maxAttachmentBytes) throw new Error(t('composer.attachment_size_error'));
      if (count > 4096) throw new Error(t('composer.attachment_count_error'));
    } else if (entry.isDirectory()) {
      for (const name of await readdir(current)) await inspect(join(current, name));
    }
  }
  await inspect(path);
  const mime = imageMime[extname(path).toLocaleLowerCase()] ?? 'application/octet-stream';
  return {
    name: basename(path),
    path,
    directory: info.isDirectory(),
    mime,
    dataBase64:
      mime.startsWith('image/') && info.isFile()
        ? Buffer.from(await Bun.file(path).arrayBuffer()).toString('base64')
        : '',
  };
}

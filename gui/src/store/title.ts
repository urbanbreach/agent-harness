import type { SessionRow } from '../backend/protocol';
import type { SessionView } from '../model/session';
import { t } from '../i18n';

export function taskTitle(row: SessionRow | undefined, view?: SessionView) {
  const title = (view?.title || row?.catalog.run_name || '').trim();
  if (title && !/^New session - \d{4}-\d{2}-\d{2}T/.test(title)) return title;
  const prompts = view?.entries.filter((entry) => entry.kind === 'user');
  if (prompts?.length === 1) {
    const fallback = prompts[0]!.text.trim().split(/\s+/).slice(0, 7).join(' ').slice(0, 54);
    if (fallback) return fallback;
  }
  return t('task.untitled');
}

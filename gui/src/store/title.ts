import type { SessionRow } from '../backend/protocol';
import type { SessionView } from '../model/session';
import { t } from '../i18n';

export function taskTitle(row: SessionRow | undefined, view?: SessionView) {
  const title = (view?.title || row?.catalog.run_name || '').trim();
  if (title && !/^New session - \d{4}-\d{2}-\d{2}T/.test(title)) return title;
  // Harness has no automatic titles, so the first prompt keeps naming the task after later turns.
  const prompt = view?.entries.find((entry) => entry.kind === 'user');
  const fallback = prompt?.text.trim().split(/\s+/).slice(0, 7).join(' ').slice(0, 54);
  if (fallback) return fallback;
  return t('task.untitled');
}

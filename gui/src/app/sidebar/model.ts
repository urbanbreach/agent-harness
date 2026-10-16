import type { SessionRow } from '../../backend/protocol';
import type { SessionView } from '../../model/session';
import { t } from '../../i18n';

export const dateBuckets = [
  'Today',
  'Yesterday',
  'This Week',
  'This Month',
  'This Year',
  'More',
] as const;
export const bucketLabels = {
  Today: 'sidebar.today',
  Yesterday: 'sidebar.yesterday',
  'This Week': 'sidebar.this_week',
  'This Month': 'sidebar.this_month',
  'This Year': 'sidebar.this_year',
  More: 'sidebar.more',
} as const;
export function sidebarTimestamp(row: SessionRow): number {
  const value = Date.parse(row.catalog.last_updated_at || row.createdAt || '');
  return Number.isFinite(value) ? value : Infinity;
}
export function dateBucket(timestamp: number, now: number): (typeof dateBuckets)[number] {
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  const monday = new Date(today);
  monday.setDate(today.getDate() - ((today.getDay() + 6) % 7));
  if (!Number.isFinite(timestamp) || timestamp >= today.getTime()) return 'Today';
  if (timestamp >= yesterday.getTime()) return 'Yesterday';
  if (timestamp >= monday.getTime()) return 'This Week';
  if (timestamp >= new Date(today.getFullYear(), today.getMonth(), 1).getTime())
    return 'This Month';
  if (timestamp >= new Date(today.getFullYear(), 0, 1).getTime()) return 'This Year';
  return 'More';
}
export function relativeTime(timestamp: number, now: number): string {
  const seconds = Math.max(0, Math.floor((now - timestamp) / 1000));
  if (seconds < 60) return t('sidebar.just_now');
  if (seconds < 3600) return t('sidebar.minutes_ago', { count: Math.floor(seconds / 60) });
  if (seconds < 86400) return t('sidebar.hours_ago', { count: Math.floor(seconds / 3600) });
  return t('sidebar.days_ago', { count: Math.floor(seconds / 86400) });
}
export function workingTime(seconds: number): string {
  seconds = Math.max(0, Math.floor(seconds));
  if (seconds < 60) return t('shell.duration_seconds', { count: seconds });
  if (seconds < 3600)
    return `${t('shell.duration_minutes', { count: Math.floor(seconds / 60) })}${seconds % 60 ? ` ${t('shell.duration_seconds', { count: seconds % 60 })}` : ''}`;
  const minutes = Math.floor(seconds / 60) % 60;
  return `${t('shell.duration_hours', { count: Math.floor(seconds / 3600) })}${minutes ? ` ${t('shell.duration_minutes', { count: minutes })}` : ''}`;
}
export function taskTime(row: SessionRow, view: SessionView | undefined, now: number): string {
  if (view?.status === 'background') return t('task.background');
  if (view?.status === 'working') {
    let turnStartedMs: number | undefined;
    for (let index = view.turns.length - 1; index >= 0; index--) {
      const turn = view.turns[index];
      if (turn?.status === 'working') {
        turnStartedMs = turn.startedMs;
        break;
      }
    }
    const event = view.events.find((item) => item.mono_ms === turnStartedMs);
    const started = event?.ts ? Date.parse(event.ts) : NaN;
    if (Number.isFinite(started))
      return t('sidebar.working', { elapsed: workingTime((now - started) / 1000) });
    return t('shell.working');
  }
  const replied = row.catalog.last_updated_at;
  return replied && Number.isFinite(Date.parse(replied))
    ? relativeTime(Date.parse(replied), now)
    : '';
}

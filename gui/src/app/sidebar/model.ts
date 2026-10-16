import type { SessionRow } from '../../backend/protocol';
import type { SessionView } from '../../model/session';

export const dateBuckets = [
  'Today',
  'Yesterday',
  'This Week',
  'This Month',
  'This Year',
  'More',
] as const;
export function sidebarTimestamp(row: SessionRow): number {
  const value = Date.parse(row.catalog.last_updated_at || row.createdAt || '');
  return Number.isFinite(value) ? value : Infinity;
}
export function dateBucket(timestamp: number, now: number): string {
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
  if (seconds < 60) return 'just now';
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
}
export function workingTime(seconds: number): string {
  seconds = Math.max(0, Math.floor(seconds));
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600)
    return `${Math.floor(seconds / 60)}m${seconds % 60 ? ` ${seconds % 60}s` : ''}`;
  const minutes = Math.floor(seconds / 60) % 60;
  return `${Math.floor(seconds / 3600)}h${minutes ? ` ${minutes}m` : ''}`;
}
export function taskTime(row: SessionRow, view: SessionView | undefined, now: number): string {
  if (view?.status === 'background') return 'Waiting for background tasks';
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
    if (Number.isFinite(started)) return `Working for ${workingTime((now - started) / 1000)}`;
    return 'Working';
  }
  const replied = row.catalog.last_updated_at;
  return replied && Number.isFinite(Date.parse(replied))
    ? relativeTime(Date.parse(replied), now)
    : '';
}

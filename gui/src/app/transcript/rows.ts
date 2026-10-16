import { t, getLocale } from '../../i18n';
import type { SessionView, TranscriptEntry, TurnView, TextEntry } from '../../model/session';

export type TranscriptRow =
  | { id: string; kind: 'message'; entry: TranscriptEntry }
  | { id: string; kind: 'activity'; entries: TranscriptEntry[]; live: boolean }
  | { id: string; kind: 'fold'; turn: TurnView }
  | { id: string; kind: 'footer'; turn: TurnView; text: string }
  | { id: string; kind: 'working'; turn?: TurnView };

export function duration(ms: number, compact = false): string {
  const seconds = Math.max(compact ? 0 : 1, Math.floor(ms / 1000));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor(seconds / 60) % 60;
  const rest = seconds % 60;
  const unit = (n: number, word: 'hour' | 'minute' | 'second') =>
    t(`transcript.${word}_${compact ? 'short' : n === 1 ? 'one' : 'other'}`, { count: n });
  return hours
    ? `${unit(hours, 'hour')}${minutes ? ` ${unit(minutes, 'minute')}` : ''}`
    : minutes
      ? `${unit(minutes, 'minute')}${rest ? ` ${unit(rest, 'second')}` : ''}`
      : unit(seconds, 'second');
}

export function timestamp(ms: number, now = new Date()): string {
  const date = new Date(ms);
  if (!Number.isFinite(ms) || ms <= 0) return '';
  const locale = getLocale();
  const time = date.toLocaleTimeString(locale === 'en' ? 'en-US' : locale, {
    hour: 'numeric',
    minute: '2-digit',
  });
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const day = new Date(date.getFullYear(), date.getMonth(), date.getDate());
  if (day >= today) return time;
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  if (day.getTime() === yesterday.getTime()) return t('transcript.yesterday', { time });
  const monday = new Date(today);
  monday.setDate(today.getDate() - ((today.getDay() + 6) % 7));
  if (day >= monday)
    return `${date.toLocaleDateString(locale === 'en' ? 'en-US' : locale, { weekday: 'long' })} ${time}`;
  if (locale !== 'en')
    return `${date.toLocaleDateString(locale, { month: 'short', day: 'numeric', ...(date.getFullYear() !== now.getFullYear() ? ({ year: 'numeric' } as const) : {}) })} ${time}`;
  const n = date.getDate();
  const suffix =
    n % 100 >= 11 && n % 100 <= 13
      ? 'th'
      : (({ 1: 'st', 2: 'nd', 3: 'rd' } as Record<number, string>)[n % 10] ?? 'th');
  return `${date.toLocaleDateString('en-US', { month: 'short' })} ${n}${suffix}${date.getFullYear() !== now.getFullYear() ? `, ${date.getFullYear()}` : ''}, ${time}`;
}

export function transcriptRows(
  view: SessionView,
  expanded: ReadonlySet<string>,
  showReasoning = true,
): TranscriptRow[] {
  const rows: TranscriptRow[] = [];
  const turns = new Map(view.turns.map((turn) => [turn.id, turn]));
  const grouped = new Set<string>();
  const visibleEntries = showReasoning
    ? view.entries
    : view.entries.filter((entry) => entry.kind !== 'reasoning');
  const entriesByTurn = new Map<string, TranscriptEntry[]>();
  for (const entry of visibleEntries) {
    if (!entry.turnId) continue;
    const entries = entriesByTurn.get(entry.turnId);
    if (entries) entries.push(entry);
    else entriesByTurn.set(entry.turnId, [entry]);
  }
  const append = (entries: TranscriptEntry[], live: boolean) => {
    let activity: TranscriptEntry[] = [];
    const flush = () => {
      if (activity.length)
        rows.push({ id: `activity:${activity[0]!.id}`, kind: 'activity', entries: activity, live });
      activity = [];
    };
    for (const entry of entries) {
      if (
        entry.kind === 'tool' ||
        entry.kind === 'reasoning' ||
        entry.kind === 'compaction' ||
        entry.kind === 'notice'
      )
        activity.push(entry);
      else {
        flush();
        rows.push({ id: entry.id, kind: 'message', entry });
      }
    }
    flush();
  };
  for (const entry of visibleEntries) {
    const turn = entry.turnId ? turns.get(entry.turnId) : undefined;
    if (!turn) {
      append([entry], false);
      continue;
    }
    if (grouped.has(turn.id)) continue;
    grouped.add(turn.id);
    const entries = entriesByTurn.get(turn.id)!;
    const live = turn.status === 'working' || turn.status === 'queued';
    if (live) {
      append(entries, true);
      continue;
    }
    const opening = entries.findIndex((item) => item.kind !== 'user');
    if (opening < 0) {
      append(entries, false);
      continue;
    }
    append(entries.slice(0, opening), false);
    let answerStart = entries.length;
    while (answerStart > opening && entries[answerStart - 1]?.kind === 'text') answerStart--;
    const work = entries.slice(opening, answerStart);
    if (work.length || turn.status === 'interrupted') {
      rows.push({ id: `fold:${turn.id}`, kind: 'fold', turn });
      if (expanded.has(turn.id)) append(work, false);
    }
    const answer = entries.slice(answerStart);
    append(answer, false);
    rows.push({
      id: `footer:${turn.id}`,
      kind: 'footer',
      turn,
      text: answer
        .filter((item): item is TextEntry => item.kind === 'text')
        .map((item) => item.text)
        .join('\n\n'),
    });
  }
  if (view.status === 'working' || view.status === 'waiting' || view.status === 'background')
    rows.push({ id: 'working', kind: 'working', turn: view.turns.at(-1) });
  return rows;
}

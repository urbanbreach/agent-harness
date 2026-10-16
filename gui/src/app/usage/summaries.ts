import type { UsageAggregate, UsageHistory } from '../../backend/extensions';
import type { Project } from '../../store';
import { t } from '../../i18n';

function mergeRows(rows: UsageAggregate[]): UsageAggregate[] {
  const grouped = new Map<string, UsageAggregate>();
  for (const row of rows) {
    const previous = grouped.get(row.key);
    if (!previous) {
      grouped.set(row.key, { ...row });
      continue;
    }
    const numeric = [
      'records',
      'sessions',
      'inputTokens',
      'cachedInputTokens',
      'cacheWriteTokens',
      'outputTokens',
      'reasoningTokens',
      'totalTokens',
      'knownCostUsd',
      'unpricedRecords',
    ] as const;
    for (const key of numeric) previous[key] += row[key];
    previous.costUsd = previous.unpricedRecords ? null : previous.knownCostUsd;
    previous.lastActive =
      [previous.lastActive, row.lastActive]
        .filter((value): value is string => value !== null)
        .sort()
        .at(-1) ?? null;
    previous.providers = mergeRows([...previous.providers, ...row.providers]);
    previous.models = mergeRows([...previous.models, ...row.models]);
  }
  return [...grouped.values()];
}

export function groupProjects(rows: UsageAggregate[], projects: Project[]): UsageAggregate[] {
  const roots = [...projects].sort((a, b) => b.cwd.length - a.cwd.length);
  return mergeRows(
    rows.map((row) => {
      const path = row.project ?? row.key;
      const root = roots.find(
        (root) => path === root.cwd || path.startsWith(`${root.cwd.replace(/\/$/, '')}/`),
      );
      return { ...row, key: root?.cwd ?? path, project: root?.cwd ?? (path || undefined) };
    }),
  )
    .map((row) => {
      const root = roots.find((root) => root.cwd === row.key);
      return {
        ...row,
        key:
          root?.name ?? row.project?.split('/').filter(Boolean).at(-1) ?? t('usage.other_sessions'),
      };
    })
    .sort((a, b) => b.knownCostUsd - a.knownCostUsd || b.totalTokens - a.totalTokens);
}

export function monthRows(data: UsageHistory): UsageAggregate[] {
  const rows = [...data.byMonth].sort((a, b) => b.key.localeCompare(a.key));
  if (!rows.length) return rows;
  const earliest = rows.at(-1)!.key.slice(0, 7);
  const filled: UsageAggregate[] = [];
  for (
    const month = new Date(`${data.to.slice(0, 7)}-01T12:00:00Z`);
    month.toISOString().slice(0, 7) >= earliest;
    month.setUTCMonth(month.getUTCMonth() - 1)
  ) {
    const key = month.toISOString().slice(0, 7);
    filled.push(
      rows.find((row) => row.key.slice(0, 7) === key) ?? {
        key,
        records: 0,
        sessions: 0,
        inputTokens: 0,
        cachedInputTokens: 0,
        cacheWriteTokens: 0,
        outputTokens: 0,
        reasoningTokens: 0,
        totalTokens: 0,
        costUsd: 0,
        knownCostUsd: 0,
        unpricedRecords: 0,
        lastActive: null,
        providers: [],
        models: [],
      },
    );
  }
  return filled;
}

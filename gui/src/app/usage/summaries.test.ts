import { expect, test } from 'bun:test';
import type { UsageAggregate, UsageHistory } from '../../backend/extensions';
import { groupProjects, monthRows } from './summaries';

function aggregate(key: string, tokens = 0, cost = 0): UsageAggregate {
  return {
    key,
    records: tokens ? 1 : 0,
    sessions: tokens ? 1 : 0,
    inputTokens: tokens,
    cachedInputTokens: 0,
    cacheWriteTokens: 0,
    outputTokens: 0,
    reasoningTokens: 0,
    totalTokens: tokens,
    costUsd: cost,
    knownCostUsd: cost,
    unpricedRecords: 0,
    lastActive: null,
    providers: [],
    models: [],
  };
}

test('project usage rolls descendants into the most specific registered root without losing unpriced totals', () => {
  const first = {
    ...aggregate('/work/app/src', 100, 1),
    project: '/work/app/src',
    providers: [aggregate('anthropic', 100, 1)],
  };
  const second = {
    ...aggregate('/work/app/test', 200),
    project: '/work/app/test',
    unpricedRecords: 1,
    costUsd: null,
    providers: [{ ...aggregate('anthropic', 200), unpricedRecords: 1, costUsd: null }],
  };
  const nested = { ...aggregate('/work/app/plugin/lib', 50, 2), project: '/work/app/plugin/lib' };
  const result = groupProjects(
    [first, second, nested, aggregate('', 10)],
    [
      { id: 'app', name: 'App', cwd: '/work/app' },
      { id: 'plugin', name: 'Plugin', cwd: '/work/app/plugin' },
    ],
  );
  expect(
    result.map((row) => [row.key, row.project, row.totalTokens, row.costUsd, row.knownCostUsd]),
  ).toEqual([
    ['Plugin', '/work/app/plugin', 50, 2, 2],
    ['App', '/work/app', 300, null, 1],
    ['Other sessions', undefined, 10, 0, 0],
  ]);
  expect(result[1]!.providers[0]).toMatchObject({
    totalTokens: 300,
    costUsd: null,
    knownCostUsd: 1,
    unpricedRecords: 1,
  });
  expect(first.totalTokens).toBe(100);
  expect(first.providers[0]!.totalTokens).toBe(100);
});

test('monthly usage retains calendar gaps through the current month but omits earlier empty months', () => {
  const january = aggregate('2026-01', 100, 1);
  const march = aggregate('2026-03', 50, 2);
  const history: UsageHistory = {
    from: '2025-04-01',
    to: '2026-03-15',
    timeZone: 'UTC',
    totals: aggregate('total', 150, 3),
    byCategory: [],
    byModel: [],
    byProvider: [],
    byProject: [],
    byDay: [],
    byMonth: [january, march],
    quality: { providerReportedRecords: 2, modelPricedRecords: 0, unpricedRecords: 0 },
    scan: { scanned: 2, skipped: 0, records: 2, elapsedMs: 1 },
    errors: [],
    ratesAvailable: true,
    cacheSavingsUsd: null,
  };
  expect(monthRows(history).map((row) => [row.key, row.totalTokens])).toEqual([
    ['2026-03', 50],
    ['2026-02', 0],
    ['2026-01', 100],
  ]);
});

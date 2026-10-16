import { useEffect, useMemo, useRef, useState } from 'react';
import { useGpuix, type PublicInstance } from '@gpuix/react';
import type { Methods } from '../../backend/protocol';
import { getAppStore, useProjects, useStore } from '../../store';
import { useTheme } from '../../theme';
import { providerColor as baseProviderColor, type ThemeColors } from '../../theme/colors';
import { Button, Dropdown, Icon, Segmented, Spinner, TextField, Tooltip, useToast } from '../../ui';
import { ellipsis, row } from '../../ui/controls';
import { Pulse } from './pulse';
import { groupProjects, monthRows } from './summaries';
import { t, useLocale, type TranslationKey } from '../../i18n';

type Snapshot = Methods['usage.history']['result'];
type Aggregate = Snapshot['totals'];
type Metric = 'cost' | 'tokens';
const labels: Record<string, string> = {
  claude: 'Claude Code',
  anthropic: 'Claude Code',
  'anthropic-subscription': 'Claude Code',
  codex: 'Codex',
  openai: 'OpenAI',
};
const providerFamily = (provider: string) =>
  /claude|anthropic/i.test(provider)
    ? 'claude'
    : /codex|openai/i.test(provider)
      ? 'codex'
      : provider;
const providerColor = (provider: string, colors: ThemeColors) =>
  baseProviderColor(providerFamily(provider), colors);
function useUsageFormats() {
  const locale = useLocale();
  return useMemo(() => {
    const compact = Intl.NumberFormat(locale, {
      notation: 'compact',
      maximumFractionDigits: 1,
    }).format;
    const money = (value: number) =>
      Intl.NumberFormat(locale === 'en' ? 'en-US' : locale, {
        style: 'currency',
        currency: 'USD',
        maximumFractionDigits: value > 0 && value < 0.01 ? 4 : 2,
      }).format(value);
    const dollars = (aggregate: Aggregate) =>
      `${money(aggregate.knownCostUsd)}${aggregate.unpricedRecords ? '+' : ''}`;
    return { compact, money, dollars };
  }, [locale]);
}
const share = (part: number, total: number) =>
  `${(total > 0 ? (part / total) * 100 : 0).toFixed(1)}%`;
const metricValue = (aggregate: Aggregate, metric: Metric) =>
  metric === 'cost' ? aggregate.knownCostUsd : aggregate.totalTokens;
const iso = (date: Date) =>
  `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
const parseDay = (value: string) => new Date(`${value.slice(0, 10)}T12:00:00`);
const shortDay = (value: string, locale: string) =>
  parseDay(value).toLocaleDateString(locale, { month: 'short', day: 'numeric' });
const ranges: { id: string; key: TranslationKey }[] = [
  { id: '7', key: 'usage.last_7_days' },
  { id: '30', key: 'usage.last_30_days' },
  { id: '90', key: 'usage.last_90_days' },
  { id: 'this', key: 'usage.this_month' },
  { id: 'last', key: 'usage.last_month' },
];

export function UsagePage({ cwd }: { cwd?: string }) {
  const locale = useLocale();
  const { compact, money, dollars } = useUsageFormats();
  const { colors, sp } = useTheme();
  const { showToast } = useToast();
  const registeredProjects = useProjects();
  const home = useStore((state) => state.initialized?.home);
  const [view, setView] = useState('daily');
  const [range, setRange] = useState('30');
  const [metric, setMetric] = useState<Metric>('cost');
  const [breakdown, setBreakdown] = useState('model');
  const [query, setQuery] = useState('');
  const [snapshot, setSnapshot] = useState<Snapshot>();
  const [snapshotKey, setSnapshotKey] = useState<{ cwd?: string; shape: string }>();
  const [pending, setPending] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const request = useRef(0);
  const monthlyView = view === 'monthly';
  const shape = monthlyView ? 'months' : ['7', '30', '90'].includes(range) ? 'days' : range;
  const window = useMemo(() => {
    const end = new Date(),
      start = new Date(end);
    if (monthlyView) {
      start.setDate(1);
      start.setMonth(start.getMonth() - 11);
    } else if (range === 'this') start.setDate(1);
    else if (range === 'last') {
      start.setDate(1);
      start.setMonth(start.getMonth() - 1);
      end.setDate(0);
    } else start.setDate(start.getDate() - Number(range) + 1);
    return { from: iso(start), to: iso(end) };
  }, [monthlyView, range, refresh]);
  useEffect(() => {
    let active = true;
    const serial = ++request.current;
    setPending(true);
    void getAppStore()
      .backend.call('usage.history', { cwd, ...window })
      .then((result) => {
        if (active && serial === request.current) {
          setSnapshotKey({ cwd, shape });
          setSnapshot(result);
        }
      })
      .catch((reason) => {
        if (active) showToast(t('usage.refresh_failed', { error: String(reason) }));
      })
      .finally(() => {
        if (active) setPending(false);
      });
    return () => {
      active = false;
    };
  }, [cwd, window, showToast, shape]);
  const data =
    snapshot &&
    snapshotKey &&
    snapshotKey.cwd === cwd &&
    ((snapshot.from === window.from && snapshot.to === window.to) ||
      (snapshotKey.shape === shape && (shape === 'days' || shape === 'months')))
      ? snapshot
      : undefined;
  const displayed = data;
  const total = data?.totals;
  const sortedModels = data
    ? [...data.byModel].sort(
        (a, b) => b.knownCostUsd - a.knownCostUsd || b.totalTokens - a.totalTokens,
      )
    : [];
  const groupedProjects = useMemo(
    () => groupProjects(data?.byProject ?? [], registeredProjects),
    [data, registeredProjects, locale],
  );
  const projects = groupedProjects.filter((p) =>
    `${p.key} ${p.project ?? ''}`.toLowerCase().includes(query.trim().toLowerCase()),
  );
  const monthly = useMemo(() => (data ? monthRows(data) : []), [data]);
  const providers = [
    ...new Set(['claude', 'codex', ...(data?.byProvider.map((p) => providerFamily(p.key)) ?? [])]),
  ];
  const sourceRows = view === 'monthly' ? monthly : projects;
  const rankingMetric: Metric = total && total.knownCostUsd > 0 ? 'cost' : 'tokens';
  const maxMagnitude = Math.max(0, ...sourceRows.map((p) => metricValue(p, rankingMetric)));
  const text = { fontSize: sp(12.5), color: colors.text_secondary };
  const segmentStyle = {
    borderWidth: 1,
    borderColor: colors.border_strong,
    borderRadius: 7,
    overflow: 'hidden',
  };
  const itemStyle = { height: 26, paddingLeft: 10, paddingRight: 10, fontSize: sp(12.5) };
  const sections = total
    ? [
        [
          t('usage.processed_tokens'),
          compact(total.totalTokens),
          t('usage.per_active_day', {
            count: compact(
              total.totalTokens / Math.max(1, data!.byDay.filter((d) => d.totalTokens > 0).length),
            ),
          }),
        ],
        [
          t('usage.cached_input'),
          compact(total.cachedInputTokens),
          t('usage.observed_input_share', {
            share: share(total.cachedInputTokens, total.cachedInputTokens + total.inputTokens),
          }),
        ],
        [
          t('usage.uncached_input'),
          compact(total.inputTokens),
          t('usage.cache_writes', { count: compact(total.cacheWriteTokens) }),
        ],
        [
          t('usage.output'),
          compact(total.outputTokens),
          t('usage.includes_reasoning', { count: compact(total.reasoningTokens) }),
        ],
        [
          t('usage.cache_savings'),
          data!.cacheSavingsUsd == null
            ? t('settings.usage_unavailable_value')
            : money(data!.cacheSavingsUsd),
          t('usage.vs_full_input_rates'),
        ],
      ]
    : [];
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        flexGrow: 1,
        minHeight: 0,
        width: '100%',
        maxWidth: 1088,
        alignSelf: 'center',
        paddingLeft: 0,
        paddingRight: 0,
        paddingTop: 6,
        paddingBottom: view === 'daily' ? 48 : 16,
        gap: 6,
        color: colors.text,
      }}
    >
      <div style={{ ...row, gap: 10 }}>
        <text style={{ ...text, ...ellipsis, flexGrow: 1 }}>
          {t('usage.range', {
            start: displayed
              ? view === 'monthly'
                ? parseDay(displayed.from).toLocaleDateString(locale, {
                    month: 'short',
                    year: 'numeric',
                  })
                : shortDay(displayed.from, locale)
              : shortDay(window.from, locale),
            end: displayed
              ? view === 'monthly'
                ? parseDay(displayed.to).toLocaleDateString(locale, {
                    month: 'short',
                    year: 'numeric',
                  })
                : shortDay(displayed.to, locale)
              : shortDay(window.to, locale),
          })}
        </text>
        <Segmented
          label={t('settings.usage_view')}
          value={view}
          onChange={setView}
          options={[
            { value: 'daily', label: t('usage.daily') },
            { value: 'monthly', label: t('usage.monthly') },
            { value: 'projects', label: t('usage.projects') },
          ]}
          style={segmentStyle}
          itemStyle={itemStyle}
        />
        {view !== 'monthly' && (
          <Dropdown
            label={t(ranges.find((r) => r.id === range)!.key)}
            outlined
            style={{
              width: 124,
              height: 28,
              justifyContent: 'space-between',
              backgroundColor: colors.surface,
            }}
            items={ranges.map((r) => ({
              ...r,
              label: t(r.key),
              selected: r.id === range,
              onSelect: () => setRange(r.id),
            }))}
          />
        )}
        <Tooltip content={t(pending ? 'usage.scanning' : 'usage.rescan')}>
          <Button
            aria-label={t('usage.rescan')}
            disabled={pending}
            onClick={() => setRefresh((v) => v + 1)}
            style={{ height: 28, width: 30, borderRadius: 7, borderColor: colors.border_strong }}
          >
            {pending ? <Spinner size={12} /> : <Icon name="rotate-cw" size={12} />}
          </Button>
        </Tooltip>
      </div>
      {data && (data.errors.length > 0 || !data.ratesAvailable) ? (
        <div
          style={{
            marginTop: 14,
            padding: 10,
            borderWidth: 1,
            borderColor: colors.border,
            borderRadius: 8,
            gap: 3,
          }}
        >
          {[...data.errors, ...(!data.ratesAvailable ? [t('usage.rates_unavailable')] : [])].map(
            (error, i) => (
              <text key={i} style={{ ...text, whiteSpace: 'normal' }}>
                {error}
              </text>
            ),
          )}
        </div>
      ) : null}
      {!data || !total ? (
        <LoadingUsage list={view !== 'daily'} />
      ) : view === 'daily' ? (
        <>
          <div
            style={{
              display: 'flex',
              flexDirection: 'row',
              flexWrap: 'wrap',
              gap: 28,
              marginTop: 14,
            }}
          >
            <div style={{ width: 300, display: 'flex', flexDirection: 'column', gap: 18 }}>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
                <text style={{ ...text, color: colors.text_tertiary }}>
                  {t(metric === 'cost' ? 'usage.raw_token_cost' : 'usage.processed_tokens_upper')}
                </text>
                <text style={{ fontSize: sp(30), fontWeight: 500 }}>
                  {metric === 'cost' ? `${dollars(total)}*` : compact(total.totalTokens)}
                </text>
                <text style={{ ...text, color: colors.text_tertiary }}>
                  {metric === 'cost'
                    ? t('usage.full_api_rate_note')
                    : t('usage.sessions_summary', { count: total.sessions })}
                </text>
              </div>
              {[...data.byProvider]
                .sort((a, b) => metricValue(b, metric) - metricValue(a, metric))
                .map((p) => (
                  <div key={p.key} style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                    <div style={{ ...row, gap: 8 }}>
                      <ProviderMark provider={p.key} />
                      <text style={{ ...text, ...ellipsis, color: colors.text, flexGrow: 1 }}>
                        {labels[p.key] ?? p.key}
                      </text>
                      <text style={text}>
                        {metric === 'cost' ? dollars(p) : compact(p.totalTokens)}
                      </text>
                    </div>
                    <Magnitude
                      value={metricValue(p, metric)}
                      max={metricValue(total, metric)}
                      provider={p.key}
                    />
                    <text style={{ ...text, color: colors.text_tertiary }}>
                      {metric === 'cost'
                        ? t('usage.cost_share', {
                            share: share(p.knownCostUsd, total.knownCostUsd),
                            tokens: compact(p.totalTokens),
                          })
                        : t('usage.token_share', {
                            share: share(p.totalTokens, total.totalTokens),
                            cost: dollars(p),
                          })}
                    </text>
                  </div>
                ))}
              {!data.byProvider.length && <text style={text}>{t('usage.no_activity_window')}</text>}
            </div>
            <div
              style={{
                flexGrow: 1,
                minWidth: 320,
                display: 'flex',
                flexDirection: 'column',
                gap: 10,
              }}
            >
              <div style={{ ...row, gap: 12 }}>
                <text style={{ ...text, color: colors.text, fontWeight: 500, flexGrow: 1 }}>
                  {t(metric === 'cost' ? 'usage.daily_cost' : 'usage.daily_processed_tokens')}
                </text>
                <Segmented
                  label={t('settings.usage_chart_metric')}
                  value={metric}
                  onChange={(v) => setMetric(v as Metric)}
                  options={[
                    { value: 'cost', label: t('usage.cost_upper') },
                    { value: 'tokens', label: t('usage.tokens_upper') },
                  ]}
                  style={segmentStyle}
                  itemStyle={{ ...itemStyle, height: 22, paddingLeft: 9, paddingRight: 9 }}
                />
                <div style={{ ...row, justifyContent: 'flex-end', gap: 14 }}>
                  {providers.map((p) => (
                    <div key={p} style={{ ...row, gap: 5 }}>
                      <ProviderMark provider={p} />
                      <text style={text}>{labels[p] ?? p}</text>
                    </div>
                  ))}
                </div>
              </div>
              <DailyChart data={data} metric={metric} />
            </div>
          </div>
          <div
            style={{
              ...row,
              marginTop: 18,
              borderTopWidth: 1,
              borderBottomWidth: 1,
              borderColor: colors.border,
            }}
          >
            {sections.map(([label, value, caption], i) => (
              <div
                key={label}
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  flexGrow: 1,
                  flexBasis: 0,
                  minWidth: 0,
                  paddingLeft: 14,
                  paddingRight: 14,
                  paddingTop: 11,
                  paddingBottom: 11,
                  borderLeftWidth: i ? 1 : 0,
                  borderColor: colors.border,
                  gap: 2,
                }}
              >
                <text style={{ ...text, ...ellipsis, color: colors.text_tertiary }}>{label}</text>
                <text style={{ ...ellipsis, fontSize: sp(15) }}>{value}</text>
                <text style={{ ...text, ...ellipsis, color: colors.text_tertiary }}>{caption}</text>
              </div>
            ))}
          </div>
          <div
            style={{
              display: 'flex',
              flexDirection: 'row',
              flexWrap: 'wrap',
              gap: 32,
              marginTop: 18,
            }}
          >
            <div style={{ flexGrow: 1, minWidth: 320 }}>
              <div style={{ ...row, marginBottom: 10 }}>
                <text style={{ ...text, color: colors.text, fontWeight: 500, flexGrow: 1 }}>
                  {t('usage.breakdown')}
                </text>
                <Segmented
                  label={t('settings.usage_breakdown_grouping')}
                  value={breakdown}
                  onChange={setBreakdown}
                  options={[
                    { value: 'model', label: t('usage.model_upper') },
                    { value: 'day', label: t('usage.day_upper') },
                  ]}
                  style={segmentStyle}
                  itemStyle={{ ...itemStyle, height: 22 }}
                />
              </div>
              <TableRow
                cells={
                  breakdown === 'model'
                    ? [t('usage.model'), t('usage.cost'), t('usage.share'), t('usage.tokens')]
                    : [
                        t('usage.day'),
                        ...providers.map((p) => labels[p] ?? p),
                        t('usage.total'),
                        t('usage.tokens'),
                      ]
                }
                heading
              />
              {(breakdown === 'model'
                ? sortedModels
                : [...data.byDay]
                    .filter((d) => d.totalTokens > 0)
                    .sort((a, b) => b.key.localeCompare(a.key))
                    .slice(0, 8)
              ).map((a) => (
                <TableRow
                  key={a.key}
                  cells={
                    breakdown === 'model'
                      ? [
                          a.model ?? a.key,
                          dollars(a),
                          share(a.knownCostUsd, total.knownCostUsd),
                          compact(a.totalTokens),
                        ]
                      : [
                          shortDay(a.key, locale),
                          ...providers.map((p) => dollarsForProvider(a, p, money)),
                          dollars(a),
                          compact(a.totalTokens),
                        ]
                  }
                />
              ))}
              {!sortedModels.length && (
                <text style={{ ...text, marginTop: 26, textAlign: 'center' }}>
                  {t('usage.no_activity_window')}
                </text>
              )}
            </div>
            <div style={{ width: 240 }}>
              <text style={{ ...text, color: colors.text, fontWeight: 500, marginBottom: 10 }}>
                {t('usage.cost_quality')}
              </text>
              {[
                [
                  t('usage.provider_reported'),
                  share(data.quality.providerReportedRecords, total.records),
                ],
                [t('usage.model_priced'), share(data.quality.modelPricedRecords, total.records)],
                [t('usage.unpriced'), share(data.quality.unpricedRecords, total.records)],
                [
                  t('usage.cache_savings'),
                  data.cacheSavingsUsd == null
                    ? t('settings.usage_unavailable_value')
                    : money(data.cacheSavingsUsd),
                ],
              ].map(([label, value]) => (
                <div
                  key={label}
                  style={{
                    ...row,
                    paddingTop: 8,
                    paddingBottom: 8,
                    borderBottomWidth: 1,
                    borderColor: colors.border,
                    gap: 12,
                  }}
                >
                  <text style={{ ...text, flexGrow: 1 }}>{label}</text>
                  <text style={{ ...text, color: colors.text }}>{value}</text>
                </div>
              ))}
            </div>
          </div>
        </>
      ) : (
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            flexGrow: 1,
            minHeight: 0,
            marginTop: 20,
            paddingBottom: 8,
            borderRadius: 13,
            backgroundColor: colors.raised,
            overflow: 'hidden',
          }}
        >
          <div
            style={{
              ...row,
              paddingLeft: 20,
              paddingRight: 20,
              paddingTop: 13,
              paddingBottom: 13,
              borderBottomWidth: 1,
              borderColor: colors.border,
              gap: 16,
            }}
          >
            <div style={{ flexGrow: 1, minWidth: 0, gap: 3 }}>
              <text style={{ fontSize: sp(13.5), fontWeight: 500 }}>
                {t(view === 'monthly' ? 'usage.last_12_months' : 'usage.by_project')}
              </text>
              <text style={{ ...text, ...ellipsis }}>
                {`${t('usage.tokens_and_sessions', { tokens: compact(total.totalTokens), sessions: t('usage.session_many', { count: total.sessions }) })}${view === 'projects' ? ` · ${query.trim() ? t('usage.projects_shown', { shown: projects.length, projects: groupedProjects.length }) : t('usage.project_many', { count: projects.length })}` : ''}`}
              </text>
            </div>
            {view === 'projects' ? (
              <TextField
                value={query}
                onChange={setQuery}
                placeholder={t('usage.filter_projects')}
                icon="search"
                style={{ width: 240 }}
              />
            ) : (
              <text style={{ fontSize: sp(15), fontWeight: 500 }}>{dollars(total)}</text>
            )}
          </div>
          {sourceRows.length ? (
            <virtual-list
              estimatedItemHeight={96}
              style={{ flexGrow: 1, minHeight: 0, overflow: 'scroll' }}
            >
              {sourceRows.map((a) => (
                <HistoryRow
                  key={a.project ?? a.key}
                  aggregate={a}
                  data={data}
                  monthly={view === 'monthly'}
                  rankingMetric={rankingMetric}
                  maxMagnitude={maxMagnitude}
                  home={home}
                />
              ))}
            </virtual-list>
          ) : (
            <text style={{ ...text, padding: 24 }}>
              {view === 'monthly'
                ? t('usage.no_activity_12_months')
                : query.trim()
                  ? t('usage.no_projects_match')
                  : t('usage.no_activity_window')}
            </text>
          )}
        </div>
      )}
      {data && (
        <text style={{ ...text, color: colors.text_tertiary, marginTop: 14 }}>
          {`${t('usage.scan_summary', { scanned: data.scan.scanned, skipped: data.scan.skipped, records: data.scan.records, seconds: (data.scan.elapsedMs / 1000).toFixed(1) })} · ${t('settings.usage_timezone_dates', { timeZone: data.timeZone })}`}
        </text>
      )}
    </div>
  );
}
function ProviderMark({ provider }: { provider: string }) {
  const { colors } = useTheme();
  return (
    <Icon
      name={
        /claude|anthropic/i.test(provider)
          ? 'provider-claude'
          : /codex|openai/i.test(provider)
            ? 'provider-openai'
            : 'package'
      }
      size={12}
      color={providerColor(provider, colors)}
    />
  );
}
function dollarsForProvider(
  aggregate: Aggregate,
  provider: string,
  money: (value: number) => string,
) {
  const matching = aggregate.providers.filter((p) => providerFamily(p.key) === provider);
  return `${money(matching.reduce((sum, p) => sum + p.knownCostUsd, 0))}${matching.some((p) => p.unpricedRecords > 0) ? '+' : ''}`;
}
function Magnitude({
  value,
  max,
  provider,
  aggregate,
}: {
  value: number;
  max: number;
  provider?: string;
  aggregate?: Aggregate;
}) {
  const { colors } = useTheme();
  return (
    <div
      style={{
        height: 4,
        borderRadius: 4,
        backgroundColor: colors.overlay_strong,
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          ...row,
          height: 4,
          width: `${max > 0 && value > 0 ? Math.max(2, Math.min(100, (value / max) * 100)) : 0}%`,
          backgroundColor: providerColor(provider ?? '', colors),
        }}
      >
        {aggregate?.providers?.map((p) => (
          <div
            key={p.key}
            style={{
              height: 4,
              width: `${share(aggregate.knownCostUsd > 0 ? p.knownCostUsd : p.totalTokens, aggregate.knownCostUsd > 0 ? aggregate.knownCostUsd : aggregate.totalTokens)}`,
              backgroundColor: providerColor(p.key, colors),
            }}
          />
        ))}
      </div>
    </div>
  );
}
function TableRow({ cells, heading = false }: { cells: string[]; heading?: boolean }) {
  const { colors, sp } = useTheme();
  return (
    <div
      style={{
        ...row,
        gap: 12,
        paddingTop: heading ? 0 : 8,
        paddingBottom: heading ? 7 : 8,
        borderBottomWidth: 1,
        borderColor: colors.border,
      }}
    >
      {cells.map((cell, i) => (
        <text
          key={i}
          style={{
            ...ellipsis,
            flexGrow: i ? 0 : 1,
            width: i ? (i === cells.length - 2 && cells.length === 4 ? 64 : 84) : undefined,
            textAlign: i ? 'right' : 'left',
            fontSize: sp(12.5),
            color: heading || i > 1 ? colors.text_tertiary : colors.text,
          }}
        >
          {cell}
        </text>
      ))}
    </div>
  );
}
function ModelMenu({ models }: { models: Aggregate[] }) {
  const { colors, sp } = useTheme();
  const { dollars } = useUsageFormats();
  const sorted = [...models].sort(
    (a, b) => b.knownCostUsd - a.knownCostUsd || b.totalTokens - a.totalTokens,
  );
  const total = sorted.reduce((sum, model) => sum + model.knownCostUsd, 0);
  return !sorted.length ? null : (
    <Dropdown
      label={`${sorted
        .slice(0, 2)
        .map((m) => m.model ?? m.key)
        .join(' · ')}${sorted.length > 2 ? ` · +${sorted.length - 2}` : ''}`}
      aria-label={t('usage.show_models', {
        models: t('usage.model_many', { count: sorted.length }),
      })}
      style={{ maxWidth: 300, height: 22, paddingLeft: 6, paddingRight: 6, borderRadius: 5 }}
      menuStyle={{ width: 288, maxHeight: 320 }}
      items={[
        {
          id: 'caption',
          type: 'caption',
          label:
            total > 0
              ? t('usage.models_by_spend', {
                  models: t('usage.model_many', { count: sorted.length }),
                })
              : t('usage.model_many', { count: sorted.length }),
        },
        ...sorted.map((m, i) => ({
          id: `${m.key}-${i}`,
          type: 'custom' as const,
          content: (
            <div
              style={{
                ...row,
                minHeight: 34,
                paddingTop: 6,
                paddingBottom: 6,
                gap: 14,
                borderTopWidth: i ? 1 : 0,
                borderColor: colors.border,
              }}
            >
              <text
                style={{
                  ...ellipsis,
                  flexGrow: 1,
                  fontSize: sp(12.5),
                  color: colors.text_secondary,
                }}
              >
                {m.model ?? m.key}
              </text>
              {total > 0 && (
                <div style={{ alignItems: 'flex-end' }}>
                  <text style={{ fontSize: sp(12.5), color: colors.text }}>{dollars(m)}</text>
                  <text style={{ fontSize: sp(12.5), color: colors.text_ghost }}>
                    {share(m.knownCostUsd, total)}
                  </text>
                </div>
              )}
            </div>
          ),
        })),
      ]}
    />
  );
}
function LoadingUsage({ list }: { list: boolean }) {
  const { colors } = useTheme();
  return (
    <Pulse
      style={{
        marginTop: 20,
        display: 'flex',
        flexDirection: list ? 'column' : 'row',
        gap: 28,
        padding: list ? 20 : 0,
        backgroundColor: list ? colors.raised : undefined,
        borderRadius: 13,
      }}
    >
      {[0, 1, 2, 3].slice(0, list ? 4 : 2).map((i) => (
        <div key={i} style={{ flexGrow: 1, display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div
            style={{
              width: 96,
              height: 9,
              backgroundColor: colors.overlay_strong,
              borderRadius: 5,
            }}
          />
          <div
            style={{
              width: list ? '100%' : 150,
              height: list ? 38 : i ? 224 : 26,
              backgroundColor: colors.overlay_strong,
              borderRadius: 8,
            }}
          />
          <div
            style={{
              width: 180,
              height: 9,
              backgroundColor: colors.overlay_strong,
              borderRadius: 5,
            }}
          />
        </div>
      ))}
    </Pulse>
  );
}

function DailyChart({ data, metric }: { data: Snapshot; metric: Metric }) {
  const { colors, sp } = useTheme();
  const locale = useLocale();
  const { compact, money } = useUsageFormats();
  const { renderer } = useGpuix();
  const plot = useRef<PublicInstance>(null);
  const providers = useMemo(
    () => [...new Set(['claude', 'codex', ...data.byProvider.map((p) => providerFamily(p.key))])],
    [data],
  );
  const [width, setWidth] = useState(400);
  const [hover, setHover] = useState<number>();
  const days = useMemo(() => {
    const result: { key: string; values: number[] }[] = [];
    for (const day = parseDay(data.from); iso(day) <= data.to; day.setDate(day.getDate() + 1)) {
      const key = iso(day),
        aggregate = data.byDay.find((d) => d.key === key);
      result.push({
        key,
        values: providers.map((p) =>
          (aggregate?.providers ?? [])
            .filter((a) => providerFamily(a.key) === p)
            .reduce((sum, a) => sum + metricValue(a, metric), 0),
        ),
      });
    }
    return result;
  }, [data, metric, providers]);
  const peak = Math.max(0, ...days.flatMap((d) => d.values));
  const rawStep = peak / 4;
  const exponent = rawStep ? 10 ** Math.floor(Math.log10(rawStep)) : 1;
  const step = ([1, 2, 5, 10].find((n) => n * exponent >= rawStep) ?? 10) * exponent;
  const maximum = peak ? Math.ceil(peak / step) * step : 1;
  const ticks = peak
    ? Array.from({ length: Math.round(maximum / step) + 1 }, (_, i) => i * step)
    : [0];
  const x = (i: number) => (days.length > 1 ? (i / (days.length - 1)) * width : width / 2);
  const y = (value: number) => 223 - (value / maximum) * 215;
  const paths = providers.map((p, provider) => {
    const points = days.map((d, i) => [x(i), y(d.values[provider]!)]) as [number, number][];
    const line = points
      .map(([px, py], i) =>
        i === 0
          ? `M ${px} ${py}`
          : `C ${(points[i - 1]![0] + px) / 2} ${points[i - 1]![1]} ${(points[i - 1]![0] + px) / 2} ${py} ${px} ${py}`,
      )
      .join(' ');
    return { p, line, total: days.reduce((sum, d) => sum + d.values[provider]!, 0) };
  });
  const source = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="224" viewBox="0 0 ${width} 224">${ticks.map((t) => `<path d="M 0 ${y(t)} H ${width}" stroke="${colors.border}" fill="none"/>`).join('')}${[
    ...paths,
  ]
    .sort((a, b) => b.total - a.total)
    .map(
      (p) =>
        `<path d="${p.line} L ${days.length ? x(days.length - 1) : width} 224 L ${days.length ? x(0) : 0} 224 Z" fill="${providerColor(p.p, colors)}" fill-opacity="0.12"/>`,
    )
    .join(
      '',
    )}${paths.map((p) => `<path d="${p.line}" fill="none" stroke="${providerColor(p.p, colors)}" stroke-width="2"/>`).join('')}${hover === undefined ? '' : `<path d="M ${x(hover)} 8 V 224" stroke="${colors.text_ghost}"/>`}</svg>`;
  const active = hover === undefined ? undefined : days[hover];
  return (
    <div
      tabIndex={0}
      role="group"
      aria-label={t('settings.usage_chart_accessibility', {
        metric: t(
          metric === 'cost' ? 'settings.usage_cost_metric' : 'settings.usage_tokens_metric',
        ),
      })}
      onKeyDown={(e) => {
        const key = e.key;
        if (!['left', 'right', 'home', 'end', 'escape'].includes(key ?? '')) return;
        e.preventDefault();
        e.stopPropagation();
        if (key === 'escape') setHover(undefined);
        else
          setHover((old) =>
            key === 'home'
              ? 0
              : key === 'end'
                ? days.length - 1
                : key === 'left'
                  ? Math.max(0, old === undefined ? days.length - 1 : old - 1)
                  : Math.min(days.length - 1, old === undefined ? 0 : old + 1),
          );
      }}
      onMouseLeave={() => setHover(undefined)}
      style={{
        borderWidth: 1,
        borderColor: 'transparent',
        focusVisible: { borderColor: colors.accent },
      }}
    >
      <div style={{ ...row, alignItems: 'stretch', gap: 8 }}>
        <div style={{ width: 56, height: 224, position: 'relative' }}>
          {ticks.map((t) => (
            <text
              key={t}
              style={{
                position: 'absolute',
                right: 1,
                top: Math.max(0, y(t) - 7),
                fontSize: sp(12.5),
                color: colors.text_tertiary,
              }}
            >
              {t === 0 ? '0' : metric === 'cost' ? money(t) : compact(t)}
            </text>
          ))}
        </div>
        <div
          ref={plot}
          onLayout={(e) => {
            if (e.width) setWidth(e.width);
          }}
          onMouseMove={(e) => {
            const b = plot.current && renderer?.getElementBounds?.(plot.current.id);
            if (b && e.x !== undefined)
              setHover(
                Math.max(
                  0,
                  Math.min(
                    days.length - 1,
                    Math.round(((e.x - b.x) / b.width) * (days.length - 1)),
                  ),
                ),
              );
          }}
          style={{ flexGrow: 1, height: 224, minWidth: 0, position: 'relative' }}
        >
          <svg source={source} style={{ width: '100%', height: 224 }} />
          {active && (
            <div
              style={{
                position: 'absolute',
                top: 0,
                left:
                  hover! / Math.max(1, days.length - 1) <= 0.6
                    ? Math.max(0, Math.min(width - 160, x(hover!)))
                    : undefined,
                right:
                  hover! / Math.max(1, days.length - 1) > 0.6
                    ? Math.max(0, width - x(hover!))
                    : undefined,
                minWidth: 150,
                paddingLeft: 9,
                paddingRight: 9,
                paddingTop: 7,
                paddingBottom: 7,
                borderRadius: 8,
                borderWidth: 1,
                borderColor: colors.border_strong,
                backgroundColor: colors.raised,
                gap: 3,
                pointerEvents: 'none',
                boxShadow: {
                  offsetX: 0,
                  offsetY: 6,
                  blurRadius: 12,
                  spreadRadius: 0,
                  color: '#00000022',
                },
              }}
            >
              <text style={{ fontSize: sp(12.5), color: colors.text_tertiary }}>
                {shortDay(active.key, locale)}
              </text>
              {providers.map((p, i) => (
                <div key={p} style={{ ...row, gap: 10 }}>
                  <ProviderMark provider={p} />
                  <text style={{ fontSize: sp(12.5), color: colors.text_secondary, flexGrow: 1 }}>
                    {labels[p] ?? p}
                  </text>
                  <text style={{ fontSize: sp(12.5), color: colors.text }}>
                    {metric === 'cost' ? money(active.values[i]!) : compact(active.values[i]!)}
                  </text>
                </div>
              ))}
              <div
                style={{
                  ...row,
                  borderTopWidth: 1,
                  borderColor: colors.border,
                  paddingTop: 4,
                  marginTop: 2,
                }}
              >
                <text style={{ flexGrow: 1, fontSize: sp(12.5) }}>{t('usage.total')}</text>
                <text style={{ fontSize: sp(12.5) }}>
                  {metric === 'cost'
                    ? money(active.values.reduce((a, b) => a + b, 0))
                    : compact(active.values.reduce((a, b) => a + b, 0))}
                </text>
              </div>
            </div>
          )}
        </div>
      </div>
      <div style={{ ...row, justifyContent: 'space-between', paddingLeft: 64, marginTop: 8 }}>
        {[0, Math.floor(days.length / 2), days.length - 1].map((i, n) => (
          <text key={n} style={{ fontSize: sp(12.5), color: colors.text_tertiary }}>
            {days[i] ? shortDay(days[i]!.key, locale) : ''}
          </text>
        ))}
      </div>
    </div>
  );
}

function HistoryRow({
  aggregate: a,
  data,
  monthly,
  rankingMetric,
  maxMagnitude,
  home,
}: {
  aggregate: Aggregate;
  data: Snapshot;
  monthly: boolean;
  rankingMetric: Metric;
  maxMagnitude: number;
  home?: string;
}) {
  const { colors, sp } = useTheme();
  const locale = useLocale();
  const { compact, dollars } = useUsageFormats();
  const text = { fontSize: sp(12.5), color: colors.text_tertiary };
  const title = monthly
    ? parseDay(`${a.key.slice(0, 7)}-01`).toLocaleDateString(locale, {
        month: 'long',
        year: 'numeric',
      })
    : a.key;
  const path =
    a.project && home && (a.project === home || a.project.startsWith(`${home}/`))
      ? `~${a.project.slice(home.length)}`
      : a.project;
  const activeDays = data.byDay.filter(
    (day) => day.key.startsWith(a.key.slice(0, 7)) && day.totalTokens > 0,
  ).length;
  return (
    <div
      style={{
        paddingLeft: 20,
        paddingRight: 20,
        paddingTop: a.records ? 13 : 11,
        paddingBottom: a.records ? 13 : 11,
        borderBottomWidth: 1,
        borderColor: colors.border,
        display: 'flex',
        flexDirection: 'column',
        gap: 2,
      }}
    >
      <div style={{ ...row, gap: 8 }}>
        <text style={{ ...ellipsis, fontSize: sp(13), fontWeight: 500 }}>{title}</text>
        {monthly && a.key.slice(0, 7) === iso(new Date()).slice(0, 7) && (
          <text style={{ ...text, color: colors.text_ghost }}>{t('usage.so_far')}</text>
        )}
        {!monthly && path && (
          <text style={{ ...text, ...ellipsis, color: colors.text_ghost }}>{path}</text>
        )}
        <div style={{ flexGrow: 1 }} />
        <text style={{ fontSize: sp(14), fontWeight: 500 }}>{a.records ? dollars(a) : '—'}</text>
      </div>
      {a.records ? (
        <>
          <div style={{ ...row, gap: 12 }}>
            <text style={{ ...text, ...ellipsis, flexGrow: 1 }}>
              {`${!monthly && data.totals.knownCostUsd > 0 && a.knownCostUsd > 0 ? `${t('usage.percent_of_cost', { share: share(a.knownCostUsd, data.totals.knownCostUsd) })} · ` : ''}${t('usage.tokens_and_sessions', { tokens: compact(a.totalTokens), sessions: t('usage.session_many', { count: a.sessions }) })}${monthly ? ` · ${t('usage.active_day_many', { count: activeDays })}` : a.lastActive ? ` · ${t('usage.last_active', { date: shortDay(a.lastActive, locale) })}` : ''}`}
            </text>
            {monthly && <MonthChart month={a.key} data={data} />}
          </div>
          <div style={{ marginTop: 7 }}>
            <Magnitude value={metricValue(a, rankingMetric)} max={maxMagnitude} aggregate={a} />
          </div>
          <div style={{ ...row, gap: 14, marginTop: 7 }}>
            {a.providers.map((p) => (
              <div key={p.key} style={{ ...row, gap: 5 }}>
                <ProviderMark provider={p.key} />
                <text style={{ ...text, color: colors.text_secondary }}>{dollars(p)}</text>
              </div>
            ))}
            <div style={{ flexGrow: 1 }} />
            <ModelMenu models={a.models} />
          </div>
        </>
      ) : (
        <text style={text}>{t('usage.no_activity')}</text>
      )}
    </div>
  );
}

function MonthChart({ month, data }: { month: string; data: Snapshot }) {
  const { colors } = useTheme();
  const days = new Date(Number(month.slice(0, 4)), Number(month.slice(5, 7)), 0).getDate();
  const unit: Metric = data.totals.knownCostUsd > 0 ? 'cost' : 'tokens';
  const peak = Math.max(0, ...data.byDay.map((day) => metricValue(day, unit)));
  const barWidth = Math.max(1, (168 - days + 1) / days);
  const bands: string[] = [];
  for (let day = 1; day <= days; day++) {
    const key = `${month.slice(0, 7)}-${String(day).padStart(2, '0')}`;
    const aggregate = data.byDay.find((row) => row.key === key);
    let bottom = 20;
    for (const provider of aggregate?.providers ?? []) {
      const value = metricValue(provider, unit);
      if (value <= 0 || peak <= 0) continue;
      const height = Math.max(1.5, (value / peak) * 20);
      bottom -= height;
      bands.push(
        `<rect x="${(day - 1) * (barWidth + 1)}" y="${Math.max(0, bottom)}" width="${barWidth}" height="${height}" fill="${providerColor(provider.key, colors)}"/>`,
      );
    }
  }
  return (
    <svg
      source={`<svg xmlns="http://www.w3.org/2000/svg" width="168" height="20" viewBox="0 0 168 20">${bands.join('')}</svg>`}
      style={{ width: 168, height: 20, flexShrink: 0 }}
    />
  );
}

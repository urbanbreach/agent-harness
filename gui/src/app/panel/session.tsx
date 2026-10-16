import { useEffect, useState, type ReactNode } from 'react';
import { t, useLocale } from '../../i18n';
import type { SessionInfo, PlanEntry } from '../../backend/protocol';
import { getAppStore, useSessionView } from '../../store';
import { useTheme } from '../../theme';
import { Markdown } from '../../markdown';
import { Button, Icon, row, ellipsis } from '../../ui';
import { EmptyMessage, Toolbar } from './shared';
function Section({ title, children }: { title: string; children: ReactNode }) {
  const { colors, sp } = useTheme();
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 6, paddingBottom: 20 }}>
      <text
        style={{
          color: colors.text_tertiary,
          fontSize: sp(12.5),
          fontWeight: 500,
          marginBottom: 4,
        }}
      >
        {title}
      </text>
      {children}
    </div>
  );
}
function Detail({ label, value }: { label: string; value: string }) {
  const { colors, sp } = useTheme();
  return (
    <div style={{ ...row, gap: 12, minHeight: 28 }}>
      <text style={{ ...ellipsis, flexGrow: 1, fontSize: sp(12.5), color: colors.text_secondary }}>
        {label}
      </text>
      <text
        style={{
          maxWidth: '65%',
          fontSize: sp(12.5),
          color: colors.text_tertiary,
          textAlign: 'right',
        }}
      >
        {value}
      </text>
    </div>
  );
}
export function SessionSurface({ sessionId }: { sessionId: string | null }) {
  useLocale();
  const [info, setInfo] = useState<SessionInfo>();
  const [error, setError] = useState<string>();
  const view = useSessionView(sessionId ?? undefined);
  const { colors, sp } = useTheme();
  const modified = new Map<string, { additions?: number; deletions?: number }>();
  for (const tool of Object.values(view?.tools ?? {})) {
    if (tool.status !== 'succeeded' || tool.presentation.output?.kind !== 'diff') continue;
    for (const file of tool.presentation.output.files) {
      const previous = modified.get(file.path);
      modified.set(file.path, {
        additions:
          file.additions === undefined
            ? previous?.additions
            : (previous?.additions ?? 0) + file.additions,
        deletions:
          file.deletions === undefined
            ? previous?.deletions
            : (previous?.deletions ?? 0) + file.deletions,
      });
    }
  }
  for (const event of view?.events ?? []) {
    if (event.payload.event_type === 'edit_applied' && !modified.has(event.payload.data.path)) {
      modified.set(event.payload.data.path, {});
    }
  }
  useEffect(() => {
    let active = true;
    let generation = 0;
    setInfo(undefined);
    setError(undefined);
    const refresh = () => {
      if (!sessionId) return;
      const request = ++generation;
      void getAppStore()
        .backend.call('session.info', { sessionId })
        .then((next) => {
          if (!active || request !== generation) return;
          setInfo(next);
          setError(undefined);
        })
        .catch((failure) => {
          if (active && request === generation) setError(String(failure));
        });
    };
    refresh();
    const timer = setInterval(refresh, 5000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [sessionId]);
  if (!sessionId) return <EmptyMessage title={t('panel.no_session')} />;
  if (error) return <EmptyMessage title={t('panel.session_unavailable')} description={error} />;
  if (!info) return <EmptyMessage title={t('panel.loading_session')} />;
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        flexGrow: 1,
        minHeight: 0,
        overflowY: 'scroll',
        padding: 20,
      }}
    >
      <Section title={t('panel.mcp_servers')}>
        {info.mcp.length ? (
          info.mcp.map((server) => (
            <Detail
              key={server.name}
              label={server.name}
              value={server.enabled ? server.status : t('panel.disabled')}
            />
          ))
        ) : (
          <Detail label={t('panel.no_mcp_servers')} value="" />
        )}
        <text style={{ fontSize: sp(12.5), color: colors.text_tertiary }}>
          {info.integrationStatusReason}
        </text>
      </Section>
      <Section title={t('panel.lsp_servers')}>
        {info.lsp.length ? (
          info.lsp.map((server) => (
            <Detail key={server.name} label={server.name} value={server.status} />
          ))
        ) : (
          <Detail label={t('panel.no_lsp_servers')} value="" />
        )}
      </Section>
      <Section title={t('panel.modified_files')}>
        {modified.size ? (
          [...modified].map(([path, counts]) => (
            <Detail
              key={path}
              label={path}
              value={
                counts.additions === undefined && counts.deletions === undefined
                  ? t('panel.modified')
                  : `+${counts.additions ?? 0} −${counts.deletions ?? 0}`
              }
            />
          ))
        ) : (
          <Detail label={t('panel.no_modified_files')} value="" />
        )}
      </Section>
      <Section title={t('panel.runtime')}>
        <Detail label={t('panel.model')} value={info.model ?? t('panel.not_selected')} />
        <Detail label={t('panel.provider')} value={info.provider ?? t('panel.not_selected')} />
        <Detail label={t('panel.profile')} value={info.profile} />
        <Detail label={t('panel.sandbox')} value={info.sandbox.status} />
        <text style={{ fontSize: sp(12.5), color: colors.text_tertiary }}>
          {info.sandbox.reason}
        </text>
      </Section>
    </div>
  );
}
export function TodosSurface({ sessionId }: { sessionId: string | null }) {
  useLocale();
  const view = useSessionView(sessionId ?? undefined);
  const { colors, sp } = useTheme();
  if (!view?.todos.length)
    return (
      <EmptyMessage title={t('panel.no_todos')} description={t('panel.todos_empty_description')} />
    );
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        flexGrow: 1,
        minHeight: 0,
        overflowY: 'scroll',
        padding: 16,
        gap: 6,
      }}
    >
      {view.todos.map((todo, index) => (
        <div
          key={index}
          style={{
            ...row,
            alignItems: 'flex-start',
            gap: 10,
            minHeight: 30,
            paddingTop: 6,
            paddingBottom: 6,
          }}
        >
          <Icon
            name={
              todo.status === 'completed'
                ? 'check'
                : todo.status === 'in_progress'
                  ? 'loader-circle'
                  : 'block'
            }
            size={14}
            color={
              todo.status === 'completed'
                ? colors.success
                : todo.status === 'in_progress'
                  ? colors.accent
                  : colors.text_ghost
            }
          />
          <text
            style={{
              color:
                todo.status === 'completed' || todo.status === 'cancelled'
                  ? colors.text_tertiary
                  : colors.text,
              fontSize: sp(12.5),
              lineHeight: sp(18),
            }}
          >
            {todo.content}
          </text>
        </div>
      ))}
    </div>
  );
}
export function PlanSurface({ cwd }: { cwd: string }) {
  useLocale();
  const { colors, sp } = useTheme();
  const [plans, setPlans] = useState<PlanEntry[]>([]);
  const [selected, setSelected] = useState<string>();
  const [content, setContent] = useState<string>();
  const [error, setError] = useState<string>();
  const [listError, setListError] = useState<string>();
  useEffect(() => {
    let active = true;
    let generation = 0;
    setPlans([]);
    setSelected(undefined);
    setListError(undefined);
    const refresh = () => {
      const request = ++generation;
      void getAppStore()
        .backend.call('plans.list', { cwd })
        .then(({ plans: next }) => {
          if (!active || request !== generation) return;
          setPlans(next);
          setListError(undefined);
          setSelected((current) =>
            next.some((plan) => plan.path === current && plan.exists)
              ? current
              : (next.find((plan) => plan.is_active && plan.exists)?.path ??
                next.find((plan) => plan.exists)?.path),
          );
        })
        .catch((failure) => {
          if (active && request === generation) setListError(String(failure));
        });
    };
    refresh();
    const timer = setInterval(refresh, 5000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [cwd]);
  useEffect(() => {
    let active = true;
    let generation = 0;
    setContent(undefined);
    setError(undefined);
    if (!selected) return;
    const refresh = () => {
      const request = ++generation;
      void getAppStore()
        .backend.call('plans.read', { cwd, path: selected })
        .then((result) => {
          if (!active || request !== generation) return;
          setContent(result.content);
          setError(undefined);
        })
        .catch((failure) => {
          if (active && request === generation) setError(String(failure));
        });
    };
    refresh();
    const timer = setInterval(refresh, 5000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [cwd, selected]);
  if (listError || error)
    return <EmptyMessage title={t('panel.plan_unavailable')} description={listError || error} />;
  if (!selected)
    return (
      <EmptyMessage title={t('panel.no_plan')} description={t('panel.plans_empty_description')} />
    );
  return (
    <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minHeight: 0 }}>
      <Toolbar>
        <Icon name="file" />
        <text style={{ ...ellipsis, color: colors.text_secondary, fontSize: sp(12.5) }}>
          {selected.split('/').pop()}
        </text>
      </Toolbar>
      {plans.length > 1 && (
        <div style={{ ...row, flexWrap: 'wrap', gap: 4, padding: 8 }}>
          {plans
            .filter((plan) => plan.exists)
            .map((plan) => (
              <Button
                key={plan.path}
                onClick={() => setSelected(plan.path)}
                style={{
                  padding: 6,
                  borderRadius: 6,
                  backgroundColor: selected === plan.path ? colors.overlay_strong : undefined,
                }}
              >
                <text style={{ fontSize: sp(12.5) }}>{plan.slug}</text>
              </Button>
            ))}
        </div>
      )}
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          flexGrow: 1,
          minHeight: 0,
          overflowY: 'scroll',
          paddingLeft: 16,
          paddingRight: 16,
          paddingTop: 14,
          paddingBottom: 24,
        }}
      >
        {content === undefined ? (
          <text style={{ color: colors.text_tertiary }}>{t('panel.loading_plan')}</text>
        ) : (
          <Markdown source={content} />
        )}
      </div>
    </div>
  );
}

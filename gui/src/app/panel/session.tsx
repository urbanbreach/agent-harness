import { useEffect, useState, type ReactNode } from 'react';
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
    setInfo(undefined);
    setError(undefined);
    const refresh = () => {
      if (sessionId)
        void getAppStore()
          .backend.call('session.info', { sessionId })
          .then((next) => {
            if (active) setInfo(next);
          })
          .catch((failure) => {
            if (active) setError(String(failure));
          });
    };
    refresh();
    const timer = setInterval(refresh, 5000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [sessionId]);
  if (!sessionId) return <EmptyMessage title="No session selected" />;
  if (error) return <EmptyMessage title="Session unavailable" description={error} />;
  if (!info) return <EmptyMessage title="Loading session…" />;
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
      <Section title="MCP servers">
        {info.mcp.length ? (
          info.mcp.map((server) => (
            <Detail
              key={server.name}
              label={server.name}
              value={server.enabled ? server.status : 'Disabled'}
            />
          ))
        ) : (
          <Detail label="No MCP servers" value="" />
        )}
        <text style={{ fontSize: sp(12.5), color: colors.text_tertiary }}>
          {info.integrationStatusReason}
        </text>
      </Section>
      <Section title="LSP servers">
        {info.lsp.length ? (
          info.lsp.map((server) => (
            <Detail key={server.name} label={server.name} value={server.status} />
          ))
        ) : (
          <Detail label="No LSP servers" value="" />
        )}
      </Section>
      <Section title="Modified files">
        {modified.size ? (
          [...modified].map(([path, counts]) => (
            <Detail
              key={path}
              label={path}
              value={
                counts.additions === undefined && counts.deletions === undefined
                  ? 'Modified'
                  : `+${counts.additions ?? 0} −${counts.deletions ?? 0}`
              }
            />
          ))
        ) : (
          <Detail label="No modified files" value="" />
        )}
      </Section>
      <Section title="Runtime">
        <Detail label="Model" value={info.model ?? 'Not selected'} />
        <Detail label="Provider" value={info.provider ?? 'Not selected'} />
        <Detail label="Profile" value={info.profile} />
        <Detail label="Sandbox" value={info.sandbox.status} />
        <text style={{ fontSize: sp(12.5), color: colors.text_tertiary }}>
          {info.sandbox.reason}
        </text>
      </Section>
    </div>
  );
}
export function TodosSurface({ sessionId }: { sessionId: string | null }) {
  const view = useSessionView(sessionId ?? undefined);
  const { colors, sp } = useTheme();
  if (!view?.todos.length)
    return <EmptyMessage title="No todos" description="The session checklist will appear here" />;
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
  const { colors, sp } = useTheme();
  const [plans, setPlans] = useState<PlanEntry[]>([]);
  const [selected, setSelected] = useState<string>();
  const [content, setContent] = useState<string>();
  const [error, setError] = useState<string>();
  useEffect(() => {
    let active = true;
    void getAppStore()
      .backend.call('plans.list', { cwd })
      .then(({ plans: next }) => {
        if (active) {
          setPlans(next);
          setSelected(
            next.find((plan) => plan.is_active)?.path ?? next.find((plan) => plan.exists)?.path,
          );
        }
      })
      .catch((failure) => {
        if (active) setError(String(failure));
      });
    return () => {
      active = false;
    };
  }, [cwd]);
  useEffect(() => {
    let active = true;
    if (selected) {
      setContent(undefined);
      setError(undefined);
      void getAppStore()
        .backend.call('plans.read', { cwd, path: selected })
        .then((result) => {
          if (active) setContent(result.content);
        })
        .catch((failure) => {
          if (active) setError(String(failure));
        });
    }
    return () => {
      active = false;
    };
  }, [cwd, selected]);
  if (error) return <EmptyMessage title="Plan unavailable" description={error} />;
  if (!selected)
    return <EmptyMessage title="No plan" description="Workspace plans will appear here" />;
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
          <text style={{ color: colors.text_tertiary }}>Loading plan…</text>
        ) : (
          <Markdown source={content} />
        )}
      </div>
    </div>
  );
}

import { taskStateLabel } from '../../model/tools';
import { t, useLocale } from '../../i18n';
import { useState } from 'react';
import { useEffect } from 'react';
import type { Intent } from '../../backend/protocol';
import { registerAction } from '../actions';
import { useTheme } from '../../theme';
import { getAppStore } from '../../store';
import { Button, Icon, IconButton } from '../../ui';
import type { SessionView, TaskView } from '../../model/session';

function childSession(task: TaskView): string | undefined {
  if (!task.lineage || typeof task.lineage !== 'object') return;
  const lineage = task.lineage as Record<string, unknown>;
  const route = lineage.display_route as Record<string, unknown> | undefined;
  const id = lineage.child_session_id ?? route?.child_session_id;
  return typeof id === 'string' ? id : undefined;
}
export function BackgroundWork({ sessionId, view }: { sessionId: string; view: SessionView }) {
  const locale = useLocale();
  const { colors, sp, codeFontSize } = useTheme();
  const [expanded, setExpanded] = useState<string>();
  const [error, setError] = useState('');
  const rows = [
    ...Object.values(view.subagents)
      .filter((task) => task.background || childSession(task))
      .map((task) => ({ task, child: true })),
    ...Object.values(view.tasks)
      .filter(
        (task) =>
          task.scope !== 'tool_call' &&
          task.scope !== 'agent_turn' &&
          !view.tools[task.id] &&
          !view.subagents[childSession(task) ?? ''] &&
          !view.entries.some((entry) => entry.kind === 'user' && entry.requestId === task.id),
      )
      .map((task) => ({ task, child: !!childSession(task) })),
  ];
  const active = (task: TaskView) =>
    !['completed', 'failed', 'cancelled', 'succeeded', 'done', 'finished', 'stopped'].includes(
      task.state,
    );
  const send = (intent: Intent) => {
    void getAppStore()
      .sendIntent(sessionId, intent)
      .catch((cause) => setError(String(cause)));
  };
  async function openChild(task: TaskView) {
    const id = childSession(task);
    if (!id || !view.workspaceRoot) return;
    try {
      const sessions = await getAppStore().loadSessions(view.workspaceRoot);
      const row = sessions.find((row) => row.runId === id);
      if (!row) throw new Error(t('transcript.child_unavailable'));
      await getAppStore().selectTask(row.runId);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }
  useEffect(() => {
    const stops = rows.map(({ task, child }) =>
      registerAction({
        id: `transcript.background:${task.id}`,
        label: t('transcript.background'),
        run: () => {
          if (child) void openChild(task);
          else {
            setExpanded(task.id);
            send({ kind: 'inspectCommand', taskId: task.id });
          }
        },
      }),
    );
    return () => stops.forEach((stop) => stop());
  }, [sessionId, view.tasks, view.subagents, locale]);
  if (!rows.length) return null;
  return (
    <div
      style={{
        marginTop: 8,
        borderRadius: 9,
        borderWidth: 1,
        borderColor: colors.border_strong,
        backgroundColor: colors.overlay,
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'center',
          paddingLeft: 8,
          paddingRight: 8,
          minHeight: 30,
          gap: 8,
        }}
      >
        <Icon name="bot" size={12} />
        <text style={{ flexGrow: 1, color: colors.text_secondary, fontSize: sp(12.5) }}>
          {t('transcript.background')}
        </text>
        {view.status === 'working' && (
          <Button
            onClick={() => send({ kind: 'backgroundForeground' })}
            style={{ height: 24, paddingLeft: 6, paddingRight: 6 }}
          >
            <text style={{ color: colors.text_secondary, fontSize: sp(12.5) }}>
              {t('transcript.run_background')}
            </text>
          </Button>
        )}
      </div>
      {rows.map(({ task, child }) => (
        <div
          key={`${child}:${task.id}`}
          style={{ borderTopWidth: 1, borderColor: colors.border, padding: 8, gap: 5 }}
        >
          <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 8 }}>
            <Button
              disabled={child && !childSession(task)}
              onClick={() => {
                if (child) void openChild(task);
                else {
                  setExpanded(expanded === task.id ? undefined : task.id);
                  send({ kind: 'inspectCommand', taskId: task.id });
                }
              }}
              style={{ flexGrow: 1, minWidth: 0, justifyContent: 'flex-start', gap: 8 }}
            >
              <Icon name={child ? 'bot' : 'terminal'} size={12} />
              <text
                style={{
                  flexGrow: 1,
                  color: colors.text_secondary,
                  fontSize: sp(12.5),
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                }}
              >
                {task.summary ?? task.id}
              </text>
              <text
                style={{
                  color:
                    task.state === 'failed'
                      ? colors.danger
                      : active(task)
                        ? colors.accent
                        : colors.text_tertiary,
                  fontSize: sp(12.5),
                }}
              >
                {taskStateLabel(task.state)}
              </text>
            </Button>
            {active(task) && (
              <IconButton
                name="square"
                label={t(child ? 'transcript.stop_subagent' : 'transcript.cancel_task')}
                onClick={() =>
                  send(
                    child
                      ? { kind: 'cancelSubagent', sessionId: task.id }
                      : { kind: 'interrupt', taskIds: [task.id], reason: 'task' },
                  )
                }
              />
            )}
          </div>
          {expanded === task.id && (
            <text
              style={{
                color: colors.text_secondary,
                fontFamily: 'JetBrains Mono',
                fontSize: codeFontSize,
                maxHeight: 400,
                overflow: 'scroll',
                whiteSpace: 'normal',
              }}
            >
              {typeof view.commandOutput === 'string'
                ? view.commandOutput
                : JSON.stringify(view.commandOutput ?? task.data ?? {}, null, 2)}
            </text>
          )}
        </div>
      ))}
      {!!error && (
        <text style={{ color: colors.danger, fontSize: sp(12.5), padding: 8 }}>{error}</text>
      )}
    </div>
  );
}

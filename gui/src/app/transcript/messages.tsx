import { t, useLocale } from '../../i18n';
import { useEffect, useRef, useState } from 'react';
import { useGpuix, type EventPayload, type PublicInstance } from '@gpuix/react';
import { Markdown } from '../../markdown';
import { getAppStore, usePreferences } from '../../store';
import { nativeRenderer } from '../../native';
import { useTheme } from '../../theme';
import { Button, ContextMenu, Icon, IconButton, Tooltip, type MenuItem } from '../../ui';
import type { SessionView, UserEntry, TurnView } from '../../model/session';
import type { Update } from '../../backend/protocol';
import { restoreNamedSnapshot, copySessionSnapshots } from '../../workspace/checkpoints';
import { runBeforeSubmit } from '../turn-hooks';
import { registerAction, runAction } from '../actions';
import { timestamp } from './rows';
import { AttachmentTile, loadPromptAttachments } from './attachments';
import { copyTurnCheckpoints, getTurnCheckpoint } from './checkpoints';
import { usePromptDisplay } from '../composer/prompt-display';
const promptEditors = new Map<string, () => void>();
export function requestPromptEdit(sessionId: string, requestId: string): boolean {
  const edit = promptEditors.get(`${sessionId}:${requestId}`);
  if (!edit) return false;
  edit();
  return true;
}

function waitForUpdate(sessionId: string, accept: (update: Update) => boolean): Promise<Update> {
  const { promise, resolve: resolveUpdate, reject } = Promise.withResolvers<Update>();
  const stop = getAppStore().backend.subscribe((notification) => {
    if (notification.method !== 'session.update' || notification.params.sessionId !== sessionId)
      return;
    const update = notification.params.update;
    if (update.kind === 'exited') {
      finish();
      reject(new Error(update.error ?? t('transcript.session_ended')));
    } else if (accept(update)) {
      finish();
      resolveUpdate(update);
    }
  });
  const timer = setTimeout(() => {
    stop();
    reject(new Error(t('transcript.session_timeout')));
  }, 30000);
  function finish() {
    clearTimeout(timer);
    stop();
  }
  return promise;
}

export function CopyButton({
  text,
  label = t('transcript.copy_message'),
  small = false,
}: {
  text: string;
  label?: string;
  small?: boolean;
}) {
  const { renderer } = useGpuix();
  const [copied, setCopied] = useState(false);
  const timer = useRef<Timer | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  return (
    <Tooltip content={copied ? t('clipboard.copyDone') : label}>
      <IconButton
        name={copied ? 'check' : 'copy'}
        size={small ? 11 : 14}
        label={label}
        style={{ width: small ? 20 : 27, height: small ? 20 : 27, borderRadius: small ? 5 : 8 }}
        onClick={() => {
          if (!renderer) return;
          nativeRenderer(renderer).writeClipboardText(text);
          setCopied(true);
          clearTimeout(timer.current);
          timer.current = setTimeout(() => setCopied(false), 1800);
        }}
      />
    </Tooltip>
  );
}

export function ResponseFooter({
  sessionId,
  view,
  turn,
  text,
  visible,
}: {
  sessionId: string;
  view: SessionView;
  turn: TurnView;
  text: string;
  visible: boolean;
}) {
  const { colors, sp } = useTheme();
  const locale = useLocale();
  const { renderer } = useGpuix();
  const preferences = usePreferences();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const cursor = turn.requestIds.find((id) => view.providerTurns[id]);
  const eligible = (view.status === 'idle' || view.status === 'failed') && !!cursor;
  async function fork() {
    if (!eligible || busy) return;
    setBusy(true);
    setError('');
    try {
      await getAppStore().continueSession(sessionId);
      const completed = waitForUpdate(sessionId, (update) => update.kind === 'forked');
      await getAppStore().sendIntent(sessionId, { kind: 'fork', promptRequestId: turn.id });
      const result = await completed;
      if (result.kind !== 'forked') return;
      const cwd = view.workspaceRoot;
      if (cwd) {
        const checkpoint = await getTurnCheckpoint(sessionId, turn.id);
        if (checkpoint) {
          await copySessionSnapshots(cwd, sessionId, result.runId, checkpoint.turn);
          await copyTurnCheckpoints(sessionId, result.runId, turn.id);
        }
        await getAppStore().loadSessions(cwd);
      }
      await getAppStore().selectTask(result.runId);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  }
  useEffect(() => {
    if (view.turns.at(-1)?.id !== turn.id) return;
    return registerAction({
      id: 'session.fork',
      label: t('transcript.fork'),
      enabled: () => eligible && !busy,
      run: () => void fork(),
    });
  }, [sessionId, turn.id, view.turns.length, eligible, busy, locale]);
  useEffect(
    () =>
      registerAction({
        id: `session.fork:${turn.id}`,
        label: t('transcript.fork'),
        enabled: () => eligible && !busy,
        run: () => void fork(),
      }),
    [sessionId, turn.id, eligible, busy, locale],
  );
  const snapshot = view.snapshots.find((item) => turn.requestIds.includes(item.request_id));
  const items: MenuItem[] = [
    {
      id: 'copy',
      label: t('transcript.copy_message_menu'),
      onSelect: () => {
        if (renderer) nativeRenderer(renderer).writeClipboardText(text);
      },
    },
    ...(eligible
      ? [
          {
            id: 'fork',
            label: t(busy ? 'transcript.forking_menu' : 'transcript.fork_menu'),
            icon: 'fork' as const,
            disabled: busy,
            onSelect: () => void fork(),
          },
        ]
      : []),
    ...(snapshot
      ? [
          {
            id: 'undo',
            label: t('transcript.undo_files'),
            icon: 'rewind' as const,
            onSelect: () => {
              void getAppStore()
                .sendIntent(sessionId, {
                  kind: 'revertWorkspace',
                  snapshotRequestId: snapshot.request_id,
                })
                .catch((cause) => setError(String(cause)));
            },
          },
        ]
      : []),
  ];
  return (
    <ContextMenu items={items} style={{ width: '100%' }}>
      <div
        style={{
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'center',
          gap: 1,
          height: 27,
          marginLeft: -7,
          opacity: visible ? 1 : 0,
        }}
      >
        <CopyButton text={text} />
        <Tooltip content={t(busy ? 'transcript.forking' : 'transcript.fork')}>
          <IconButton
            name={busy ? 'loader-circle' : 'fork'}
            size={14}
            label={t('transcript.fork')}
            disabled={!eligible || busy}
            onClick={() => void fork()}
            style={{ width: 27, height: 27, borderRadius: 8 }}
          />
        </Tooltip>
        {preferences.showMessageTimes !== false && (
          <text style={{ paddingLeft: 4, color: colors.text_tertiary, fontSize: sp(12.5) }}>
            {timestamp(
              Date.parse(
                view.entries.find((entry) => entry.turnId === turn.id && entry.timestamp)
                  ?.timestamp ?? '',
              ) + (turn.durationMs ?? 0),
            )}
          </text>
        )}
      </div>
      {error && <text style={{ color: colors.danger, fontSize: sp(12.5) }}>{error}</text>}
    </ContextMenu>
  );
}

export function UserMessage({
  sessionId,
  entry,
  view,
  checkpoint,
}: {
  sessionId: string;
  entry: UserEntry;
  view: SessionView;
  checkpoint?: { cwd: string; startRef: string };
}) {
  const { colors, sp } = useTheme();
  const { renderer } = useGpuix();
  const preferences = usePreferences();
  const displayText = usePromptDisplay(sessionId, entry.text);
  const editor = useRef<PublicInstance>(null);
  const bubble = useRef<PublicInstance>(null);
  const [hovered, setHovered] = useState(false);
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(entry.text);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [points, setPoints] = useState<string[]>([]);
  const generation = useRef(0);
  const idle = view.status === 'idle' || view.status === 'failed';
  useEffect(() => {
    if (!idle || !checkpoint || entry.steering) return;
    let active = true;
    const current = ++generation.current;
    const unsubscribe = getAppStore().backend.subscribe((notification) => {
      if (notification.method !== 'session.update' || notification.params.sessionId !== sessionId)
        return;
      const update = notification.params.update;
      if (active && update.kind === 'rewindPoints' && update.generation === current)
        setPoints(update.points?.map((point) => point.request_id) ?? []);
    });
    void getAppStore()
      .sendIntent(sessionId, { kind: 'loadRewindPoints', generation: current, cancelTaskIds: [] })
      .catch((cause) => {
        if (active) setError(String(cause));
      });
    return () => {
      active = false;
      unsubscribe();
    };
  }, [sessionId, idle, checkpoint?.startRef, entry.requestId, entry.steering]);
  const canEdit = idle && !entry.steering && !!checkpoint && points.includes(entry.requestId);
  useEffect(() => {
    if (!canEdit) return;
    const key = `${sessionId}:${entry.requestId}`;
    promptEditors.set(key, () => {
      setValue(entry.text);
      setEditing(true);
    });
    return () => {
      promptEditors.delete(key);
    };
  }, [canEdit, sessionId, entry.requestId, entry.text]);
  useEffect(() => {
    if (editing && editor.current) renderer?.focusElement?.(editor.current.id);
  }, [editing, renderer]);
  async function resend() {
    if (!canEdit || !value.trim() || busy || !checkpoint) return;
    setBusy(true);
    setError('');
    try {
      const attachments = await loadPromptAttachments(entry.attachments);
      const current = ++generation.current;
      const completed = waitForUpdate(
        sessionId,
        (update) => update.kind === 'rewindComplete' && update.generation === current,
      );
      await getAppStore().sendIntent(sessionId, {
        kind: 'rewind',
        generation: current,
        requestId: entry.requestId,
      });
      const result = await completed;
      if (result.kind === 'rewindComplete' && result.error) throw new Error(result.error);
      await restoreNamedSnapshot(checkpoint.cwd, checkpoint.startRef);
      await runBeforeSubmit({ sessionId, cwd: checkpoint.cwd }, (cause) => {
        throw cause;
      });
      await getAppStore().sendIntent(sessionId, {
        kind: 'submit',
        text: value.trim(),
        attachments,
      });
      setEditing(false);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  }
  const copy = (text: string) => {
    if (renderer) nativeRenderer(renderer).writeClipboardText(text);
  };
  function menu(): MenuItem[] {
    const selected = renderer?.getSelectedText?.() ?? '';
    const code = [...entry.text.matchAll(/```[^\n]*\n([\s\S]*?)```/g)]
      .map((match) => match[1])
      .join('\n\n');
    return [
      ...(selected
        ? [{ id: 'selection', label: t('clipboard.selection'), onSelect: () => copy(selected) }]
        : []),
      { id: 'copy', label: t('transcript.copy_message_menu'), onSelect: () => copy(entry.text) },
      ...(!canEdit
        ? [
            {
              id: 'composer',
              label: t('transcript.copy_composer'),
              onSelect: () => {
                const store = getAppStore();
                store.setDraft(
                  {
                    kind: 'session',
                    sessionId,
                    projectId: store.getSnapshot().selectedProject ?? '',
                  },
                  { text: entry.text },
                );
                runAction('composer.focus');
              },
            },
          ]
        : []),
      ...(code ? [{ id: 'code', label: t('clipboard.code'), onSelect: () => copy(code) }] : []),
      ...(canEdit
        ? [
            { id: 'separator', type: 'separator' as const },
            {
              id: 'rewind',
              label: t('transcript.revert_menu'),
              icon: 'rewind' as const,
              onSelect: () => {
                setValue(entry.text);
                setEditing(true);
              },
            },
          ]
        : []),
    ];
  }
  function updateHover(event: EventPayload) {
    const bounds = bubble.current && renderer?.getElementBounds?.(bubble.current.id);
    const x = event.x;
    const y = event.y;
    if (!bounds || x === undefined || y === undefined) return;
    setHovered(
      x >= bounds.x - 12 &&
        x <= bounds.x + bounds.width + 12 &&
        y >= bounds.y - 8 &&
        y <= bounds.y + bounds.height + 38,
    );
  }
  return (
    <div
      onMouseEnter={updateHover}
      onMouseLeave={() => setHovered(false)}
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'flex-end',
        width: '100%',
        gap: 3,
      }}
    >
      {!!entry.attachments.length && (
        <div
          style={{
            display: 'flex',
            flexDirection: 'row',
            flexWrap: 'wrap',
            justifyContent: 'flex-end',
            maxWidth: 540,
            gap: 8,
          }}
        >
          {entry.attachments.map((attachment) => (
            <AttachmentTile key={attachment.id} attachment={attachment} />
          ))}
        </div>
      )}
      {editing ? (
        <div
          style={{
            width: '100%',
            maxWidth: 540,
            borderRadius: 12,
            backgroundColor: colors.raised,
            paddingTop: 9,
            paddingBottom: 8,
          }}
        >
          <textarea
            ref={editor}
            value={value}
            onChange={(event) => setValue(event.value ?? '')}
            style={{
              marginLeft: 12,
              marginRight: 12,
              minHeight: 60,
              maxHeight: 384,
              color: colors.text,
              fontSize: sp(14),
            }}
          />
          <div
            style={{
              display: 'flex',
              flexDirection: 'row',
              justifyContent: 'flex-end',
              gap: 6,
              marginTop: 7,
              paddingLeft: 12,
              paddingRight: 12,
            }}
          >
            <Button
              disabled={busy}
              onClick={() => setEditing(false)}
              variant="secondary"
              style={{
                height: 26,
                paddingLeft: 10,
                paddingRight: 10,
                borderRadius: 7,
                borderColor: colors.border,
              }}
            >
              <text style={{ color: colors.text_secondary, fontSize: sp(12.5) }}>
                {t('transcript.cancel')}
              </text>
            </Button>
            <Button
              disabled={busy || !value.trim()}
              onClick={() => void resend()}
              variant="primary"
              style={{ height: 26, paddingLeft: 11, paddingRight: 11, borderRadius: 7 }}
            >
              <text style={{ color: colors.on_inverse, fontSize: sp(12.5) }}>
                {t(busy ? 'transcript.sending' : 'transcript.send')}
              </text>
            </Button>
          </div>
        </div>
      ) : entry.text.trim() ? (
        <ContextMenu
          items={menu}
          onKeyDown={(event) => {
            if (
              !bubble.current ||
              event.modifiers?.ctrl ||
              event.modifiers?.cmd ||
              event.modifiers?.alt
            )
              return;
            const key = event.key?.toLowerCase();
            const amount =
              key === 'up'
                ? -40
                : key === 'down'
                  ? 40
                  : key === 'pageup'
                    ? -345.6
                    : key === 'pagedown' || key === 'space' || key === ' '
                      ? event.modifiers?.shift
                        ? -345.6
                        : 345.6
                      : 0;
            if (!amount && key !== 'home' && key !== 'end') return;
            event.preventDefault();
            event.stopPropagation();
            const offset = renderer?.getScrollOffset?.(bubble.current.id)?.[1] ?? 0;
            renderer?.scrollTo?.(
              bubble.current.id,
              0,
              key === 'home' ? 0 : key === 'end' ? -1e9 : offset - amount,
            );
          }}
          style={{
            maxWidth: 540,
            minWidth: 0,
            borderRadius: 12,
            borderWidth: 1,
            borderColor: colors.raised,
            backgroundColor: colors.raised,
            paddingLeft: 11,
            paddingRight: 11,
            paddingTop: 7,
            paddingBottom: 7,
            maxHeight: 400,
            overflow: 'hidden',
            focusVisible: { borderColor: colors.accent },
          }}
        >
          <div ref={bubble} style={{ maxHeight: 384, overflow: 'scroll' }}>
            <Markdown
              source={displayText}
              context="user"
              menuItems={menu()}
              onHover={updateHover}
            />
          </div>
        </ContextMenu>
      ) : null}
      <div
        style={{
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'center',
          height: 27,
          gap: 1,
          opacity: hovered || editing ? 1 : 0,
        }}
      >
        {preferences.showMessageTimes !== false && (
          <text
            style={{
              color: colors.text_tertiary,
              fontSize: sp(12.5),
              paddingLeft: 4,
              paddingRight: 4,
            }}
          >
            {timestamp(entry.timestamp ? Date.parse(entry.timestamp) : 0)}
          </text>
        )}
        <CopyButton text={entry.text} />
        {canEdit && (
          <Tooltip content={t('transcript.revert')}>
            <IconButton
              name="rewind"
              size={14}
              label={t('transcript.revert')}
              onClick={() => {
                setValue(entry.text);
                setEditing(true);
              }}
              style={{ width: 27, height: 27, borderRadius: 8 }}
            />
          </Tooltip>
        )}
      </div>
      {!!error && <text style={{ color: colors.danger, fontSize: sp(12.5) }}>{error}</text>}
    </div>
  );
}

export function AssistantMessage({
  sessionId,
  view,
  turnId,
  text,
  streaming,
}: {
  sessionId: string;
  view: SessionView;
  turnId?: string;
  text: string;
  streaming: boolean;
}) {
  const { renderer } = useGpuix();
  const copy = (value: string) => {
    if (renderer) nativeRenderer(renderer).writeClipboardText(value);
  };
  const items = (): MenuItem[] => {
    const selected = renderer?.getSelectedText?.() ?? '';
    const code = [...text.matchAll(/```[^\n]*\n([\s\S]*?)```/g)]
      .map((match) => match[1])
      .join('\n\n');
    const turn = view.turns.find((item) => item.id === turnId);
    const canFork =
      turn &&
      (view.status === 'idle' || view.status === 'failed') &&
      turn.requestIds.some((id) => !!view.providerTurns[id]);
    const snapshot = view.snapshots.find((item) => turn?.requestIds.includes(item.request_id));
    return [
      ...(selected
        ? [{ id: 'selection', label: t('clipboard.selection'), onSelect: () => copy(selected) }]
        : []),
      { id: 'copy', label: t('transcript.copy_message_menu'), onSelect: () => copy(text) },
      ...(code ? [{ id: 'code', label: t('clipboard.code'), onSelect: () => copy(code) }] : []),
      ...(canFork
        ? [
            {
              id: 'fork',
              label: t('transcript.fork_menu'),
              icon: 'fork' as const,
              onSelect: () => {
                runAction(`session.fork:${turnId}`);
              },
            },
          ]
        : []),
      ...(snapshot
        ? [
            {
              id: 'undo',
              label: t('transcript.undo_files'),
              icon: 'rewind' as const,
              onSelect: () => {
                void getAppStore().sendIntent(sessionId, {
                  kind: 'revertWorkspace',
                  snapshotRequestId: snapshot.request_id,
                });
              },
            },
          ]
        : []),
    ];
  };
  return (
    <ContextMenu items={items}>
      <Markdown source={text} streaming={streaming} menuItems={items()} />
    </ContextMenu>
  );
}

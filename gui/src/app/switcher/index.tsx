import { useEffect, useRef, useSyncExternalStore } from 'react';
import { DismissableLayer, useWindowSize, type PublicInstance } from '@gpuix/react';
import { useTheme } from '../../theme';
import { Icon, ellipsis, row } from '../../ui';
import { TaskSwitcherController } from './controller';
export {
  TaskSwitcherController,
  createTaskSwitcherController,
  type TaskSwitcherSnapshot,
} from './controller';

export interface SwitcherTask {
  id: string;
  title?: string;
  projectName?: string;
  provider?: string;
  providerName?: string;
  model?: string;
  branch?: string;
  lastReplyMs?: number;
  status?: 'idle' | 'connecting' | 'working' | 'waiting' | 'background' | 'failed';
}
export interface TaskSwitcherProps {
  controller: TaskSwitcherController;
  tasks: readonly SwitcherTask[];
  onSelect: (id: string) => void;
  restoreFocusId?: number;
  now?: number;
}
const providerColors: Record<string, string> = {
  claude: '#D97757',
  anthropic: '#D97757',
  deepseek: '#4D6BFE',
};
const providerIcons: Record<string, string> = {
  claude: 'claude',
  anthropic: 'claude',
  deepseek: 'deepseek',
  openai: 'openai',
  codex: 'openai',
  grok: 'grok',
  xai: 'grok',
};
export function TaskSwitcher({
  controller,
  tasks,
  onSelect,
  restoreFocusId,
  now,
}: TaskSwitcherProps) {
  const state = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
  const { colors, sp } = useTheme();
  const window = useWindowSize();
  const panel = useRef<PublicInstance>(null);
  const cards = useRef(new Map<string, PublicInstance>());
  useEffect(() => {
    if (state.highlightedId) cards.current.get(state.highlightedId)?.scrollIntoView?.();
  }, [state.highlightedId]);
  if (!state.open) return null;
  const columns = Math.min(
    5,
    state.ids.length,
    Math.max(Math.ceil(state.ids.length / 2), Math.floor((window.width - 60) / 194), 1),
  );
  const rows = Math.ceil(state.ids.length / columns);
  const height = Math.max(185, Math.min(rows * 169 + 16, window.height - 44));
  return (
    <DismissableLayer
      initialFocus={() => panel.current?.id ?? null}
      finalFocus={(previous) => restoreFocusId ?? previous}
      onEscapeKeyDown={() => controller.cancel()}
    >
      <anchored
        position={{ x: 0, y: 0 }}
        deferred
        priority={6}
        occlude={false}
        style={{ backgroundColor: 'transparent' }}
      >
        <div
          style={{
            ...row,
            width: window.width,
            height: window.height,
            justifyContent: 'center',
            pointerEvents: 'auto',
          }}
        >
          <div
            ref={panel}
            tabIndex={-1}
            role="listbox"
            aria-label="Recent tasks"
            onMouseDownOutside={(event) => {
              if (event.button === 0) controller.cancel();
            }}
            style={{
              width: columns * 194 + 16,
              height,
              flexShrink: 0,
              padding: 8,
              borderRadius: 26,
              borderWidth: 1,
              borderColor: colors.border_strong,
              backgroundColor: colors.raised,
              overflowY: 'scroll',
              display: 'flex',
              flexDirection: 'column',
              boxShadow: {
                offsetX: 0,
                offsetY: 20,
                blurRadius: 40,
                spreadRadius: -5,
                color: '#00000055',
              },
            }}
          >
            {Array.from({ length: rows }, (_, rowIndex) => (
              <div key={rowIndex} style={{ ...row, height: 169, flexShrink: 0 }}>
                {state.ids.slice(rowIndex * columns, (rowIndex + 1) * columns).map((id) => {
                  const task = tasks.find((item) => item.id === id);
                  if (!task) return null;
                  const highlighted = id === state.highlightedId;
                  const provider = (task.provider || '').toLowerCase();
                  const providerName = task.providerName || task.provider || 'Harness';
                  const providerColor = providerColors[provider] || colors.text;
                  const providerIcon = providerIcons[provider]
                    ? `provider-${providerIcons[provider]}`
                    : 'sparkle';
                  const status = task.status || 'idle';
                  const statusIcon =
                    status === 'working' || status === 'connecting'
                      ? 'loader-circle'
                      : status === 'waiting'
                        ? 'alert'
                        : status === 'background'
                          ? 'hourglass'
                          : status === 'failed'
                            ? 'x'
                            : undefined;
                  const statusColor =
                    status === 'working' || status === 'connecting'
                      ? colors.accent
                      : status === 'waiting'
                        ? colors.warning
                        : status === 'failed'
                          ? colors.danger
                          : colors.text_secondary;
                  const seconds =
                    task.lastReplyMs === undefined
                      ? undefined
                      : Math.max(0, Math.floor(((now ?? Date.now()) - task.lastReplyMs) / 1000));
                  const age =
                    seconds === undefined
                      ? ''
                      : seconds < 60
                        ? 'just now'
                        : seconds < 3600
                          ? `${Math.floor(seconds / 60)}m`
                          : seconds < 86400
                            ? `${Math.floor(seconds / 3600)}h`
                            : `${Math.floor(seconds / 86400)}d`;
                  return (
                    <div
                      key={id}
                      ref={(instance) => {
                        if (instance) cards.current.set(id, instance);
                        else cards.current.delete(id);
                      }}
                      role="option"
                      aria-label={task.title || 'New task'}
                      aria-selected={highlighted}
                      onMouseMove={() => controller.highlight(id)}
                      onMouseDown={(event) => {
                        if (event.button === 0) {
                          const selected = controller.commit(id);
                          if (selected) onSelect(selected);
                        }
                      }}
                      style={{
                        width: 194,
                        height: 169,
                        flexShrink: 0,
                        paddingLeft: 9,
                        paddingRight: 9,
                        paddingTop: 9,
                        paddingBottom: 14,
                        borderRadius: 16,
                        display: 'flex',
                        flexDirection: 'column',
                        gap: 9,
                        backgroundColor: highlighted ? colors.overlay_strong : undefined,
                        hover: {
                          backgroundColor: highlighted ? colors.overlay_strong : colors.overlay,
                        },
                      }}
                    >
                      <div
                        style={{
                          width: 176,
                          height: 119,
                          flexShrink: 0,
                          borderRadius: 12,
                          borderWidth: 1,
                          borderColor: colors.border_strong,
                          backgroundColor: colors.inset,
                          padding: 12,
                          display: 'flex',
                          flexDirection: 'column',
                        }}
                      >
                        <div style={{ ...row, height: 18, flexShrink: 0, gap: 7 }}>
                          <Icon name={providerIcon} size={14} color={providerColor} />
                          <text
                            style={{
                              ...ellipsis,
                              flexGrow: 1,
                              fontSize: sp(12.5),
                              fontWeight: 500,
                              color: colors.text_secondary,
                            }}
                          >
                            {providerName}
                          </text>
                          {statusIcon && <Icon name={statusIcon} size={12} color={statusColor} />}
                        </div>
                        <div
                          style={{
                            display: 'flex',
                            flexDirection: 'column',
                            justifyContent: 'center',
                            flexGrow: 1,
                            minHeight: 0,
                            gap: 6,
                          }}
                        >
                          <div style={{ ...row, gap: 7 }}>
                            <Icon name="folder" size={16} />
                            <text
                              style={{
                                ...ellipsis,
                                fontSize: sp(14),
                                fontWeight: 500,
                                color: colors.text,
                              }}
                            >
                              {task.projectName || 'No project'}
                            </text>
                          </div>
                          <text
                            style={{ ...ellipsis, fontSize: sp(12.5), color: colors.text_tertiary }}
                          >
                            {task.model || providerName}
                          </text>
                        </div>
                        <div style={{ ...row, height: 16, flexShrink: 0, gap: 5 }}>
                          {task.branch && (
                            <>
                              <Icon name="git-branch" size={11.5} />
                              <text
                                style={{
                                  ...ellipsis,
                                  flexShrink: 1,
                                  fontSize: sp(12.5),
                                  color: colors.text_tertiary,
                                }}
                              >
                                {task.branch}
                              </text>
                            </>
                          )}
                          <div style={{ flexGrow: 1 }} />
                          <text
                            style={{ fontSize: sp(12.5), flexShrink: 0, color: colors.text_ghost }}
                          >
                            {age}
                          </text>
                        </div>
                      </div>
                      <div style={{ ...row, height: 18, flexShrink: 0, gap: 8 }}>
                        <Icon
                          name={providerIcon}
                          size={16}
                          color={highlighted ? providerColor : colors.text_secondary}
                        />
                        <text
                          style={{
                            ...ellipsis,
                            fontSize: sp(14),
                            lineHeight: sp(18),
                            fontWeight: 500,
                            color: highlighted ? colors.text : colors.text_secondary,
                          }}
                        >
                          {task.title || 'New task'}
                        </text>
                      </div>
                    </div>
                  );
                })}
              </div>
            ))}
          </div>
        </div>
      </anchored>
    </DismissableLayer>
  );
}

import { useEffect, useRef, useState, type ReactNode } from 'react';
import type { PublicInstance } from '@gpuix/react';
import { useTheme } from '../../theme';
import { Button, Icon, IconButton, Menu, Tooltip, ellipsis, row, useToast } from '../../ui';
import { getAppStore } from '../../store';
import { registerAction, runAction } from '../actions';
import { BrowserView } from './browser';
import { FilesView, createFilesState, saveFileState, type FilesState } from './files';
import { Review, openReviewTurn } from './review';
import { CommitDialog } from './commit';
import { TerminalView } from './terminal';
import { SessionSurface, TodosSurface, PlanSurface } from './session';
import { FileIcon } from './shared';
export { GitChangesChip } from './review';
export { CommitDialog } from './commit';
export type PanelKind = 'browser' | 'terminal' | 'files' | 'review' | 'session' | 'todos' | 'plan';
interface Tab {
  id: number;
  kind: PanelKind;
  title?: string;
}
interface PanelState {
  tabs: Tab[];
  active?: number;
  files: FilesState;
  cwd?: string | null;
}
const states = new Map<string, PanelState>();
let serial = 0;
let activeKey = '';
const listeners = new Set<() => void>();
const surfaceInfo: Record<PanelKind, { label: string; icon: string; description: string }> = {
  browser: { label: 'Browser', icon: 'globe', description: 'Open a local app or URL' },
  terminal: { label: 'Terminal', icon: 'terminal', description: 'Start a shell in this workspace' },
  files: { label: 'Files', icon: 'folder', description: 'Browse and read workspace files' },
  review: { label: 'Review', icon: 'file-diff', description: 'Review file changes' },
  session: { label: 'Session', icon: 'info', description: 'Servers, modified files and runtime' },
  todos: { label: 'Todos', icon: 'list', description: 'Track the latest task checklist' },
  plan: { label: 'Plan', icon: 'file', description: 'Read workspace plans' },
};
function currentState() {
  let state = states.get(activeKey);
  if (!state) {
    state = { tabs: [], files: createFilesState() };
    states.set(activeKey, state);
  }
  return state;
}
function notify() {
  listeners.forEach((listener) => listener());
}
export function openPanelSurface(kind: PanelKind) {
  const state = currentState();
  let tab = !['terminal', 'browser'].includes(kind)
    ? state.tabs.find((item) => item.kind === kind)
    : undefined;
  if (!tab) {
    tab = { id: ++serial, kind };
    state.tabs.push(tab);
  }
  state.active = tab.id;
  runAction('shell.showRightPanel');
  if (kind === 'review' || (kind === 'files' && state.files.selected)) {
    const store = getAppStore();
    store.setLayout({
      rightWidth: Math.max(Number(store.getSnapshot().layout.rightWidth) || 460, 684),
    });
  }
  notify();
}
export function openPanelFile(path: string) {
  currentState().files.selected = path.replace(/:\d+(?::\d+)?$/, '');
  openPanelSurface('files');
}
export function openPanelReviewTurn(sessionId: string, turnId: string) {
  openReviewTurn(sessionId, turnId);
  openPanelSurface('review');
}
export function closeActivePanelTab() {
  const state = currentState();
  const index = state.tabs.findIndex((tab) => tab.id === state.active);
  if (index < 0) return;
  closeTab(state.tabs[index]!.id);
}
function closeTab(id: number) {
  const state = currentState();
  const index = state.tabs.findIndex((tab) => tab.id === id);
  state.tabs = state.tabs.filter((tab) => tab.id !== id);
  if (state.active === id) state.active = state.tabs[Math.max(0, index - 1)]?.id;
  if (!state.tabs.length) {
    runAction('shell.hideRightPanel');
    runAction('composer.focus');
  }
  notify();
}
export async function saveActivePanelFile() {
  const state = currentState();
  const tab = state.tabs.find((item) => item.id === state.active);
  if (tab?.kind !== 'files') return;
  const project = getAppStore()
    .getSnapshot()
    .projects.find((item) => item.id === getAppStore().getSnapshot().selectedProject);
  if (project) {
    await saveFileState(project.cwd, state.files);
    notify();
  }
}
export function RightPanel({
  cwd,
  sessionId,
  headerTrailing,
}: {
  cwd: string | null;
  sessionId: string | null;
  headerTrailing?: ReactNode;
}) {
  const { colors, sp } = useTheme();
  const { showToast } = useToast();
  const [, update] = useState(0);
  const [menu, setMenu] = useState(false);
  const [commit, setCommit] = useState(false);
  const plus = useRef<PublicInstance>(null);
  const tabRefs = useRef(new Map<number, PublicInstance>());
  const [scroll, setScroll] = useState({ x: 0, max: 0 });
  const [stripWidth, setStripWidth] = useState(0);
  const [contentWidth, setContentWidth] = useState(0);
  activeKey = sessionId ?? cwd ?? '';
  const state = currentState();
  state.cwd = cwd;
  useEffect(() => {
    if (state.active !== undefined) tabRefs.current.get(state.active)?.scrollIntoView?.();
  }, [state.active]);
  const active = state.tabs.find((tab) => tab.id === state.active);
  useEffect(() => {
    const listener = () => update((value) => value + 1);
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }, []);
  useEffect(() => {
    const cleanup = [
      registerAction({
        id: 'panel.close',
        label: 'Close panel tab',
        enabled: () => currentState().active !== undefined,
        run: closeActivePanelTab,
      }),
      registerAction({
        id: 'panel.save',
        label: 'Save file',
        run: () => {
          void saveActivePanelFile().catch((error) => showToast(String(error)));
        },
      }),
      registerAction({ id: 'git.commit', label: 'Commit or push', run: () => setCommit(true) }),
      ...Object.keys(surfaceInfo).map((kind) =>
        registerAction({
          id: `panel.${kind}`,
          label: surfaceInfo[kind as PanelKind].label,
          run: () => openPanelSurface(kind as PanelKind),
        }),
      ),
    ];
    return () => cleanup.forEach((remove) => remove());
  }, [showToast]);
  const changed = () => {
    notify();
  };
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: '100%',
        height: '100%',
        flexGrow: 1,
        minHeight: 0,
        minWidth: 0,
        backgroundColor: colors.surface,
      }}
    >
      <div style={{ ...row, height: 48, flexShrink: 0, paddingLeft: 10, paddingRight: 14, gap: 6 }}>
        <div style={{ flexGrow: 1, minWidth: 0, position: 'relative' }}>
          <div
            onLayout={(event) => setStripWidth(event.width ?? 0)}
            onScrollChange={(event) =>
              setScroll({ x: -(event.scrollX ?? 0), max: Math.max(0, contentWidth - stripWidth) })
            }
            style={{ ...row, overflowX: 'scroll', height: 28 }}
          >
            <div
              onLayout={(event) => setContentWidth(event.width ?? 0)}
              style={{ ...row, flexShrink: 0, gap: 4 }}
            >
              {state.tabs.map((tab) => {
                const info = surfaceInfo[tab.kind];
                const path = tab.kind === 'files' ? state.files.selected : undefined;
                const buffer = path ? state.files.buffers.get(path) : undefined;
                const dirty = buffer && !buffer.error && buffer.text !== buffer.saved;
                return (
                  <Button
                    key={tab.id}
                    ref={(instance) => {
                      if (instance) tabRefs.current.set(tab.id, instance);
                      else tabRefs.current.delete(tab.id);
                    }}
                    onClick={() => {
                      if (!state.tabs.some((item) => item.id === tab.id)) return;
                      state.active = tab.id;
                      changed();
                    }}
                    style={{
                      minWidth: 100,
                      maxWidth: 176,
                      height: 28,
                      paddingLeft: 8,
                      paddingRight: 8,
                      borderRadius: 6,
                      gap: 6,
                      flexShrink: 0,
                      backgroundColor: tab.id === state.active ? colors.overlay_strong : undefined,
                    }}
                  >
                    {path ? (
                      <FileIcon path={path} size={13} />
                    ) : (
                      <Icon name={info.icon} size={13} />
                    )}
                    <text
                      style={{
                        ...ellipsis,
                        flexGrow: 1,
                        color: tab.id === state.active ? colors.text : colors.text_secondary,
                        fontSize: sp(12.5),
                      }}
                    >
                      {path?.split('/').pop() ?? tab.title?.replace(/[\r\n]+/g, ' ') ?? info.label}
                    </text>
                    {dirty && (
                      <Tooltip content="Unsaved changes — Ctrl+S to save">
                        <div
                          style={{
                            width: 7,
                            height: 7,
                            borderRadius: 4,
                            backgroundColor: colors.warning,
                          }}
                        />
                      </Tooltip>
                    )}
                    <IconButton
                      name="x"
                      size={10}
                      style={{ width: 16, height: 16, borderRadius: 4 }}
                      onClick={() => closeTab(tab.id)}
                    />
                  </Button>
                );
              })}
              <div style={{ width: 24, flexShrink: 0 }} />
            </div>
          </div>
          {scroll.x > 0 && (
            <div
              style={{
                position: 'absolute',
                left: 0,
                top: 0,
                bottom: 0,
                width: 24,
                backgroundColor: colors.surface,
                opacity: 0.85,
                pointerEvents: 'none',
              }}
            />
          )}
          {scroll.x < contentWidth - stripWidth && (
            <div
              style={{
                position: 'absolute',
                right: 0,
                top: 0,
                bottom: 0,
                width: 24,
                backgroundColor: colors.surface,
                opacity: 0.85,
                pointerEvents: 'none',
              }}
            />
          )}
        </div>
        {!!state.tabs.length && (
          <div style={{ position: 'relative' }}>
            <IconButton ref={plus} name="plus" size={14} onClick={() => setMenu(!menu)} />
            {menu && (
              <Menu
                restoreFocusId={plus.current?.id}
                ignoreOutsideId={plus.current?.id}
                onClose={() => setMenu(false)}
                items={Object.entries(surfaceInfo).map(([kind, info]) => ({
                  id: kind,
                  label: info.label,
                  icon: info.icon,
                  selected:
                    state.tabs.some((tab) => tab.kind === kind) &&
                    !['browser', 'terminal'].includes(kind),
                  onSelect: () => openPanelSurface(kind as PanelKind),
                }))}
              />
            )}
          </div>
        )}
        <IconButton
          name="panel-right"
          size={14}
          label="Hide right panel"
          style={{ width: 26, height: 26 }}
          onClick={() => runAction('shell.hideRightPanel')}
        />
        {headerTrailing}
      </div>
      {!active ? (
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            flexGrow: 1,
            minHeight: 0,
            justifyContent: 'center',
            alignItems: 'center',
            paddingLeft: 20,
            paddingRight: 20,
            paddingBottom: 32,
          }}
        >
          <text style={{ color: colors.text, fontSize: sp(13), fontWeight: 500 }}>
            Open a surface
          </text>
          <text style={{ color: colors.text_tertiary, fontSize: sp(12.5), marginTop: 5 }}>
            Choose what to show in the right panel
          </text>
          <div
            style={{
              display: 'flex',
              flexDirection: 'column',
              width: '100%',
              maxWidth: 420,
              gap: 8,
              marginTop: 18,
            }}
          >
            {(['browser', 'files', 'session', 'plan'] as PanelKind[]).map((first, index) => (
              <div key={first} style={{ ...row, alignItems: 'stretch', gap: 8 }}>
                {[first, ['terminal', 'review', 'todos'][index]].filter(Boolean).map((kind) => {
                  const info = surfaceInfo[kind as PanelKind];
                  return (
                    <Button
                      key={kind}
                      onClick={() => openPanelSurface(kind as PanelKind)}
                      style={{
                        flexGrow: 1,
                        flexBasis: 0,
                        flexShrink: 1,
                        minWidth: 0,
                        height: 112,
                        padding: 14,
                        borderRadius: 8,
                        borderColor: colors.border_strong,
                        backgroundColor: colors.composer,
                        alignItems: 'flex-start',
                        justifyContent: 'flex-start',
                        flexDirection: 'column',
                        hover: { backgroundColor: colors.raised, borderColor: colors.text_ghost },
                      }}
                    >
                      <Icon name={info.icon} size={18} color={colors.text_secondary} />
                      <text
                        style={{
                          color: colors.text,
                          fontSize: sp(12.5),
                          fontWeight: 500,
                          marginTop: 12,
                        }}
                      >
                        {info.label}
                      </text>
                      <text
                        style={{
                          color: colors.text_tertiary,
                          fontSize: sp(12.5),
                          lineHeight: sp(15),
                          marginTop: 4,
                          maxHeight: sp(30),
                          overflow: 'hidden',
                        }}
                      >
                        {info.description}
                      </text>
                    </Button>
                  );
                })}
              </div>
            ))}
          </div>
        </div>
      ) : (
        <div
          style={{
            display: active.kind === 'terminal' ? 'none' : 'flex',
            flexDirection: 'column',
            flexGrow: 1,
            minHeight: 0,
          }}
        >
          {state.tabs.map((tab) => (
            <div
              key={tab.id}
              style={{
                display: tab.id === state.active ? 'flex' : 'none',
                flexDirection: 'column',
                flexGrow: 1,
                minHeight: 0,
              }}
            >
              {tab.kind === 'browser' && <BrowserView active={tab.id === state.active} />}
              {tab.kind === 'files' && cwd && (
                <FilesView cwd={cwd} state={state.files} onChange={changed} />
              )}
              {tab.kind === 'review' && cwd && <Review cwd={cwd} sessionId={sessionId} />}
              {tab.kind === 'session' && <SessionSurface sessionId={sessionId} />}
              {tab.kind === 'todos' && <TodosSurface sessionId={sessionId} />}
              {tab.kind === 'plan' && cwd && <PlanSurface cwd={cwd} />}
            </div>
          ))}
        </div>
      )}
      {[...states].flatMap(([key, task]) =>
        task.tabs
          .filter((tab) => tab.kind === 'terminal')
          .map((tab) => (
            <div
              key={tab.id}
              style={{
                display: key === activeKey && tab.id === task.active ? 'flex' : 'none',
                flexDirection: 'column',
                flexGrow: 1,
                minHeight: 0,
              }}
            >
              <TerminalView
                cwd={task.cwd ?? null}
                active={key === activeKey && tab.id === task.active}
                onTitle={(title) => {
                  if (tab.title !== title) {
                    tab.title = title;
                    changed();
                  }
                }}
              />
            </div>
          )),
      )}
      {cwd && <CommitDialog cwd={cwd} open={commit} onClose={() => setCommit(false)} />}
    </div>
  );
}

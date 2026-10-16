import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useGpuix, type PublicInstance, type StyleDesc } from '@gpuix/react';
import { getAppStore, useStore } from '../../store';
import type { Project } from '../../store';
import { taskTitle } from '../../store/title';
import type { SessionRow, Update } from '../../backend/protocol';
import {
  Button,
  Icon,
  IconButton,
  Menu,
  Scrollbar,
  Spinner,
  TextField,
  Tooltip,
  useToast,
  type MenuItem,
} from '../../ui';
import { row, ellipsis, bare } from '../../ui/controls';
import { nativeRenderer } from '../../native';
import { useTheme } from '../../theme';
import { dateBucket, dateBuckets, sidebarTimestamp, taskTime } from './model';
import { registerAction, runAction } from '../actions';
import { copySessionSnapshots } from '../../workspace/checkpoints';

export interface SidebarProps {
  header?: ReactNode;
  onNewTask: (projectId?: string) => void;
  onOpenProject: () => void;
  onOpenPalette: () => void;
  onOpenSettings: () => void;
  onToggleSidebar?: () => void;
  onHistory?: (direction: 'back' | 'forward') => void;
  canGoBack?: boolean;
  canGoForward?: boolean;
  onFocusComposer?: () => void;
  branchLabels?: Record<string, string>;
  /** Capture clock; omitted in the live application. */
  now?: number;
}
type Group = { key: string; label: string; project?: Project; tasks: SessionRow[] };
type Entry =
  | { kind: 'search'; height: number }
  | { kind: 'header'; group: Group; first: boolean; height: number }
  | { kind: 'task'; task: SessionRow; height: number }
  | { kind: 'more'; group: Group; height: number }
  | { kind: 'space'; key: string; height: number };

export function Sidebar(props: SidebarProps) {
  const { colors, sp } = useTheme();
  const { renderer } = useGpuix();
  const { showToast } = useToast();
  const store = getAppStore();
  const state = useStore((value) => value);
  const [clock, setClock] = useState(Date.now);
  const now = props.now ?? clock;
  const grouping = state.preferences.sidebarGrouping === 'project' ? 'project' : 'updated';
  const oldest = state.preferences.sidebarOrdering === 'oldest';
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [revealed, setRevealed] = useState<Record<string, number>>({});
  const [options, setOptions] = useState(false);
  const [rename, setRename] = useState<{ id: string; title: string; original: string }>();
  const renameRef = useRef(rename);
  renameRef.current = rename;
  const input = useRef<PublicInstance>(null);
  const list = useRef<PublicInstance>(null);
  const optionTrigger = useRef<PublicInstance>(null);
  const taskRefs = useRef(new Map<string, PublicInstance>());
  const [viewport, setViewport] = useState(0);
  const [offset, setOffset] = useState(0);
  const [hoverGroup, setHoverGroup] = useState<string>();
  const [focusGroup, setFocusGroup] = useState<string>();
  useEffect(() => {
    if (props.now !== undefined) return;
    const timer = setInterval(() => setClock(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [props.now]);
  useEffect(() => {
    if (!rename?.id) return;
    const timer = setTimeout(() => {
      if (!renderer || !input.current) return;
      renderer.focusElement?.(input.current.id);
      nativeRenderer(renderer).inputCommand(input.current.id, 'selectAll');
    }, 0);
    return () => clearTimeout(timer);
  }, [rename?.id, renderer]);
  function run(action: Promise<unknown>) {
    void action.catch((error: unknown) =>
      showToast(error instanceof Error ? error.message : String(error)),
    );
  }
  const tasks = Object.values(state.sessions)
    .flat()
    .sort((a, b) => {
      const difference = sidebarTimestamp(a) - sidebarTimestamp(b);
      return (
        (Number.isNaN(difference) ? 0 : difference) * (oldest ? 1 : -1) ||
        a.runId.localeCompare(b.runId)
      );
    });
  const scratchRoot = state.initialized?.home
    ? `${state.initialized.home.replace(/\/$/, '')}/scratch/`
    : undefined;
  function isScratch(cwd: string) {
    return scratchRoot !== undefined && cwd.startsWith(scratchRoot);
  }
  const groups: Group[] = [];
  if (grouping === 'project') {
    const byProject = new Map<string, Group>();
    for (const task of tasks) {
      const project = isScratch(task.cwd)
        ? undefined
        : state.projects.find((candidate) => candidate.cwd === task.cwd);
      const key = project?.id ?? 'projectless';
      let group = byProject.get(key);
      if (!group) {
        group = { key: `project:${key}`, label: project?.name ?? 'No project', project, tasks: [] };
        byProject.set(key, group);
      }
      group.tasks.push(task);
    }
    groups.push(...[...byProject.values()].filter((group) => group.project));
    const projectless = byProject.get('projectless');
    if (projectless) groups.push(projectless);
    for (const group of groups)
      group.tasks.sort((a, b) => Number(isPinned(b)) - Number(isPinned(a)));
  } else {
    const buckets = oldest ? [...dateBuckets].reverse() : dateBuckets;
    for (const label of buckets) {
      const matches = tasks.filter((task) => dateBucket(sidebarTimestamp(task), now) === label);
      if (matches.length) groups.push({ key: `updated:${label}`, label, tasks: matches });
    }
  }
  if (!groups.length) {
    const selectedProject = state.projects.find((item) => item.id === state.selectedProject);
    const project = selectedProject
      ? isScratch(selectedProject.cwd)
        ? undefined
        : selectedProject
      : state.projects.find((item) => !isScratch(item.cwd));
    groups.push({
      key: grouping === 'project' ? `project:${project?.id ?? 'projectless'}` : 'updated:Today',
      label: grouping === 'project' ? (project?.name ?? 'No project') : 'Today',
      project: grouping === 'project' ? project : undefined,
      tasks: [],
    });
  }
  function isPinned(task: SessionRow) {
    return state.pins?.[task.cwd]?.includes(task.runId) ?? false;
  }
  const entries: Entry[] = [{ kind: 'search', height: 42 }];
  for (const [index, group] of groups.entries()) {
    entries.push({ kind: 'header', group, first: index === 0, height: 30 });
    if (!collapsed.has(group.key)) {
      const recent = group.tasks.filter(
        (task) => sidebarTimestamp(task) >= now - 3 * 86400000 || isPinned(task),
      );
      const older = group.tasks.filter((task) => !recent.includes(task));
      const shown =
        grouping === 'project'
          ? group.tasks.filter(
              (task) => recent.includes(task) || older.indexOf(task) < (revealed[group.key] ?? 0),
            )
          : group.tasks;
      entries.push(...shown.map((task): Entry => ({ kind: 'task', task, height: 52 })));
      if (grouping === 'project' && older.length > (revealed[group.key] ?? 0))
        entries.push({ kind: 'more', group, height: 30 });
    }
    entries.push({ kind: 'space', key: group.key, height: 10 });
  }
  const heights = entries.map((entry) => entry.height);
  const content = heights.reduce((sum, height) => sum + height, 0);
  const selection = state.provisionalSelection ?? state.selection;
  const selectedId = selection.kind === 'session' ? selection.sessionId : undefined;
  useEffect(() => {
    if (!selectedId || !renderer || !list.current) return;
    const index = entries.findIndex(
      (entry) => entry.kind === 'task' && entry.task.runId === selectedId,
    );
    if (index < 0) return;
    const visible = taskRefs.current.get(selectedId);
    const bounds = visible && renderer.getElementBounds?.(visible.id);
    const host = renderer.getElementBounds?.(list.current.id);
    if (bounds && host && bounds.y >= host.y && bounds.y + bounds.height <= host.y + host.height)
      return;
    const top = heights.slice(0, index).reduce((sum, height) => sum + height, 0);
    renderer.scrollToItem?.(list.current.id, index, top < offset ? 0 : -Math.max(0, viewport - 52));
  }, [selectedId, grouping, collapsed, revealed, viewport, renderer]);
  function toggle(group: Group, expanded?: boolean) {
    const closing = expanded === undefined ? !collapsed.has(group.key) : !expanded;
    setCollapsed((current) => {
      const next = new Set(current);
      if (closing) next.add(group.key);
      else next.delete(group.key);
      return next;
    });
    if (closing) setRevealed((current) => ({ ...current, [group.key]: 0 }));
  }
  function setOption(key: string, value: string) {
    store.setPreferences({ [key]: value });
    if (list.current) renderer?.scrollToItem?.(list.current.id, 0);
  }
  const optionItems: MenuItem[] = [
    {
      id: 'grouping',
      label: 'Grouping',
      value: grouping === 'project' ? 'Project' : 'Updated',
      items: ['Project', 'Updated'].map((label) => ({
        id: label,
        label,
        selected: grouping === label.toLowerCase(),
        onSelect: () => setOption('sidebarGrouping', label.toLowerCase()),
      })),
    },
    {
      id: 'ordering',
      label: 'Ordering',
      value: oldest ? 'Oldest first' : 'Newest first',
      items: [
        {
          id: 'newest',
          label: 'Newest first',
          selected: !oldest,
          onSelect: () => setOption('sidebarOrdering', 'newest'),
        },
        {
          id: 'oldest',
          label: 'Oldest first',
          selected: oldest,
          onSelect: () => setOption('sidebarOrdering', 'oldest'),
        },
      ],
    },
  ];
  function commitRename() {
    const current = renameRef.current;
    if (!current) return;
    renameRef.current = undefined;
    setRename(undefined);
    const title = current.title.trim();
    if (title && title !== current.original) run(store.renameTask(current.id, title));
  }
  function beginRename(task: SessionRow) {
    const title = taskTitle(task, state.views[task.runId]);
    const group = groups.find((candidate) =>
      candidate.tasks.some((item) => item.runId === task.runId),
    );
    if (group) {
      setCollapsed((current) => {
        const next = new Set(current);
        next.delete(group.key);
        return next;
      });
      setRevealed((current) => ({ ...current, [group.key]: group.tasks.length }));
    }
    setRename({ id: task.runId, title, original: title });
  }
  async function exportMarkdown(task: SessionRow) {
    if (!renderer?.promptForPaths) throw new Error('The native folder picker is unavailable');
    const paths = await renderer.promptForPaths({
      directories: true,
      files: false,
      multiple: false,
    });
    const folder = paths?.[0];
    if (!folder) return;
    const filename = `${task.runId.replace(/[^a-zA-Z0-9_-]/g, '_')}.md`;
    const result = await store.backend.call('sessions.exportMarkdown', {
      runDir: task.runDir,
      dest: `${folder.replace(/\/$/, '')}/${filename}`,
    });
    showToast(`Exported Markdown to ${result.dest}`, 'success');
  }
  async function duplicateTask(task: SessionRow) {
    await store.continueSession(task.runId);
    let stop: (() => void) | undefined;
    let timer: Timer | undefined;
    const completed = new Promise<Extract<Update, { kind: 'forked' }>>((resolve, reject) => {
      stop = store.backend.subscribe((notification) => {
        if (
          notification.method !== 'session.update' ||
          notification.params.sessionId !== task.runId
        )
          return;
        const update = notification.params.update;
        if (update.kind === 'forked') resolve(update);
        else if (update.kind === 'exited') reject(new Error(update.error ?? 'Session ended'));
      });
      timer = setTimeout(() => reject(new Error('The session did not respond')), 30000);
    });
    try {
      const [result] = await Promise.all([
        completed,
        store.sendIntent(task.runId, { kind: 'clone' }),
      ]);
      const turns = store.getSnapshot().views[task.runId]?.turns.length ?? 0;
      if (turns > 0 && !store.getSnapshot().initialized?.mock)
        await copySessionSnapshots(task.cwd, task.runId, result.runId, turns - 1);
      await store.loadSessions(task.cwd);
      await store.selectTask(result.runId);
    } finally {
      clearTimeout(timer);
      stop?.();
    }
  }
  useEffect(() => {
    const selected = tasks.find((task) => task.runId === selectedId);
    const cleanup = [
      registerAction({
        id: 'shell.collapseSidebarGroups',
        label: 'action.collapseSidebarGroups',
        run: () => {
          setCollapsed(new Set(groups.map((group) => group.key)));
          setRevealed({});
        },
      }),
      registerAction({
        id: 'session.rename',
        label: 'action.renameTask',
        enabled: () => Boolean(selected),
        run: () => {
          if (selected) beginRename(selected);
        },
      }),
      registerAction({
        id: 'session.export',
        label: 'action.exportMarkdown',
        enabled: () => Boolean(selected),
        run: () => {
          if (selected) run(exportMarkdown(selected));
        },
      }),
      registerAction({
        id: 'session.clone',
        label: 'action.duplicateTask',
        enabled: () => Boolean(selected),
        run: () => {
          if (selected) run(duplicateTask(selected));
        },
      }),
    ];
    return () => cleanup.forEach((remove) => remove());
  }, [selectedId, state.sessions, grouping, renderer]);
  function menuItems(task: SessionRow): MenuItem[] {
    const selected = task.runId === selectedId;
    return [
      {
        id: 'rename',
        label: 'Rename',
        onSelect: () => {
          if (!selected || !runAction('session.rename')) beginRename(task);
        },
      },
      {
        id: 'pin',
        label: isPinned(task) ? 'Unpin' : 'Pin',
        onSelect: () => run(store.pinTask(task.runId, !isPinned(task))),
      },
      {
        id: 'duplicate',
        label: 'Duplicate',
        onSelect: () => {
          if (!selected || !runAction('session.clone')) run(duplicateTask(task));
        },
      },
      {
        id: 'export',
        label: 'Export Markdown…',
        onSelect: () => {
          if (!selected || !runAction('session.export')) run(exportMarkdown(task));
        },
      },
      { id: 'separator', type: 'separator' },
      { id: 'remove', label: 'Remove', onSelect: () => run(store.removeTask(task.runId)) },
    ];
  }
  const [context, setContext] = useState<{
    task: SessionRow;
    position: { x: number; y: number };
    focus?: number;
  }>();
  const actionStyle: StyleDesc = {
    height: 32,
    paddingLeft: 4,
    paddingRight: 4,
    borderRadius: 7,
    gap: 10,
    justifyContent: 'flex-start',
    hover: { backgroundColor: colors.sidebar_item_background },
  };
  return (
    <div
      aria-label="Sidebar"
      style={{
        display: 'flex',
        flexDirection: 'column',
        flexGrow: 1,
        minHeight: 0,
        height: '100%',
        backgroundColor: colors.sidebar,
      }}
    >
      {props.header ?? (
        <div style={{ ...row, height: 48, paddingLeft: 10, flexShrink: 0 }}>
          <IconButton
            name="panel-left"
            size={14}
            label="Toggle sidebar"
            onClick={props.onToggleSidebar}
            style={{ width: 26, height: 26 }}
          />
          <div style={{ ...row, gap: 2, marginLeft: 6 }}>
            <Tooltip content="Back">
              <IconButton
                name="arrow-left"
                label="Back"
                size={14}
                disabled={!props.canGoBack}
                onClick={() => props.onHistory?.('back')}
                style={{ width: 26, height: 26, opacity: props.canGoBack ? 1 : 0.35 }}
              />
            </Tooltip>
            <Tooltip content="Forward">
              <IconButton
                name="arrow-right"
                label="Forward"
                size={14}
                disabled={!props.canGoForward}
                onClick={() => props.onHistory?.('forward')}
                style={{ width: 26, height: 26, opacity: props.canGoForward ? 1 : 0.35 }}
              />
            </Tooltip>
          </div>
        </div>
      )}
      <div style={{ paddingLeft: 10, paddingRight: 10, flexShrink: 0 }}>
        <Button onClick={() => props.onNewTask(state.selectedProject)} style={actionStyle}>
          <div style={{ ...row, width: 20, justifyContent: 'center' }}>
            <Icon name="compose" size={14} color={colors.text_secondary} />
          </div>
          <text style={{ ...ellipsis, fontSize: sp(13) }}>New Task</text>
        </Button>
      </div>
      <div
        onLayout={(event) => setViewport(event.height ?? 0)}
        style={{
          display: 'flex',
          flexGrow: 1,
          flexShrink: 1,
          minHeight: 0,
          position: 'relative',
          marginTop: 0,
        }}
      >
        <virtual-list
          ref={list}
          estimatedItemHeight={52}
          onVisibleRange={() => {
            if (!list.current || !renderer) return;
            const anchor = renderer.getListScrollTop?.(list.current.id);
            if (anchor) {
              const index = anchor[0] ?? 0;
              const top =
                heights.slice(0, index).reduce((sum, height) => sum + height, 0) + (anchor[1] ?? 0);
              setOffset(top);
            }
          }}
          style={{ flexGrow: 1, minHeight: 0, paddingLeft: 10, paddingRight: 10 }}
        >
          {entries.map((entry, index) => {
            if (entry.kind === 'space')
              return <div key={`space:${entry.key}`} style={{ height: 10 }} />;
            if (entry.kind === 'search')
              return (
                <div key="search" style={{ height: 42 }}>
                  <Button onClick={props.onOpenPalette} style={actionStyle}>
                    <div style={{ ...row, width: 20, justifyContent: 'center' }}>
                      <Icon name="search" size={14} color={colors.text_secondary} />
                    </div>
                    <text style={{ fontSize: sp(13) }}>Search</text>
                  </Button>
                </div>
              );
            if (entry.kind === 'more')
              return (
                <Button
                  key={`more:${entry.group.key}`}
                  onClick={() =>
                    setRevealed((current) => ({
                      ...current,
                      [entry.group.key]: (current[entry.group.key] ?? 0) + 30,
                    }))
                  }
                  style={{
                    height: 30,
                    paddingLeft: 28,
                    justifyContent: 'flex-start',
                    fontSize: sp(12.5),
                    color: colors.text_tertiary,
                    hover: { color: colors.text },
                    focusVisible: { color: colors.text },
                    position: 'relative',
                  }}
                >
                  <div
                    style={{
                      position: 'absolute',
                      left: 15,
                      top: 0,
                      width: 9,
                      height: 15,
                      borderLeftWidth: 1,
                      borderBottomWidth: 1,
                      borderColor: colors.border,
                      borderBottomLeftRadius: 4,
                    }}
                  />
                  <text>Show more</text>
                </Button>
              );
            if (entry.kind === 'header') {
              const group = entry.group;
              const closed = collapsed.has(group.key);
              const showCompose = hoverGroup === group.key || focusGroup === group.key;
              return (
                <div
                  key={group.key}
                  onMouseEnter={() => setHoverGroup(group.key)}
                  onMouseLeave={() => setHoverGroup(undefined)}
                  style={{ ...row, height: 30, position: 'relative' }}
                >
                  {grouping === 'project' && !closed && (
                    <div
                      style={{
                        position: 'absolute',
                        left: 15,
                        top: 19,
                        bottom: -2,
                        width: 1,
                        backgroundColor: colors.border,
                      }}
                    />
                  )}
                  <Button
                    aria-expanded={!closed}
                    aria-label={group.label}
                    onClick={() => toggle(group)}
                    onFocus={() => setFocusGroup(group.key)}
                    onBlur={() => setFocusGroup(undefined)}
                    onKeyDown={(event) => {
                      if (bare(event) && ['left', 'right'].includes(event.key ?? '')) {
                        event.preventDefault();
                        event.stopPropagation();
                        toggle(group, event.key === 'right');
                      }
                    }}
                    style={{
                      height: 28,
                      flexGrow: 1,
                      minWidth: 0,
                      paddingLeft: 8,
                      paddingRight: 3,
                      gap: 5,
                      borderRadius: 6,
                      justifyContent: 'flex-start',
                      hover: { backgroundColor: colors.sidebar_item_background },
                    }}
                  >
                    {grouping === 'project' ? (
                      <Icon
                        name={closed ? 'folder' : 'folder-open'}
                        size={14}
                        color={colors.text_secondary}
                      />
                    ) : (
                      <div style={{ width: closed || showCompose ? 14 : 0, overflow: 'hidden' }}>
                        <Icon name={closed ? 'chevron-right' : 'chevron-down'} size={14} />
                      </div>
                    )}
                    <text
                      style={{
                        ...ellipsis,
                        fontSize: sp(13),
                        fontWeight: 500,
                        color: colors.text_secondary,
                      }}
                    >
                      {group.label}
                    </text>
                  </Button>
                  {grouping === 'project' && (
                    <Tooltip content="New Task">
                      <IconButton
                        name="compose"
                        label={`New task in ${group.label}`}
                        size={14}
                        onFocus={() => setFocusGroup(group.key)}
                        onBlur={() => setFocusGroup(undefined)}
                        onClick={() => props.onNewTask(group.project?.id)}
                        style={{
                          width: showCompose ? 20 : 0,
                          height: 22,
                          opacity: showCompose ? 1 : 0,
                          borderRadius: 4,
                        }}
                      />
                    </Tooltip>
                  )}
                  {entry.first && (
                    <>
                      <div style={{ position: 'relative' }}>
                        <Tooltip content="Sidebar options">
                          <IconButton
                            ref={optionTrigger}
                            name="list-filter"
                            label="Sidebar options"
                            size={14}
                            onClick={() => setOptions(!options)}
                            style={{
                              width: 20,
                              height: 20,
                              backgroundColor: options ? colors.overlay_strong : undefined,
                            }}
                          />
                        </Tooltip>
                        {options && (
                          <Menu
                            items={optionItems}
                            restoreFocusId={optionTrigger.current?.id}
                            onClose={() => setOptions(false)}
                          />
                        )}
                      </div>
                      <Tooltip content="New project">
                        <IconButton
                          name="folder-new"
                          label="New project"
                          size={14}
                          onClick={props.onOpenProject}
                          style={{ width: 20, height: 22, marginLeft: 2 }}
                        />
                      </Tooltip>
                    </>
                  )}
                </div>
              );
            }
            const task = entry.task;
            const view = state.views[task.runId];
            const title = taskTitle(task, view);
            const status = store.sessionStatus(task.runId);
            const busy = ['working', 'background', 'waiting'].includes(status);
            const scratch = isScratch(task.cwd);
            const project = scratch
              ? undefined
              : state.projects.find((candidate) => candidate.cwd === task.cwd);
            const detail =
              grouping === 'project'
                ? scratch
                  ? undefined
                  : props.branchLabels?.[task.cwd]
                : scratch
                  ? 'No project'
                  : (project?.name ?? 'Unknown project');
            const editing = rename?.id === task.runId;
            function openContext(x?: number, y?: number) {
              const ref = taskRefs.current.get(task.runId);
              const bounds = ref && renderer?.getElementBounds?.(ref.id);
              setContext({
                task,
                position: {
                  x: x ?? bounds?.x ?? 0,
                  y: y ?? (bounds?.y ?? 0) + (bounds?.height ?? 0),
                },
                focus: ref?.id,
              });
            }
            return (
              <div key={task.runId} style={{ height: 52, position: 'relative' }}>
                {grouping === 'project' && (
                  <div
                    style={{
                      position: 'absolute',
                      top: 0,
                      bottom: 0,
                      left: 15,
                      width: 1,
                      backgroundColor: colors.border,
                    }}
                  />
                )}
                <Button
                  ref={(value) => {
                    if (value) taskRefs.current.set(task.runId, value);
                    else taskRefs.current.delete(task.runId);
                  }}
                  role="button"
                  aria-label={title}
                  aria-selected={selectedId === task.runId}
                  onClick={editing ? undefined : () => run(store.selectTask(task.runId))}
                  onMouseDown={(event) => {
                    if (!editing && event.button === 2) openContext(event.x, event.y);
                  }}
                  onKeyDown={(event) => {
                    if (editing) return;
                    if (event.key === 'f10' && event.modifiers?.shift) {
                      event.preventDefault();
                      event.stopPropagation();
                      openContext();
                    }
                    if (bare(event) && ['up', 'down', 'home', 'end'].includes(event.key ?? '')) {
                      event.preventDefault();
                      event.stopPropagation();
                      const indices = entries.flatMap((item, i) =>
                        item.kind === 'task' ? [i] : [],
                      );
                      const at = indices.indexOf(index);
                      const next =
                        event.key === 'home'
                          ? indices[0]
                          : event.key === 'end'
                            ? indices[indices.length - 1]
                            : indices[
                                Math.max(
                                  0,
                                  Math.min(indices.length - 1, at + (event.key === 'up' ? -1 : 1)),
                                )
                              ];
                      if (next !== undefined && list.current) {
                        renderer?.scrollToItem?.(list.current.id, next);
                        const target = entries[next];
                        if (target?.kind === 'task')
                          setTimeout(() => {
                            const element = taskRefs.current.get(target.task.runId);
                            if (element) renderer?.focusElement?.(element.id);
                          }, 0);
                      }
                    }
                  }}
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    alignItems: 'stretch',
                    justifyContent: 'center',
                    height: 51,
                    gap: 4,
                    borderRadius: 7,
                    paddingLeft: grouping === 'project' ? 28 : 8,
                    paddingRight: 8,
                    paddingTop: 7,
                    paddingBottom: 7,
                    backgroundColor:
                      selectedId === task.runId ? colors.sidebar_item_background : undefined,
                    hover: { backgroundColor: colors.sidebar_item_background },
                  }}
                >
                  <div style={{ ...row, height: sp(18), gap: 6 }}>
                    {editing ? (
                      <TextField
                        ref={input}
                        value={rename.title}
                        onChange={(title) =>
                          setRename((current) => current && { ...current, title })
                        }
                        onBlur={commitRename}
                        onKeyDown={(event) => {
                          event.stopPropagation();
                          if (event.key === 'escape') {
                            event.preventDefault();
                            renameRef.current = undefined;
                            setRename(undefined);
                            props.onFocusComposer?.();
                          } else if (event.key === 'enter') {
                            event.preventDefault();
                            commitRename();
                            props.onFocusComposer?.();
                          }
                        }}
                        style={{
                          height: 18,
                          flexGrow: 1,
                          minWidth: 0,
                          paddingLeft: 4,
                          paddingRight: 4,
                          borderWidth: 1,
                          borderColor: colors.accent,
                          borderRadius: 4,
                          backgroundColor: colors.inset,
                          fontSize: sp(13.5),
                        }}
                      />
                    ) : (
                      <text
                        style={{
                          ...ellipsis,
                          flexGrow: 1,
                          fontSize: sp(13.5),
                          lineHeight: sp(18),
                          color: colors.text,
                        }}
                      >
                        {title}
                      </text>
                    )}
                    {isPinned(task) && (
                      <svg
                        src={new URL('./pin.svg', import.meta.url).pathname}
                        style={{
                          width: sp(11),
                          height: sp(11),
                          flexShrink: 0,
                          color: colors.text_tertiary,
                        }}
                      />
                    )}
                    {status === 'working' ? (
                      <Spinner size={12} color={colors.accent} label="Working" />
                    ) : status !== 'idle' ? (
                      <Icon
                        name={
                          status === 'background'
                            ? 'hourglass'
                            : status === 'waiting'
                              ? 'alert'
                              : 'x'
                        }
                        size={12}
                        color={
                          status === 'waiting'
                            ? colors.warning
                            : status === 'failed'
                              ? colors.danger
                              : colors.text_secondary
                        }
                      />
                    ) : null}
                  </div>
                  <div style={{ ...row, height: sp(15), gap: 5 }}>
                    {detail && (
                      <>
                        <Icon name={grouping === 'project' ? 'git-branch' : 'folder'} size={12.5} />
                        <text
                          style={{
                            ...ellipsis,
                            flexGrow: 1,
                            fontSize: sp(grouping === 'project' ? 12.5 : 13),
                            color: colors.text_tertiary,
                          }}
                        >
                          {detail}
                        </text>
                      </>
                    )}
                    {!detail && <div style={{ flexGrow: 1 }} />}
                    <text
                      style={{
                        ...ellipsis,
                        flexShrink: 0,
                        fontSize: sp(12.5),
                        lineHeight: sp(15),
                        color: busy ? colors.text_tertiary : colors.text_ghost,
                      }}
                    >
                      {taskTime(task, view, now)}
                    </text>
                  </div>
                </Button>
              </div>
            );
          })}
        </virtual-list>
        {offset > 0.5 && (
          <div
            style={{
              position: 'absolute',
              top: 0,
              left: 0,
              right: 0,
              height: 1,
              backgroundColor: colors.border,
            }}
          />
        )}
        <Scrollbar
          viewport={viewport}
          content={content}
          offset={offset}
          surface={colors.sidebar}
          onScroll={(top) => {
            if (!list.current) return;
            let index = 0;
            let remaining = top;
            while (index < heights.length - 1 && remaining >= heights[index]!) {
              remaining -= heights[index]!;
              index++;
            }
            renderer?.scrollToItem?.(list.current.id, index, remaining);
          }}
        />
      </div>
      <div style={{ ...row, height: 40, paddingLeft: 10, paddingRight: 10, flexShrink: 0 }}>
        <Tooltip content="Settings">
          <IconButton
            name="settings"
            label="Settings"
            size={14}
            onClick={props.onOpenSettings}
            style={{ width: 26, height: 26 }}
          />
        </Tooltip>
      </div>
      {context && (
        <Menu
          items={menuItems(context.task)}
          position={context.position}
          restoreFocusId={context.focus}
          onClose={() => setContext(undefined)}
        />
      )}
    </div>
  );
}

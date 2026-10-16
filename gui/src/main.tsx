import { useEffect, useRef, useState } from 'react';
import { render, useGpuix, type KeyEvent } from '@gpuix/react';
import { basename } from 'node:path';
import { App } from './app/App';
import { Composer, WorkspaceFooter, type ComposerTarget } from './app/composer';
import { TranscriptView } from './app/transcript';
import { RightPanel, GitChangesChip } from './app/panel/index';
import { SettingsPage } from './app/settings';
import { CommandPalette } from './app/palette';
import { Sidebar } from './app/sidebar';
import { ProjectSelector } from './app/pickers';
import { TaskSwitcher, createTaskSwitcherController } from './app/switcher';
import { registerAction, runAction } from './app/actions';
import { createBackend } from './backend/fixture';
import { AppStore, setAppStore, useStore } from './store';
import { taskTitle } from './store/title';
import { createBackendPersist } from './store/persist-backend';
import { isWindowActive, nativeRenderer } from './native';
import { notify } from './workspace/os';
import { inspectBranches } from './workspace/git';
import { useToast } from './ui';
import { t } from './i18n';
import { restoreWindowGeometry, type WindowGeometry } from './state/window';
import { useTheme } from './theme';

const backend = await createBackend();
const store = new AppStore(backend);
setAppStore(store);
const persist = createBackendPersist(backend);
const taskSwitcher = createTaskSwitcherController();
await store.initialize().catch(() => {});
const savedWindow = await persist.read<WindowGeometry>('window.geometry').catch((error) => {
  store.reportError(t('notice.saveFailed', { error: String(error) }));
  return undefined;
});
const initialWindow = restoreWindowGeometry(savedWindow);

function ErrorToasts({ error }: { error?: string }) {
  const { showToast } = useToast();
  useEffect(() => {
    if (error) showToast(error, 'alert');
  }, [error, showToast]);
  return null;
}

function FpsCounter() {
  const { renderer } = useGpuix();
  const { colors, sp } = useTheme();
  const [fps, setFps] = useState(0);
  useEffect(() => {
    let frames = renderer?.getDebugFrameOverlayStats?.().frames ?? 0;
    let time = performance.now();
    const timer = setInterval(() => {
      const currentFrames = renderer?.getDebugFrameOverlayStats?.().frames ?? frames;
      const currentTime = performance.now();
      setFps(Math.max(0, Math.round(((currentFrames - frames) * 1000) / (currentTime - time))));
      frames = currentFrames;
      time = currentTime;
    }, 500);
    return () => clearInterval(timer);
  }, [renderer]);
  return (
    <text
      style={{
        flexShrink: 0,
        fontSize: sp(11),
        color:
          fps === 0
            ? colors.text_ghost
            : fps >= 55
              ? colors.success
              : fps >= 30
                ? colors.warning
                : colors.danger,
      }}
    >
      {fps} FPS
    </text>
  );
}

function Application() {
  const state = useStore((value) => value);
  const { renderer } = useGpuix();
  const [settings, setSettings] = useState<string>();
  const [palette, setPalette] = useState(false);
  const [branchLabels, setBranchLabels] = useState<Record<string, string>>({});
  const [history, setHistory] = useState<string[]>(state.selectedTask ? [state.selectedTask] : []);
  const [historyIndex, setHistoryIndex] = useState(state.selectedTask ? 0 : -1);
  const [fpsVisible, setFpsVisible] = useState(false);
  const navigating = useRef<string | undefined>(undefined);
  const seenTask = useRef(state.selectedTask);
  const stop = useRef<{ task: string; turn?: string; expires: number } | undefined>(undefined);
  const stopTimer = useRef<Timer | undefined>(undefined);
  const lastGeometry = useRef(JSON.stringify(savedWindow));
  const project = state.projects.find((value) => value.id === state.selectedProject);
  const rows = Object.values(state.sessions).flat();
  const row = rows.find((value) => value.runId === state.selectedTask);
  const view = state.selectedTask ? state.views[state.selectedTask] : undefined;
  const cwd = row?.cwd ?? project?.cwd;
  const scratchRoot = state.initialized ? `${state.initialized.home}/scratch/` : undefined;
  const projectless =
    state.layout.projectless === true || !!(scratchRoot && cwd?.startsWith(scratchRoot));
  const hasWorkspace = !!project || projectless || !!row;
  const target: ComposerTarget =
    state.selectedTask && cwd
      ? { kind: 'task', sessionId: state.selectedTask, cwd }
      : { kind: 'new', projectId: project?.id ?? null, cwd: cwd ?? null };
  const selectedProject = hasWorkspace
    ? {
        name: projectless ? t('welcome.withoutProject') : (project?.name ?? basename(cwd ?? '')),
        projectless,
      }
    : null;
  useEffect(() => {
    const abort = new AbortController();
    for (const workspace of state.projects) {
      void inspectBranches(workspace.cwd, abort.signal)
        .then((snapshot) => {
          if (snapshot && !abort.signal.aborted)
            setBranchLabels((old) => ({ ...old, [workspace.cwd]: snapshot.current }));
        })
        .catch(() => {});
    }
    return () => abort.abort();
  }, [state.projects, state.preferences.sidebarGrouping, state.layout.branchRevision]);
  const captureWindow = async () => {
    if (!renderer || !isWindowActive(renderer)) return;
    const bounds = nativeRenderer(renderer).windowBounds();
    const geometry: WindowGeometry = {
      x: bounds.x,
      y: bounds.y,
      width: bounds.width,
      height: bounds.height,
      maximized: bounds.maximized,
      displayId: bounds.displayId,
    };
    const serialized = JSON.stringify(geometry);
    if (serialized === lastGeometry.current) return;
    await persist.write('window.geometry', geometry);
    lastGeometry.current = serialized;
  };
  useEffect(() => {
    if (!renderer) return;
    const timer = setInterval(() => {
      void captureWindow().catch((error) =>
        store.reportError(t('notice.saveFailed', { error: String(error) })),
      );
    }, 1000);
    return () => clearInterval(timer);
  }, [renderer]);
  const focusComposer = () => {
    setSettings(undefined);
    queueMicrotask(() => runAction('composer.focus'));
  };
  const addProject = (path: string) => {
    store.addProject({ id: path, cwd: path, name: basename(path) || 'Project' });
    focusComposer();
  };
  const openProject = () => {
    void renderer
      ?.promptForPaths?.({ directories: true, files: false, multiple: false })
      .then((paths) => {
        if (paths?.[0]) addProject(paths[0]);
      })
      .catch((error) => store.reportError(String(error)));
  };
  const newTask = (projectId?: string) => {
    store.newTask(projectId ?? state.selectedProject);
    focusComposer();
  };
  const selectProjectless = () => {
    store.selectProjectless();
    focusComposer();
  };
  const navigate = (direction: number) => {
    const index = historyIndex + direction;
    const id = history[index];
    if (!id || settings) return;
    navigating.current = id;
    setHistoryIndex(index);
    void store.selectTask(id).catch(() => {});
  };
  const quit = async () => {
    await captureWindow();
    await store.flushDrafts();
    await store.flush();
    backend.close();
    if (renderer) nativeRenderer(renderer).closeWindow();
    process.exit(0);
  };
  useEffect(() => {
    taskSwitcher.reconcile(
      rows.map((value) => value.runId),
      state.selectedTask,
    );
    const valid = new Set(rows.map((value) => value.runId));
    if (history.some((id) => !valid.has(id))) {
      const next = history.filter((id) => valid.has(id));
      setHistory(next);
      setHistoryIndex(state.selectedTask ? next.indexOf(state.selectedTask) : -1);
    }
    const id = state.selectedTask;
    if (id === seenTask.current) return;
    seenTask.current = id;
    stop.current = undefined;
    store.setLayout({ stopArmed: false });
    if (!id) return;
    if (navigating.current === id) {
      navigating.current = undefined;
      return;
    }
    navigating.current = undefined;
    setHistory((old) => [...old.slice(0, historyIndex + 1), id]);
    setHistoryIndex(historyIndex + 1);
  }, [state.selectedTask, state.sessions]);
  useEffect(() => {
    const disposers = [
      registerAction({
        id: 'shell.newTask',
        label: 'action.newTask',
        shortcut: 'Ctrl+N',
        run: () => newTask(),
      }),
      registerAction({
        id: 'shell.openProject',
        label: 'action.newProject',
        shortcut: 'Ctrl+O',
        run: openProject,
      }),
      registerAction({
        id: 'shell.palette',
        label: 'action.palette',
        shortcut: 'Ctrl+K',
        run: () => setPalette((old) => !old),
      }),
      registerAction({
        id: 'shell.openPalette',
        label: 'action.palette',
        run: () => setPalette(true),
      }),
      registerAction({ id: 'settings.close', label: 'action.focusComposer', run: focusComposer }),
      registerAction({
        id: 'shell.focusComposer',
        label: 'action.focusComposer',
        shortcut: 'Ctrl+L',
        run: focusComposer,
      }),
      registerAction({
        id: 'shell.quit',
        label: 'action.quit',
        shortcut: 'Ctrl+Q',
        run: () => {
          void quit().catch((error) =>
            store.reportError(t('notice.saveFailed', { error: String(error) })),
          );
        },
      }),
      registerAction({
        id: 'shell.fps',
        label: 'action.fps',
        shortcut: 'Ctrl+Alt+Shift+F',
        run: () => setFpsVisible((old) => !old),
      }),
      registerAction({
        id: 'shell.back',
        label: 'navigation.back',
        enabled: () => historyIndex > 0 && !settings,
        run: () => navigate(-1),
      }),
      registerAction({
        id: 'shell.forward',
        label: 'navigation.forward',
        enabled: () => historyIndex + 1 < history.length && !settings,
        run: () => navigate(1),
      }),
      ...[
        'general',
        'appearance',
        'providers',
        'runtime',
        'tools',
        'memory',
        'skills',
        'usage',
        'diagnostics',
      ].map((page) =>
        registerAction({
          id: `settings.${page}`,
          label: 'action.settings',
          run: () => setSettings(page),
        }),
      ),
    ];
    return () => disposers.forEach((dispose) => dispose());
  }, [state.selectedProject, renderer, historyIndex, history.length, settings]);
  useEffect(() => {
    if (!renderer) return;
    const previous = new Map<string, { status: string; turns: Record<string, string> }>();
    const parked = new Set<string>();
    const abort = new AbortController();
    for (const [id, session] of Object.entries(store.getSnapshot().views))
      previous.set(id, {
        status: session.status,
        turns: Object.fromEntries(session.turns.map((turn) => [turn.id, turn.status])),
      });
    const unsubscribe = store.subscribe(() => {
      const snapshot = store.getSnapshot();
      for (const [id, session] of Object.entries(snapshot.views)) {
        const old = previous.get(id);
        previous.set(id, {
          status: session.status,
          turns: Object.fromEntries(session.turns.map((turn) => [turn.id, turn.status])),
        });
        if (!old || isWindowActive(renderer)) continue;
        const turn = session.turns.at(-1);
        if (!turn) continue;
        let body: string | undefined;
        if (
          old.turns[turn.id] === 'working' &&
          ['completed', 'failed', 'interrupted'].includes(turn.status)
        )
          body = t(turn.status === 'completed' ? 'task.completed' : 'task.stopped');
        const parkedKey = `${id}:${turn.id}`;
        if (
          session.status === 'background' &&
          old.status !== 'background' &&
          turn.userIds.length &&
          !parked.has(parkedKey)
        ) {
          parked.add(parkedKey);
          body = t('task.background');
        }
        if (body)
          void notify({
            title: taskTitle(
              Object.values(snapshot.sessions)
                .flat()
                .find((task) => task.runId === id),
              session,
            ),
            body,
            tag: id,
            signal: abort.signal,
            onActivate: async () => {
              await store.selectTask(id);
              setSettings(undefined);
              renderer.activateWindow?.();
            },
          }).catch((error) => {
            if (!abort.signal.aborted) store.reportError(String(error));
          });
      }
    });
    return () => {
      unsubscribe();
      abort.abort();
    };
  }, [renderer]);
  const selectSwitcherTask = (id: string) => {
    setSettings(undefined);
    void store.selectTask(id).catch(() => {});
  };
  const commitSwitcher = () => {
    const id = taskSwitcher.commit();
    if (id) selectSwitcherTask(id);
  };
  const keyUp = (event: KeyEvent) => {
    if (
      taskSwitcher.getSnapshot().open &&
      (event.key === 'control' || event.key === 'ctrl' || !event.modifiers?.ctrl)
    ) {
      commitSwitcher();
      event.preventDefault();
      event.stopPropagation();
    }
  };
  const keyDown = (event: KeyEvent) => {
    const modifiers = event.modifiers;
    const key = event.key?.toLowerCase();
    if (
      taskSwitcher.getSnapshot().open &&
      ['escape', 'enter', 'up', 'down', 'left', 'right', 'home', 'end'].includes(key ?? '')
    ) {
      if (key === 'escape') taskSwitcher.cancel();
      else if (key === 'enter') commitSwitcher();
      else if (key === 'home') taskSwitcher.home();
      else if (key === 'end') taskSwitcher.end();
      else taskSwitcher.cycle(key === 'left' || key === 'up' ? -1 : 1);
    } else if (modifiers?.ctrl && key === 'tab' && !modifiers.alt && !modifiers.cmd) {
      setPalette(false);
      runAction('composer.cancelEdit');
      taskSwitcher.open(modifiers.shift);
    } else if (key === 'escape') {
      if (palette) setPalette(false);
      else if (settings) focusComposer();
      else if (runAction('composer.cancelEdit')) {
        /* owned by composer */
      } else if (
        state.selectedTask &&
        view &&
        ['working', 'waiting', 'background'].includes(view.status)
      ) {
        const turn = view.turns.at(-1)?.id;
        if (
          stop.current?.task === state.selectedTask &&
          stop.current.turn === turn &&
          stop.current.expires > Date.now()
        ) {
          void store.sendIntent(state.selectedTask, {
            kind: 'interrupt',
            taskIds: [],
            reason: 'user',
          });
          stop.current = undefined;
          store.setLayout({ stopArmed: false });
        } else {
          stop.current = { task: state.selectedTask, turn, expires: Date.now() + 3000 };
          store.setLayout({ stopArmed: true });
          clearTimeout(stopTimer.current);
          stopTimer.current = setTimeout(() => {
            stop.current = undefined;
            store.setLayout({ stopArmed: false });
          }, 3000);
        }
      } else return;
    } else if (modifiers?.ctrl && !modifiers.cmd) {
      let action: string | undefined;
      if (modifiers.alt && modifiers.shift && key === 'f') {
        runAction('shell.fps');
      } else if (modifiers.alt) return;
      else if (key === 'q') action = 'shell.quit';
      else if (key === 'w') {
        if (!runAction('panel.close')) runAction('shell.quit');
      } else if (key === 'n') action = 'shell.newTask';
      else if (key === 'o') action = 'shell.openProject';
      else if (key === ',') action = 'settings.general';
      else if (key === 'k') action = 'shell.palette';
      else if (key === 'l') action = 'shell.focusComposer';
      else if (key === '/') action = 'composer.modelPicker';
      else if (key === 'u') action = 'usage.toggle';
      else if (key === 's') action = 'panel.save';
      else if (key === '[') action = 'shell.back';
      else if (key === ']') action = 'shell.forward';
      else if (key === 'f') action = 'transcript.find';
      else if (key === 'c') {
        if (!runAction('transcript.copySelection')) return;
      } else return;
      if (action) runAction(action);
    } else return;
    event.preventDefault();
    event.stopPropagation();
  };
  const content =
    state.selectedTask && (!view || view.entries.length || view.turns.length) ? (
      <TranscriptView sessionId={state.selectedTask} />
    ) : undefined;
  return (
    <App
      persist={persist}
      selectedProject={selectedProject}
      title={taskTitle(row, view)}
      sessionKey={state.selectedTask ?? state.selectedProject ?? (projectless ? 'projectless' : '')}
      renderSidebar={(header) => (
        <Sidebar
          header={header}
          onNewTask={newTask}
          onOpenProject={openProject}
          onOpenPalette={() => setPalette(true)}
          onOpenSettings={() => setSettings('general')}
          onFocusComposer={focusComposer}
          branchLabels={branchLabels}
          now={
            typeof state.preferences.referenceNow === 'number'
              ? state.preferences.referenceNow
              : undefined
          }
        />
      )}
      main={content}
      composer={hasWorkspace ? <Composer target={target} /> : undefined}
      footer={hasWorkspace ? <WorkspaceFooter target={target} /> : undefined}
      fps={fpsVisible ? <FpsCounter /> : undefined}
      renderRightPanel={(controls) => (
        <RightPanel
          cwd={cwd ?? null}
          sessionId={state.selectedTask ?? null}
          headerTrailing={controls}
        />
      )}
      requestedRightWidth={
        typeof state.layout.rightWidth === 'number' ? state.layout.rightWidth : undefined
      }
      headerActions={
        state.selectedTask && cwd ? (
          <GitChangesChip cwd={cwd} onOpen={() => runAction('panel.review')} />
        ) : undefined
      }
      settings={settings ? <SettingsPage page={settings} onClose={focusComposer} /> : undefined}
      onNewTask={() => newTask()}
      onOpenProject={openProject}
      onProjectless={selectProjectless}
      projectSelector={
        <ProjectSelector
          projects={state.projects.filter(
            (value) => !scratchRoot || !value.cwd.startsWith(scratchRoot),
          )}
          currentProjectId={projectless ? null : project?.id}
          site="empty"
          onSelect={(id) => {
            id === null ? selectProjectless() : store.selectProject(id);
            focusComposer();
          }}
          onAddProject={addProject}
          onError={(message) => store.reportError(message)}
        />
      }
      onCommandPalette={() => setPalette((old) => !old)}
      onSettings={() => setSettings('general')}
      onBack={() => navigate(-1)}
      onForward={() => navigate(1)}
      canBack={historyIndex > 0}
      canForward={historyIndex + 1 < history.length}
      onKeyDown={keyDown}
      onKeyUp={keyUp}
      overlay={
        <>
          <ErrorToasts error={state.error} />
          <CommandPalette open={palette} onClose={() => setPalette(false)} />
          <TaskSwitcher
            controller={taskSwitcher}
            tasks={rows.map((task) => ({
              id: task.runId,
              title: taskTitle(task, state.views[task.runId]),
              projectName: state.projects.find((value) => value.cwd === task.cwd)?.name,
              model: task.catalog.provider_model ?? undefined,
              lastReplyMs: task.catalog.last_updated_at
                ? Date.parse(task.catalog.last_updated_at)
                : undefined,
              status: store.sessionStatus(task.runId),
            }))}
            onSelect={selectSwitcherTask}
          />
        </>
      }
    />
  );
}

render(<Application />, {
  ...initialWindow,
  title: 'Harness',
  appId: 'Harness',
  minWidth: 980,
  minHeight: 680,
});

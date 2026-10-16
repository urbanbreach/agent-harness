import { useEffect, useRef, useState } from 'react';
import { DismissableLayer, useGpuix, useWindowSize, type PublicInstance } from '@gpuix/react';
import { getAppStore, useStore } from '../../store';
import { useTheme } from '../../theme';
import { Button, Icon, TextField, useToast, type IconName } from '../../ui';
import { english, t, type TranslationKey } from '../../i18n';
import { listActions, registerAction, runAction, subscribeActions } from '../actions';
import { settingsPages } from '../settings';
import { matchScore } from './search';
import { NestedPalette, type PalettePage } from './nested';
import type { Draft } from '../../store/store';
import { writeClipboardText, type Run } from '../../native';

export interface PaletteRow {
  id: string;
  label: string;
  detail?: string;
  searchTerms?: string;
  excerpt?: string;
  role?: 'user' | 'assistant';
  icon?: IconName;
  shortcut?: string;
  section: string;
  run(): void;
}
function PaletteExcerpt({
  text,
  role,
  query,
}: {
  text: string;
  role?: 'user' | 'assistant';
  query: string;
}) {
  const { colors, sp } = useTheme();
  const prefix = role === 'user' ? 'You: ' : 'Agent: ';
  const runs: Run[] = [
    {
      start: 0,
      end: prefix.length,
      color: role === 'user' ? colors.gauge : colors.success,
      fontWeight: 600,
    },
  ];
  const match = query.trim() ? text.toLowerCase().indexOf(query.trim().toLowerCase()) : -1;
  if (match >= 0)
    runs.push({
      start: prefix.length + match,
      end: prefix.length + match + query.trim().length,
      fontWeight: 600,
      color: colors.text_secondary,
    });
  return (
    <rich-text
      text={prefix + text}
      runs={runs}
      style={{
        fontSize: sp(12.5),
        color: colors.text_tertiary,
        whiteSpace: 'nowrap',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
      }}
    />
  );
}
export function CommandPalette({ open, onClose }: { open: boolean; onClose(): void }) {
  const requestedPage = useRef<PalettePage>('root');
  useEffect(() => {
    const store = getAppStore();
    const removeStash = registerAction({
      id: 'composer.stashDraft',
      label: 'Stash draft',
      icon: 'file-text',
      enabled: () => Boolean(store.getDraft(store.getSnapshot().selection)?.text),
      run: () => {
        const state = store.getSnapshot();
        const draft = store.getDraft(state.selection);
        if (!draft) return;
        const stashes = (state.preferences.stashedDrafts ?? []) as unknown as Draft[];
        store.setPreferences({ stashedDrafts: [draft, ...stashes] as never });
        store.consumeDraft(state.selection);
      },
    });
    const removeRestore = registerAction({
      id: 'composer.restoreStash',
      label: 'Restore stashed draft',
      icon: 'file-text',
      enabled: () =>
        Array.isArray(store.getSnapshot().preferences.stashedDrafts) &&
        Boolean((store.getSnapshot().preferences.stashedDrafts as unknown[]).length),
      run: () => {
        const state = store.getSnapshot();
        const stashes = state.preferences.stashedDrafts as unknown as Draft[];
        if (!stashes[0]) return;
        store.setDraft(state.selection, stashes[0]);
        store.setPreferences({ stashedDrafts: stashes.slice(1) as never });
        runAction('composer.focus');
      },
    });
    const removePages = (['rewind', 'resume', 'tree', 'history', 'stash'] as const).map((page) =>
      registerAction({
        id: `session.${page}`,
        label:
          page === 'rewind'
            ? 'Rewind conversation…'
            : page === 'resume'
              ? 'Resume…'
              : page === 'tree'
                ? 'Session tree'
                : page === 'history'
                  ? 'Prompt history…'
                  : 'Stashed drafts…',
        run: () => {
          requestedPage.current = page;
          runAction('shell.openPalette');
        },
      }),
    );
    return () => {
      removeStash();
      removeRestore();
      removePages.forEach((remove) => remove());
    };
  }, []);
  return open ? (
    <OpenPalette
      initialPage={requestedPage.current}
      onClose={() => {
        requestedPage.current = 'root';
        onClose();
      }}
    />
  ) : null;
}
function OpenPalette({ onClose, initialPage }: { onClose(): void; initialPage: PalettePage }) {
  const { colors, sp } = useTheme();
  const { renderer } = useGpuix();
  const window = useWindowSize();
  const store = getAppStore();
  const state = useStore((state) => state);
  const { showToast } = useToast();
  const [query, setQuery] = useState('');
  const [page, setPage] = useState<PalettePage>(initialPage);
  const [selected, setSelected] = useState(0);
  const [hits, setHits] = useState<
    { runDir: string; role: 'user' | 'assistant'; snippet: string }[]
  >([]);
  const [pending, setPending] = useState(false);
  const [, refreshActions] = useState(0);
  const input = useRef<PublicInstance>(null);
  const card = useRef<PublicInstance>(null);
  const previousFocus = useRef(renderer?.getFocusedElementId?.());
  const cache = useRef(new Map<string, typeof hits>());
  useEffect(() => subscribeActions(() => refreshActions((value) => value + 1)), []);
  useEffect(() => {
    if (input.current) renderer?.focusElement?.(input.current.id);
    return () => {
      if (previousFocus.current) renderer?.focusElement?.(previousFocus.current);
    };
  }, [renderer]);
  useEffect(() => {
    setSelected(0);
  }, [query, page]);
  const cwds = state.projects.map((project) => project.cwd);
  useEffect(() => {
    if (!query.trim() || page !== 'root') {
      setHits([]);
      setPending(false);
      return;
    }
    const key = query.trim();
    const saved = cache.current.get(key);
    if (saved) {
      setHits(saved);
      setPending(false);
      return;
    }
    let active = true;
    setPending(true);
    setHits([]);
    const timer = setTimeout(() => {
      void store.backend
        .call('sessions.search', { cwds, query: key, limit: 50 })
        .then((results) => {
          if (!active) return;
          cache.current.set(key, results);
          if (cache.current.size > 24) cache.current.delete(cache.current.keys().next().value!);
          setHits(results);
        })
        .catch((error) => {
          if (active) showToast(String(error));
        })
        .finally(() => {
          if (active) setPending(false);
        });
    }, 90);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [query, page, cwds.join('\0'), store]);
  function execute(run: () => void) {
    onClose();
    run();
  }
  function nested(value: PalettePage) {
    setPage(value);
    setQuery('');
  }
  function escape() {
    if (query) setQuery('');
    else if (page === 'providers') nested('resume');
    else if (page !== 'root') nested('root');
    else onClose();
  }
  const suggested = ['shell.newTask', 'shell.openProject', 'composer.modelPicker'];
  const hidden = [
    'shell.palette',
    'shell.openPalette',
    'shell.back',
    'shell.forward',
    'composer.focus',
    'composer.cancelEdit',
    'session.rewind',
    'session.resume',
    'session.tree',
    'session.history',
    'session.stash',
  ];
  const actions = listActions().filter(
    (action) =>
      action.enabled?.() !== false &&
      !hidden.includes(action.id) &&
      !action.id.startsWith('session.fork:') &&
      !action.id.startsWith('settings.'),
  );
  let rows: PaletteRow[] = actions.map((action) => ({
    id: action.id,
    label:
      action.id === 'shell.newTask'
        ? 'New task'
        : action.id === 'shell.openProject'
          ? 'Open project…'
          : action.id === 'composer.modelPicker'
            ? 'Choose model…'
            : action.label in english
              ? t(action.label as TranslationKey)
              : action.label,
    icon:
      action.icon ??
      (action.id === 'shell.newTask' || action.id === 'shell.focusComposer'
        ? 'pencil'
        : action.id === 'shell.openProject'
          ? 'folder'
          : action.id === 'composer.modelPicker'
            ? 'bot'
            : 'command'),
    shortcut: action.shortcut,
    section: suggested.includes(action.id) ? 'Suggested' : 'Commands',
    run: () => execute(() => runAction(action.id)),
  }));
  if (state.selectedTask)
    rows.push({
      id: 'copyTaskId',
      label: 'Copy Task ID',
      icon: 'copy',
      section: 'Commands',
      run: () =>
        execute(() => {
          if (!renderer) return;
          writeClipboardText(renderer, state.selectedTask!);
          showToast('Task ID copied', 'success');
        }),
    });
  const selectedSession = Object.values(state.sessions)
    .flat()
    .find((session) => session.runId === state.selectedTask);
  if (selectedSession) {
    rows.push({
      id: 'copyTranscript',
      label: 'Copy transcript',
      icon: 'copy',
      section: 'Commands',
      run: () =>
        execute(() => {
          const transcript =
            state.views[selectedSession.runId]?.entries
              .flatMap((entry) =>
                entry.kind === 'user'
                  ? [`## You\n\n${entry.text}`]
                  : entry.kind === 'text'
                    ? [`## Agent\n\n${entry.text}`]
                    : [],
              )
              .join('\n\n') ?? '';
          if (!renderer) return;
          writeClipboardText(renderer, transcript);
          showToast('Transcript copied', 'success');
        }),
    });
    rows.push({
      id: 'diagnosticArchive',
      label: 'Create diagnostic archive',
      icon: 'file-text',
      section: 'Commands',
      run: () =>
        execute(() => {
          void renderer
            ?.promptForPaths?.({ directories: true, files: false, multiple: false })
            .then(async (paths) => {
              if (!paths?.[0]) return;
              const dest = `${paths[0]}/${selectedSession.runId}-diagnostics.zip`;
              await store.backend.call('sessions.trace', { runDir: selectedSession.runDir, dest });
              showToast(`Diagnostic archive saved to ${dest}`, 'success');
            })
            .catch((error) => showToast(String(error)));
        }),
    });
  }
  const catalogOrder = [
    ...suggested,
    'shell.focusComposer',
    'copyTaskId',
    'usage.toggle',
    'shell.collapseSidebarGroups',
    'shell.toggleSidebar',
    'shell.toggleRightPanel',
  ];
  rows.sort(
    (a, b) =>
      (catalogOrder.includes(a.id) ? catalogOrder.indexOf(a.id) : 100) -
      (catalogOrder.includes(b.id) ? catalogOrder.indexOf(b.id) : 100),
  );
  rows.splice(1, 0, {
    id: 'resume',
    label: 'Resume…',
    icon: 'rotate-cw',
    section: 'Suggested',
    run: () => nested('resume'),
  });
  for (const [id, label, icon] of [
    ['rewind', 'Rewind conversation…', 'rotate-ccw'],
    ['tree', 'Session tree', 'git-branch'],
    ['history', 'Prompt history…', 'history'],
    ['stash', 'Stashed drafts…', 'file-text'],
    ['news', "What's new", 'file-text'],
  ] as const)
    rows.push({ id, label, icon, section: 'Commands', run: () => nested(id) });
  rows.push({
    id: 'help',
    label: 'Help',
    icon: 'circle-help',
    section: 'Commands',
    run: () => execute(() => runAction('settings.help')),
  });
  for (const [id, label, icon] of settingsPages)
    rows.push({
      id: `settings.${id}`,
      label,
      icon,
      searchTerms:
        id === 'appearance'
          ? 'theme language font size light dark system'
          : id === 'general'
            ? 'math reasoning message times timestamps'
            : id === 'providers'
              ? 'authentication sign in accounts API key'
              : id === 'usage'
                ? 'tokens cost context subscription'
                : undefined,
      section: 'Settings',
      run: () => execute(() => runAction(`settings.${id}`)),
    });
  if (query.trim()) {
    rows = rows
      .map((row, order) => ({
        row,
        order,
        score: matchScore(query, `${row.label} ${row.searchTerms ?? ''}`),
      }))
      .filter((item) => item.score > 0)
      .sort((a, b) => b.score - a.score || a.order - b.order)
      .map((item) => ({
        ...item.row,
        section: item.row.section === 'Suggested' ? 'Commands' : item.row.section,
      }));
    const tasks: PaletteRow[] = Object.values(state.sessions)
      .flat()
      .map((task, order) => {
        const hit = hits.find((hit) => hit.runDir === task.runDir);
        const project = state.projects.find((project) => project.cwd === task.cwd);
        const label = task.catalog.run_name ?? 'New task';
        return {
          task,
          hit,
          project,
          label,
          order,
          score: matchScore(
            query,
            `${label} ${project?.name ?? ''} ${task.cwd} ${task.catalog.provider_model ?? ''} task conversation`,
          ),
        };
      })
      .filter((item) => item.score > 0 || item.hit)
      .sort(
        (a, b) =>
          b.score - a.score ||
          (b.task.catalog.last_updated_at ?? '').localeCompare(
            a.task.catalog.last_updated_at ?? '',
          ) ||
          a.order - b.order,
      )
      .slice(0, 12)
      .map(({ task, hit, project, label }) => ({
        id: task.runId,
        label,
        icon: 'message-square',
        detail: [
          project?.name ?? task.cwd,
          task.runId === state.selectedTask ? 'Current' : undefined,
        ]
          .filter(Boolean)
          .join(' · '),
        excerpt: hit?.snippet,
        role: hit?.role,
        section: 'Tasks',
        run: () =>
          execute(() => {
            runAction('settings.close');
            void store
              .selectTask(task.runId)
              .then(() => runAction('composer.focus'))
              .catch((error) => showToast(String(error)));
          }),
      }));
    rows = [
      ...rows.filter((row) => row.section !== 'Settings'),
      ...tasks,
      ...rows.filter((row) => row.section === 'Settings'),
    ];
  }
  const previousRows = useRef<PaletteRow[]>([]);
  if (rows.length) previousRows.current = rows;
  if (!rows.length && pending)
    rows = previousRows.current.map((row) => ({ ...row, excerpt: undefined }));
  const [nestedRows, setNestedRows] = useState<PaletteRow[]>([]);
  const displayed = page === 'root' ? rows : nestedRows;
  const active = Math.min(selected, Math.max(0, displayed.length - 1));
  if (page === 'news')
    return (
      <NestedPalette
        page={page}
        query=""
        onRows={setNestedRows}
        onClose={onClose}
        onPage={nested}
      />
    );
  return (
    <DismissableLayer
      onEscapeKeyDown={escape}
      initialFocus={() => input.current?.id ?? null}
      finalFocus={(previous) => previousFocus.current ?? previous}
    >
      <anchored
        position={{ x: 0, y: 0 }}
        fit="snap"
        snapMargin={0}
        deferred
        priority={3}
        occlude={false}
        style={{ backgroundColor: 'transparent' }}
      >
        <div
          onMouseDown={(event) => {
            const bounds = card.current && renderer?.getElementBounds?.(card.current.id);
            if (
              event.button === 0 &&
              bounds &&
              event.x !== undefined &&
              event.y !== undefined &&
              (event.x < bounds.x ||
                event.x > bounds.x + bounds.width ||
                event.y < bounds.y ||
                event.y > bounds.y + bounds.height)
            )
              onClose();
          }}
          style={{
            pointerEvents: 'auto',
            display: 'flex',
            flexDirection: 'column',
            width: window.width,
            height: window.height,
            backgroundColor: colors.surface === '#1A1A1A' ? '#00000042' : '#00000024',
            alignItems: 'center',
            paddingLeft: 24,
            paddingRight: 24,
            paddingTop: Math.min(72, Math.max(48, window.height * 0.09)),
          }}
        >
          <div
            ref={card}
            style={{
              display: 'flex',
              flexDirection: 'column',
              flexShrink: 0,
              width: Math.min(680, window.width - 48),
              maxHeight: Math.min(480, window.height - 108),
              backgroundColor: colors.raised,
              borderRadius: 15,
              overflow: 'hidden',
              borderWidth: 1,
              borderColor: 'transparent',
              boxShadow: {
                offsetX: 0,
                offsetY: 18,
                blurRadius: 40,
                spreadRadius: 0,
                color: '#00000033',
              },
            }}
          >
            <TextField
              ref={input}
              value={query}
              onChange={setQuery}
              onSubmit={() => displayed[active]?.run()}
              placeholder={
                page === 'root'
                  ? 'Search tasks or run a command…'
                  : page === 'resume'
                    ? 'Search terminal sessions…'
                    : 'Search…'
              }
              style={{
                flexShrink: 0,
                height: 59,
                paddingLeft: 18,
                paddingRight: 19,
                paddingBottom: 1,
                borderRadius: 0,
                borderTopLeftRadius: 14,
                borderTopRightRadius: 14,
                borderWidth: 0,
                borderBottomWidth: 1,
                borderColor: colors.border,
                backgroundColor: colors.raised,
                fontSize: sp(16),
                lineHeight: sp(25),
              }}
              onKeyDown={(event) => {
                const key = event.key ?? '';
                if (
                  !['down', 'up', 'tab', 'home', 'end', 'pagedown', 'pageup', 'escape'].includes(
                    key,
                  ) &&
                  !(event.modifiers?.ctrl && ['n', 'p'].includes(key))
                )
                  return;
                event.preventDefault();
                event.stopPropagation();
                if (key === 'escape') {
                  escape();
                  return;
                }
                if (!displayed.length) return;
                const delta =
                  key === 'pageup'
                    ? -7
                    : key === 'pagedown'
                      ? 7
                      : key === 'up' || key === 'p' || (key === 'tab' && event.modifiers?.shift)
                        ? -1
                        : 1;
                setSelected(
                  key === 'home'
                    ? 0
                    : key === 'end'
                      ? displayed.length - 1
                      : key.startsWith('page')
                        ? Math.max(0, Math.min(displayed.length - 1, active + delta))
                        : (active + delta + displayed.length) % displayed.length,
                );
              }}
            />
            {page !== 'root' && (
              <NestedPalette
                page={page}
                query={query}
                onRows={setNestedRows}
                onClose={onClose}
                onPage={nested}
              />
            )}
            <div
              style={{
                overflowY: 'scroll',
                paddingLeft: 8,
                paddingRight: 8,
                paddingBottom: 8,
                minHeight: 0,
                flexShrink: 1,
              }}
            >
              {displayed.map((row, index) => (
                <div key={row.id}>
                  {index === 0 || displayed[index - 1]?.section !== row.section ? (
                    <div
                      style={{
                        height: 30,
                        paddingLeft: 8,
                        paddingTop: 10,
                        fontSize: sp(12.5),
                        fontWeight: 500,
                        color: colors.text_tertiary,
                      }}
                    >
                      {row.section}
                    </div>
                  ) : null}
                  <Button
                    onMouseEnter={() => setSelected(index)}
                    onClick={row.run}
                    style={{
                      width: '100%',
                      height: row.excerpt ? 60 : 44,
                      justifyContent: 'flex-start',
                      paddingLeft: 14,
                      paddingRight: 11,
                      gap: 10,
                      borderRadius: 9,
                      borderColor: index === active ? colors.border_strong : 'transparent',
                      backgroundColor: index === active ? colors.overlay_strong : undefined,
                    }}
                  >
                    <Icon name={row.icon ?? 'command'} size={16} />
                    <div style={{ flexGrow: 1, minWidth: 0 }}>
                      <div
                        style={{
                          display: 'flex',
                          flexDirection: 'row',
                          gap: 7,
                          alignItems: 'center',
                        }}
                      >
                        <text
                          style={{
                            fontSize: sp(14),
                            color: index === active ? colors.text : colors.text_secondary,
                            fontWeight: index === active ? 500 : 400,
                            whiteSpace: 'nowrap',
                            overflow: 'hidden',
                            textOverflow: 'ellipsis',
                          }}
                        >
                          {row.label.replace(/\s+/g, ' ')}
                        </text>
                        {row.detail && (
                          <text
                            style={{
                              color: colors.text_tertiary,
                              fontSize: sp(12.5),
                              whiteSpace: 'nowrap',
                              overflow: 'hidden',
                              textOverflow: 'ellipsis',
                              flexShrink: 1,
                            }}
                          >
                            {row.detail}
                          </text>
                        )}
                      </div>
                      {row.excerpt && (
                        <PaletteExcerpt text={row.excerpt} role={row.role} query={query} />
                      )}
                    </div>
                    {row.shortcut && (
                      <div
                        style={{
                          height: 22,
                          minWidth: 28,
                          paddingLeft: 7,
                          paddingRight: 7,
                          borderRadius: 7,
                          backgroundColor: colors.overlay_strong,
                          justifyContent: 'center',
                          fontSize: sp(12.5),
                          color: colors.text_tertiary,
                        }}
                      >
                        {row.shortcut}
                      </div>
                    )}
                  </Button>
                </div>
              ))}
              {!displayed.length && page !== 'resume' && (
                <div
                  style={{ height: 180, alignItems: 'center', justifyContent: 'center', gap: 12 }}
                >
                  <Icon name={pending ? 'loader-circle' : 'search'} size={18} />
                  <div style={{ fontSize: sp(13), color: colors.text_secondary }}>
                    {pending ? 'Searching…' : 'No matching tasks or commands'}
                  </div>
                  <div style={{ fontSize: sp(12.5), color: colors.text_tertiary }}>
                    Try a task title, message, project, setting, or command
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>
      </anchored>
    </DismissableLayer>
  );
}

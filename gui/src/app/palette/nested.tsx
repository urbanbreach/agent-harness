import { useEffect, useState } from 'react';
import { getAppStore, useStore } from '../../store';
import type { Draft } from '../../store/store';
import type { ForeignCandidate, Update } from '../../backend/protocol';
import { Button, Dialog, Icon, TextField, useToast } from '../../ui';
import { Markdown } from '../../markdown';
import { useTheme } from '../../theme';
import { runAction } from '../actions';
import { requestPromptEdit } from '../transcript/messages';
import type { PaletteRow } from './index';
import { matchScore } from './search';

export type PalettePage =
  'root' | 'resume' | 'providers' | 'rewind' | 'tree' | 'history' | 'stash' | 'news';
export function NestedPalette({
  page,
  query,
  onRows,
  onClose,
  onPage,
}: {
  page: PalettePage;
  query: string;
  onRows(rows: PaletteRow[]): void;
  onClose(): void;
  onPage(page: PalettePage): void;
}) {
  const store = getAppStore();
  const state = useStore((state) => state);
  const { colors, sp } = useTheme();
  const { showToast } = useToast();
  const project = state.projects.find((project) => project.id === state.selectedProject);
  const cwd = project?.cwd ?? state.initialized?.defaultCwd ?? '.';
  const [source, setSource] = useState(
    state.initialized?.home?.replace(/\/\.harness\/?$/, '') ?? cwd,
  );
  const [provider, setProvider] = useState('all');
  const [candidates, setCandidates] = useState<ForeignCandidate[]>([]);
  const [loading, setLoading] = useState(false);
  const [importing, setImporting] = useState<string>();
  const [error, setError] = useState('');
  const [prompts, setPrompts] = useState<string[]>([]);
  const [news, setNews] = useState<string>();
  const [scan, setScan] = useState(0);
  const view = state.selectedTask ? state.views[state.selectedTask] : undefined;
  useEffect(() => {
    let active = true;
    setError('');
    const load = async () => {
      if (page === 'resume') {
        setLoading(true);
        const result = await store.backend.call('sessions.discoverForeign', { cwd, from: source });
        if (active) setCandidates(result.candidates);
      } else if (page === 'history') {
        const result = await store.backend.call('history.prompts', { cwd });
        if (active) setPrompts(result.prompts);
      } else if (page === 'rewind' && state.selectedTask) {
        await store.sendIntent(state.selectedTask, {
          kind: 'loadRewindPoints',
          generation: Date.now(),
          cancelTaskIds: [],
        });
      } else if (page === 'news') {
        const result = await store.backend.call('app.releaseNotes', {});
        if (active) setNews(result.markdown);
      }
    };
    void load()
      .catch((error) => {
        if (active) setError(String(error));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [page, cwd, scan]);
  useEffect(() => {
    let rows: PaletteRow[] = [];
    if (page === 'resume' || page === 'providers') {
      if (page === 'providers')
        rows = [
          'all',
          ...new Set(
            candidates
              .filter((candidate) => candidate.status === 'discoverable')
              .map((candidate) => candidate.kind),
          ),
        ].map((kind) => ({
          id: kind,
          label: kind === 'all' ? 'All sources' : kind,
          detail: kind === provider ? 'Current provider' : undefined,
          icon: 'bot',
          section: 'Providers',
          run: () => {
            setProvider(kind);
            onPage('resume');
          },
        }));
      else {
        rows = [
          {
            id: 'provider',
            label: provider === 'all' ? 'All sources' : provider,
            detail: 'Change provider',
            icon: 'bot',
            section: 'Terminal sessions',
            run: () => onPage('providers'),
          },
        ];
        rows.push(
          ...candidates
            .filter((candidate) => candidate.status === 'discoverable')
            .filter((candidate) => provider === 'all' || candidate.kind === provider)
            .map((candidate) => ({
              id: candidate.path,
              label: candidate.path.split('/').pop() ?? candidate.path,
              detail:
                importing === candidate.path
                  ? 'Loading conversation…'
                  : `${candidate.kind} · ${candidate.path}`,
              icon: importing === candidate.path ? 'loader-circle' : 'message-square',
              section: 'Terminal sessions',
              run: () => {
                if (importing) return;
                setImporting(candidate.path);
                void store.backend
                  .call('sessions.importForeign', { cwd, from: candidate.path })
                  .then(async (imported) => {
                    await store.loadSessions(cwd);
                    await store.selectTask(imported.run_id);
                    onClose();
                    runAction('settings.close');
                    runAction('composer.focus');
                  })
                  .catch((error) => showToast(`Couldn’t resume session: ${String(error)}`))
                  .finally(() => setImporting(undefined));
              },
            })),
        );
        if (loading || error || rows.length === 1) rows = [];
      }
    } else if (page === 'history')
      rows = prompts.map((text, index) => ({
        id: String(index),
        label: text,
        icon: 'history',
        section: 'Prompt history',
        run: () => {
          store.setDraft(state.selection, { ...store.getDraft(state.selection), text });
          onClose();
          runAction('settings.close');
          runAction('composer.focus');
        },
      }));
    else if (page === 'stash') {
      const stashes = (state.preferences.stashedDrafts ?? []) as unknown as Draft[];
      rows = stashes.map((draft, index) => ({
        id: String(index),
        label: draft.text || 'Draft with attachments',
        icon: 'file-text',
        section: 'Stashed drafts',
        run: () => {
          store.setDraft(state.selection, draft);
          store.setPreferences({ stashedDrafts: stashes.filter((_, at) => at !== index) as never });
          onClose();
          runAction('composer.focus');
        },
      }));
    } else if (page === 'tree') {
      rows = Object.values(state.sessions)
        .flat()
        .map((session) => ({
          id: session.runId,
          label: session.catalog.run_name ?? 'New task',
          detail: session.catalog.parent_session_id
            ? `Child of ${session.catalog.parent_session_id}`
            : 'Root session',
          icon: 'git-branch',
          section: 'Session tree',
          run: () => {
            void store
              .selectTask(session.runId)
              .then(() => {
                onClose();
                runAction('settings.close');
                runAction('composer.focus');
              })
              .catch((error) => showToast(String(error)));
          },
        }));
    } else if (page === 'rewind' && state.selectedTask) {
      const sessionId = state.selectedTask;
      const rewind = view?.rewind as
        Extract<Update, { kind: 'rewindPoints' | 'rewindComplete' }> | undefined;
      if (rewind?.kind === 'rewindPoints')
        rows = [...(rewind.points ?? [])].reverse().map((point) => ({
          id: point.request_id,
          label: point.text,
          detail: `Message ${point.seq}`,
          icon: 'rotate-ccw',
          section: 'Rewind conversation',
          run: () => {
            onClose();
            runAction('settings.close');
            if (!requestPromptEdit(sessionId, point.request_id))
              showToast('This prompt cannot be edited: its workspace checkpoint is unavailable.');
          },
        }));
    }
    onRows(
      rows
        .map((row, index) => ({
          row,
          index,
          score: matchScore(query, `${row.label} ${row.detail ?? ''}`),
        }))
        .filter((item) => item.score > 0)
        .sort((a, b) => b.score - a.score || a.index - b.index)
        .map((item) => item.row),
    );
  }, [
    page,
    query,
    candidates,
    provider,
    prompts,
    importing,
    state.preferences.stashedDrafts,
    state.sessions,
    state.selectedTask,
    state.selection,
    cwd,
    loading,
    error,
    view?.rewind,
  ]);
  return (
    <>
      {page === 'resume' && (
        <div style={{ padding: 12, gap: 8 }}>
          <TextField
            value={source}
            onChange={setSource}
            placeholder="Folder containing foreign sessions"
            onKeyDown={(event) => {
              if (event.key === 'enter') setScan((value) => value + 1);
            }}
          />
          <Button onClick={() => setScan((value) => value + 1)} style={{ padding: 6 }}>
            Scan folder
          </Button>
          {(loading ||
            error ||
            !candidates.some(
              (candidate) =>
                candidate.status === 'discoverable' &&
                (provider === 'all' || candidate.kind === provider),
            )) && (
            <div style={{ height: 180, justifyContent: 'center', alignItems: 'center', gap: 12 }}>
              <Icon name={loading ? 'loader-circle' : error ? 'alert' : 'search'} size={18} />
              <div style={{ fontSize: sp(13), color: colors.text_secondary }}>
                {loading
                  ? 'Loading terminal sessions…'
                  : error
                    ? 'Couldn’t load terminal sessions'
                    : 'No resumable terminal sessions'}
              </div>
              <div style={{ fontSize: sp(12.5), color: colors.text_tertiary }}>
                {error ||
                  'Sessions already in Harness are hidden. Start one in an agent CLI and try again.'}
              </div>
              {!loading && (
                <Button onClick={() => onPage('providers')} style={{ padding: 8 }}>
                  Change provider
                </Button>
              )}
            </div>
          )}
        </div>
      )}
      {error && page !== 'resume' && (
        <div style={{ padding: 16, color: colors.danger, fontSize: sp(12.5) }}>{error}</div>
      )}
      {news !== undefined && (
        <Dialog open onClose={onClose} title="What's new">
          <Markdown source={news} />
        </Dialog>
      )}
    </>
  );
}

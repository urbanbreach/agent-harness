import { useEffect, useState } from 'react';
import { getAppStore, useStore } from '../../store';
import type { Draft } from '../../store/store';
import type { ForeignCandidate, Update } from '../../backend/protocol';
import { Button, Dialog, Icon, TextField, useToast } from '../../ui';
import { Markdown } from '../../markdown';
import { useTheme } from '../../theme';
import { t, useLocale } from '../../i18n';
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
  const locale = useLocale();
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
          label: kind === 'all' ? t('settings.palette_all_sources') : kind,
          detail: kind === provider ? t('command_palette.current_provider') : undefined,
          icon: 'bot',
          section: t('command_palette.providers'),
          run: () => {
            setProvider(kind);
            onPage('resume');
          },
        }));
      else {
        rows = [
          {
            id: 'provider',
            label: provider === 'all' ? t('settings.palette_all_sources') : provider,
            detail: t('command_palette.change_provider'),
            icon: 'bot',
            section: t('command_palette.sessions'),
            run: () => onPage('providers'),
          },
        ];
        rows.push(
          ...candidates
            .filter((candidate) => candidate.status === 'discoverable')
            .filter((candidate) => provider === 'all' || candidate.kind === provider)
            .map<PaletteRow>((candidate) => ({
              id: candidate.path,
              label: candidate.path.split('/').pop() ?? candidate.path,
              detail:
                importing === candidate.path
                  ? t('command_palette.loading_selected_session')
                  : `${candidate.kind} · ${candidate.path}`,
              icon: importing === candidate.path ? 'loader-circle' : 'message-square',
              section: t('command_palette.sessions'),
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
                  .catch((error) =>
                    showToast(t('command_palette.resume_failed', { error: String(error) })),
                  )
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
        section: t('settings.palette_prompt_history'),
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
        label: draft.text || t('settings.palette_draft_attachments'),
        icon: 'file-text',
        section: t('settings.palette_stashed_drafts'),
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
          label: session.catalog.run_name ?? t('command_palette.new_task'),
          detail: session.catalog.parent_session_id
            ? t('settings.palette_child_session', { id: session.catalog.parent_session_id })
            : t('settings.palette_root_session'),
          icon: 'git-branch',
          section: t('settings.palette_session_tree'),
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
          detail: t('settings.palette_message_number', { seq: point.seq }),
          icon: 'rotate-ccw',
          section: t('settings.palette_rewind'),
          run: () => {
            onClose();
            runAction('settings.close');
            if (!requestPromptEdit(sessionId, point.request_id))
              showToast(t('settings.palette_prompt_edit_unavailable'));
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
    locale,
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
            onSubmit={() => setScan((value) => value + 1)}
            placeholder={t('settings.palette_foreign_sessions_folder')}
          />
          <Button onClick={() => setScan((value) => value + 1)} style={{ padding: 6 }}>
            {t('settings.palette_scan_folder')}
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
                  ? t('command_palette.loading_sessions')
                  : error
                    ? t('command_palette.could_not_load_sessions')
                    : t('command_palette.no_resume_sessions')}
              </div>
              <div style={{ fontSize: sp(12.5), color: colors.text_tertiary }}>
                {error || t('command_palette.no_resume_sessions_hint')}
              </div>
              {!loading && (
                <Button onClick={() => onPage('providers')} style={{ padding: 8 }}>
                  {t('command_palette.change_provider')}
                </Button>
              )}
            </div>
          )}
        </div>
      )}
      {error && page !== 'resume' && (
        <div style={{ padding: 16, color: colors.danger, fontSize: sp(12.5) }}>{error}</div>
      )}
      {page === 'news' && (
        <Dialog open onClose={onClose} title={t('settings.palette_whats_new')}>
          <div
            style={{ display: 'flex', flexDirection: 'column', padding: 20, gap: 16, minHeight: 0 }}
          >
            <div
              style={{
                display: 'flex',
                flexDirection: 'row',
                alignItems: 'center',
                justifyContent: 'space-between',
              }}
            >
              <text style={{ fontSize: sp(18), fontWeight: 500 }}>
                {t('settings.palette_whats_new')}
              </text>
              <Button
                aria-label={t('settings.palette_close_release_notes')}
                onClick={onClose}
                style={{ width: 28, height: 28 }}
              >
                <Icon name="x" size={14} />
              </Button>
            </div>
            {error ? (
              <text style={{ color: colors.danger }}>{error}</text>
            ) : news === undefined ? (
              <text style={{ color: colors.text_secondary }}>
                {t('settings.palette_loading_release_notes')}
              </text>
            ) : (
              <div style={{ maxHeight: 480, overflowY: 'scroll' }}>
                <Markdown source={news} />
              </div>
            )}
          </div>
        </Dialog>
      )}
    </>
  );
}

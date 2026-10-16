import { useEffect, useState } from 'react';
import type { Methods } from '../../backend/protocol';
import { getAppStore } from '../../store';
import { Button, Switch, useToast } from '../../ui';
import { SettingsCard, SettingsRow } from './index';
import { t } from '../../i18n';

export function WorktreesPage({ cwd }: { cwd: string }) {
  const backend = getAppStore().backend;
  const { showToast } = useToast();
  const [report, setReport] = useState<Methods['worktrees.list']['result']>();
  const [error, setError] = useState('');
  const [armed, setArmed] = useState<string>();
  const [keepBranch, setKeepBranch] = useState(true);
  const [busy, setBusy] = useState(false);
  async function refresh() {
    setBusy(true);
    try {
      setReport(await backend.call('worktrees.list', { cwd }));
      setError('');
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
    }
  }
  useEffect(() => {
    void refresh();
  }, [cwd]);
  async function remove(slug: string) {
    if (armed !== slug) {
      setArmed(slug);
      return;
    }
    setBusy(true);
    try {
      await backend.call('worktrees.remove', { cwd, slug, keepBranch });
      setArmed(undefined);
      showToast(t('settings.worktree_removed'), 'success');
      await refresh();
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <SettingsCard>
        <SettingsRow
          title={t('settings.managed_worktrees')}
          description={report?.repository_root ?? cwd}
        >
          <Button disabled={busy} onClick={() => void refresh()}>
            {busy ? t('settings.loading') : t('settings.refresh')}
          </Button>
        </SettingsRow>
        <SettingsRow
          title={t('settings.keep_branch')}
          description={t('settings.keep_branch_description')}
          last
        >
          <Switch label={t('settings.keep_branch')} checked={keepBranch} onChange={setKeepBranch} />
        </SettingsRow>
      </SettingsCard>
      <SettingsCard>
        {error && <SettingsRow title={t('settings.worktree_failed')} description={error} last />}
        {report?.worktrees.map((worktree, index, entries) => (
          <SettingsRow
            key={worktree.path}
            title={worktree.slug ?? worktree.branch ?? worktree.path}
            description={`${worktree.path} · ${worktree.branch ?? t('settings.detached_head')}`}
            last={index === entries.length - 1}
          >
            <Button
              disabled={busy || !worktree.slug}
              variant={armed === worktree.slug ? 'danger' : 'ghost'}
              onClick={() => {
                if (worktree.slug) void remove(worktree.slug);
              }}
            >
              {armed === worktree.slug ? t('settings.confirm_remove') : t('settings.remove')}
            </Button>
            {armed === worktree.slug && (
              <Button onClick={() => setArmed(undefined)}>{t('settings.cancel')}</Button>
            )}
          </SettingsRow>
        ))}
        {report && report.worktrees.length === 0 && (
          <SettingsRow
            title={t('settings.no_worktrees')}
            description={t('settings.no_worktrees_description')}
            last
          />
        )}
      </SettingsCard>
    </>
  );
}

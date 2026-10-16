import { useEffect, useState } from 'react';
import type { Methods } from '../../backend/protocol';
import { getAppStore } from '../../store';
import { Button, Switch, useToast } from '../../ui';
import { SettingsCard, SettingsRow } from './index';

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
      showToast('Worktree removed', 'success');
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
        <SettingsRow title="Managed worktrees" description={report?.repository_root ?? cwd}>
          <Button disabled={busy} onClick={() => void refresh()}>
            {busy ? 'Loading…' : 'Refresh'}
          </Button>
        </SettingsRow>
        <SettingsRow
          title="Keep branch when removing"
          description="Removal refuses worktrees with uncommitted changes. Your main checkout is never removed."
          last
        >
          <Switch label="Keep branch when removing" checked={keepBranch} onChange={setKeepBranch} />
        </SettingsRow>
      </SettingsCard>
      <SettingsCard>
        {error && <SettingsRow title="Worktree operation failed" description={error} last />}
        {report?.worktrees.map((worktree, index, entries) => (
          <SettingsRow
            key={worktree.path}
            title={worktree.slug ?? worktree.branch ?? worktree.path}
            description={`${worktree.path} · ${worktree.branch ?? 'Detached HEAD'}`}
            last={index === entries.length - 1}
          >
            <Button
              disabled={busy || !worktree.slug}
              variant={armed === worktree.slug ? 'danger' : 'ghost'}
              onClick={() => {
                if (worktree.slug) void remove(worktree.slug);
              }}
            >
              {armed === worktree.slug ? 'Confirm remove' : 'Remove'}
            </Button>
            {armed === worktree.slug && <Button onClick={() => setArmed(undefined)}>Cancel</Button>}
          </SettingsRow>
        ))}
        {report && report.worktrees.length === 0 && (
          <SettingsRow
            title="No managed worktrees"
            description="Start a new task in a worktree to create an isolated checkout."
            last
          />
        )}
      </SettingsCard>
    </>
  );
}

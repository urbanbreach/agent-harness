import { useEffect, useRef, useState } from 'react';
import type { PublicInstance } from '@gpuix/react';
import { t, useLocale } from '../i18n';
import type { SessionView } from '../model/session';
import { useTheme } from '../theme';
import { Button, Icon, Menu, Spinner, Tooltip, ellipsis, row, type MenuItem } from '../ui';
import { activation } from '../ui/controls';
import { inspectBranches, type BranchSnapshot } from '../workspace/git';
import { runAction } from './actions';
import { useCommitOperation } from './panel/commit';
import { BackgroundWork } from './transcript/background';

export function EnvironmentPopover({
  cwd,
  sessionId,
  view,
}: {
  cwd: string | null;
  sessionId: string | null;
  view?: SessionView;
}) {
  useLocale();
  const { colors, sp } = useTheme();
  const operation = useCommitOperation();
  const committing = Boolean(operation);
  const [open, setOpen] = useState(false);
  const [keyboardOpen, setKeyboardOpen] = useState(false);
  const [snapshot, setSnapshot] = useState<BranchSnapshot | null>();
  const [error, setError] = useState<string>();
  const trigger = useRef<PublicInstance>(null);
  useEffect(() => {
    if (!open || !cwd) return;
    const abort = new AbortController();
    setSnapshot(undefined);
    setError(undefined);
    void inspectBranches(cwd, abort.signal)
      .then((value) => {
        if (!abort.signal.aborted) setSnapshot(value);
      })
      .catch((cause) => {
        if (!abort.signal.aborted) setError(String(cause));
      });
    return () => abort.abort();
  }, [cwd, open, committing]);
  const items: MenuItem[] = [
    {
      id: 'changes',
      type: 'custom',
      content: (
        <div style={{ paddingTop: 6, paddingBottom: 6, gap: 6 }}>
          <text style={{ color: colors.text_tertiary, fontSize: sp(12.5) }}>
            {t('environment.changes')}
          </text>
          <div style={{ ...row, gap: 8 }}>
            <Icon name="git-branch" size={12} />
            <text style={{ ...ellipsis, flexGrow: 1, fontSize: sp(12.5), color: colors.text }}>
              {snapshot?.current ??
                (error
                  ? t('diff.unavailable')
                  : cwd && snapshot === undefined
                    ? t('diff.loading')
                    : t('environment.no_repository'))}
            </text>
            {snapshot && (
              <>
                <text style={{ fontSize: sp(12.5), color: colors.success }}>
                  {`+${snapshot.additions.toLocaleString()}`}
                </text>
                <text style={{ fontSize: sp(12.5), color: colors.danger }}>
                  {`−${snapshot.deletions.toLocaleString()}`}
                </text>
              </>
            )}
          </div>
          {error && <text style={{ color: colors.danger, fontSize: sp(12.5) }}>{error}</text>}
        </div>
      ),
    },
    { id: 'changes-separator', type: 'separator' },
    {
      id: 'commit',
      icon: 'git-commit-horizontal',
      label: t('environment.commit_or_push'),
      disabled: !snapshot,
      onSelect: () => runAction('git.commit'),
    },
    {
      id: 'compare',
      icon: 'git-branch',
      label: t('environment.compare_branch'),
      disabled: !snapshot,
      onSelect: () => runAction('git.compareBranch'),
    },
  ];
  if (operation) {
    items.push({
      id: 'commit-operation',
      type: 'custom',
      content: (
        <div style={{ ...row, gap: 8, paddingTop: 8, paddingBottom: 4 }}>
          <Spinner size={12} color={colors.text_secondary} />
          <text style={{ color: colors.text_secondary, fontSize: sp(12.5) }}>{operation}</text>
        </div>
      ),
    });
  }
  if (sessionId && view) {
    items.push({
      id: 'background-work',
      type: 'custom',
      content: (
        <BackgroundWork sessionId={sessionId} view={view} embedded registerActions={false} />
      ),
    });
  }
  return (
    <div style={{ ...row, position: 'relative', marginRight: 1 }}>
      <Tooltip content={t('environment.title')}>
        <Button
          ref={trigger}
          aria-label={t('environment.title')}
          aria-expanded={open}
          onMouseDown={() => setKeyboardOpen(false)}
          onKeyDown={(event) => {
            if (activation(event)) setKeyboardOpen(true);
          }}
          onClick={() => setOpen((value) => !value)}
          style={{
            width: 26,
            height: 26,
            flexShrink: 0,
            borderRadius: 6,
            cursor: 'default',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            backgroundColor: open ? colors.overlay : undefined,
            hover: { backgroundColor: colors.overlay },
            active: { backgroundColor: colors.overlay_strong },
          }}
        >
          <Icon name="info" size={15} color={colors.text_tertiary} />
        </Button>
      </Tooltip>
      {open && (
        <Menu
          items={items}
          autoHighlight={keyboardOpen}
          align="end"
          restoreFocusId={trigger.current?.id}
          ignoreOutsideId={trigger.current?.id}
          onClose={() => setOpen(false)}
          style={{ width: 320, maxHeight: 520, overflowY: 'scroll' }}
        />
      )}
    </div>
  );
}

import { useEffect, useRef, useState } from 'react';
import { DismissableLayer, type KeyEvent, type PublicInstance } from '@gpuix/react';
import { useTheme } from '../../theme';
import { t, useLocale } from '../../i18n';
import { Icon, MenuChip, TextField, row } from '../../ui';
import { inspectBranches, switchBranch, type BranchSnapshot } from '../../workspace/git';
import { PickerEmpty, PickerRow, PickerSeparator } from './index';

export interface BranchSelectorProps {
  workspacePath?: string;
  projectName?: string;
  busy?: boolean;
  planned?: boolean;
  branch?: string;
  onBranchChange?: (snapshot: BranchSnapshot | null, name: string) => void | Promise<void>;
  onError?: (message: string) => void;
  restoreFocusId?: number;
}
export function filterBranches(snapshot: BranchSnapshot, query: string) {
  const tokens = query
    .trim()
    .replace(/[A-Z]/g, (value) => value.toLowerCase())
    .split(/\s+/)
    .filter(Boolean);
  return snapshot.branches
    .filter(({ name }) =>
      tokens.every((token) =>
        name.replace(/[A-Z]/g, (value) => value.toLowerCase()).includes(token),
      ),
    )
    .sort(
      (a, b) =>
        Number(b.name === snapshot.current) - Number(a.name === snapshot.current) ||
        (a.name < b.name ? -1 : a.name > b.name ? 1 : 0),
    );
}
export function BranchSelector(props: BranchSelectorProps) {
  const { colors, sp } = useTheme();
  useLocale();
  const [snapshot, setSnapshot] = useState<BranchSnapshot | null>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [cursor, setCursor] = useState(-1);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('');
  const [pending, setPending] = useState(false);
  const input = useRef<PublicInstance>(null);
  const createInput = useRef<PublicInstance>(null);
  const rows = useRef(new Map<number, PublicInstance>());
  const operation = useRef(false);
  const generation = useRef(0);
  const path = props.workspacePath;
  useEffect(() => {
    const controller = new AbortController();
    const version = ++generation.current;
    setSnapshot(null);
    setOpen(false);
    if (path)
      void inspectBranches(path, controller.signal)
        .then((value) => {
          if (version === generation.current) setSnapshot(value);
        })
        .catch((error) => {
          if (!controller.signal.aborted)
            props.onError?.(t('shell.errors_inspect_branches', { error: String(error) }));
        });
    return () => controller.abort();
  }, [path]);
  useEffect(() => {
    rows.current.get(cursor)?.scrollIntoView?.();
  }, [cursor, query]);
  async function refresh() {
    if (!path) return;
    const version = generation.current;
    try {
      const value = await inspectBranches(path);
      if (version === generation.current) setSnapshot(value);
    } catch (error) {
      if (version === generation.current)
        props.onError?.(t('shell.errors_inspect_branches', { error: String(error) }));
    }
  }
  const selected = props.planned
    ? props.branch || snapshot?.defaultBranch || snapshot?.current
    : snapshot?.current || props.branch;
  const branches = snapshot ? filterBranches(snapshot, query) : [];
  const enabled = branches
    .map((branch, index) => (props.planned || !branch.checkedOutElsewhere ? index : -1))
    .filter((index) => index >= 0);
  if (!props.planned) enabled.push(branches.length);
  function begin() {
    if (props.busy || pending) return;
    setQuery('');
    setName('');
    setCursor(-1);
    setCreating(false);
    setOpen(true);
    void refresh();
  }
  async function change(branch: string, create = false) {
    if (!path || props.busy || operation.current || !branch.trim()) return;
    setOpen(false);
    if (!create && branch === selected) return;
    operation.current = true;
    setPending(true);
    const version = generation.current;
    try {
      const next = props.planned ? null : await switchBranch(path, branch, create);
      if (version === generation.current) {
        if (next) setSnapshot(next);
        await props.onBranchChange?.(next, branch);
      }
    } catch (error) {
      props.onError?.(
        t('errors.change_branch', {
          error: error instanceof Error ? error.message : String(error),
        }),
      );
    } finally {
      operation.current = false;
      setPending(false);
    }
  }
  function choose(index: number) {
    const branch = branches[index];
    if (branch) {
      if (props.planned || !branch.checkedOutElsewhere) void change(branch.name);
    } else if (!props.planned) {
      setCreating(true);
      setName('');
    }
  }
  function escape() {
    if (creating) {
      if (name) setName('');
      else {
        setCreating(false);
        setCursor(-1);
      }
    } else if (query) {
      setQuery('');
      setCursor(-1);
    } else setOpen(false);
  }
  function keys(event: KeyEvent) {
    if (event.modifiers?.ctrl || event.modifiers?.alt || event.modifiers?.cmd) return;
    if (event.key === 'escape') {
      event.preventDefault();
      event.stopPropagation();
      escape();
    } else if (event.key === 'enter') {
      event.preventDefault();
      event.stopPropagation();
      if (creating) {
        if (name.trim()) void change(name.trim(), true);
      } else {
        const at = enabled.includes(cursor) ? cursor : enabled[0];
        if (at !== undefined) choose(at);
      }
    } else if (!creating && (event.key === 'up' || event.key === 'down')) {
      event.preventDefault();
      event.stopPropagation();
      const back = event.key === 'up',
        at = enabled.indexOf(cursor);
      setCursor(
        at < 0
          ? ((back ? enabled.at(-1) : enabled[0]) ?? -1)
          : (enabled[(at + (back ? -1 : 1) + enabled.length) % enabled.length] ?? -1),
      );
    }
  }
  if (!path || !snapshot) return null;
  const label = pending
    ? t('branches.switching')
    : !props.planned && snapshot.detached
      ? t('shell.branches_detached_head_with_branch', { branch: snapshot.current })
      : selected || t('branches.detached_head');
  return (
    <div style={{ position: 'relative' }}>
      <MenuChip
        label={label}
        icon="git-branch"
        chevron={false}
        open={open}
        disabled={props.busy || pending}
        onClick={begin}
        style={{ maxWidth: 210 }}
      />
      {open && (
        <DismissableLayer
          initialFocus={() => (creating ? createInput : input).current?.id ?? null}
          finalFocus={(previous) => props.restoreFocusId ?? previous}
          onEscapeKeyDown={escape}
        >
          <anchored
            side="top"
            align="start"
            gap={4}
            offset={{ x: 1, y: 0 }}
            fit="switch"
            snapMargin={8}
            deferred
            priority={3}
            occlude
          >
            <div
              role="listbox"
              aria-label={t('branches.title')}
              onKeyDown={keys}
              onMouseDownOutside={(event) => {
                if (event.button === 0) setOpen(false);
              }}
              style={{
                // Keep fractional positioning on the surface, outside the snapped anchor.
                position: 'relative',
                top: creating ? -0.5 : 0,
                width: 360,
                maxHeight: 390,
                display: 'flex',
                flexDirection: 'column',
                overflow: 'hidden',
                borderRadius: 13,
                borderWidth: 1,
                borderColor: colors.border_strong,
                backgroundColor: colors.raised,
                boxShadow: {
                  offsetX: 0,
                  offsetY: 10,
                  blurRadius: 20,
                  spreadRadius: 0,
                  color: '#00000033',
                },
              }}
            >
              {creating ? (
                <div style={{ padding: 14, display: 'flex', flexDirection: 'column' }}>
                  <div style={{ ...row, gap: 8 }}>
                    <Icon name="plus" size={14} />
                    <text
                      style={{
                        fontSize: sp(13),
                        lineHeight: sp(16),
                        fontWeight: 500,
                        color: colors.text,
                      }}
                    >
                      {t('branches.create_and_checkout')}
                    </text>
                  </div>
                  <TextField
                    neutralFocus
                    key="create"
                    ref={createInput}
                    autoFocus
                    value={name}
                    onChange={setName}
                    onSubmit={() => {
                      if (name.trim()) void change(name.trim(), true);
                    }}
                    placeholder={t('input.new_branch_name')}
                    style={{
                      height: 36,
                      marginTop: 12,
                      paddingLeft: 10,
                      paddingRight: 10,
                      borderRadius: 9,
                      backgroundColor: colors.surface,
                      borderColor: colors.border_strong,
                    }}
                  />
                  <text
                    style={{
                      marginTop: 9,
                      fontSize: sp(12.5),
                      lineHeight: sp(16),
                      color: colors.text_tertiary,
                    }}
                  >
                    {t('branches.create_hint')}
                  </text>
                </div>
              ) : (
                <>
                  <div
                    style={{ paddingLeft: 12, paddingRight: 12, paddingTop: 10, paddingBottom: 8 }}
                  >
                    <TextField
                      neutralFocus
                      key="browse"
                      ref={input}
                      autoFocus
                      value={query}
                      onSubmit={() => {
                        const at = enabled.includes(cursor) ? cursor : enabled[0];
                        if (at !== undefined) choose(at);
                      }}
                      onChange={(value) => {
                        setQuery(value);
                        const matches = snapshot ? filterBranches(snapshot, value) : [];
                        setCursor(
                          value.trim()
                            ? matches.findIndex(
                                (branch) => props.planned || !branch.checkedOutElsewhere,
                              )
                            : -1,
                        );
                      }}
                      placeholder={t('branches.search_project', {
                        project: props.projectName || t('project.project_lower'),
                      })}
                      icon="search"
                      iconSize={15}
                      style={{
                        height: 34,
                        borderRadius: 9,
                        backgroundColor: colors.surface,
                        paddingLeft: 10,
                        paddingRight: 10,
                        gap: 8,
                      }}
                    />
                  </div>
                  <text
                    style={{
                      paddingLeft: 14,
                      paddingRight: 14,
                      paddingTop: 3,
                      paddingBottom: 7,
                      fontSize: sp(12.5),
                      fontWeight: 500,
                      color: colors.text_tertiary,
                    }}
                  >
                    {t('branches.title')}
                  </text>
                  <div
                    style={{
                      maxHeight: 260,
                      overflowY: 'scroll',
                      display: 'flex',
                      flexDirection: 'column',
                    }}
                  >
                    {branches.length ? (
                      branches.map((branch, index) => (
                        <PickerRow
                          key={branch.name}
                          iconColor={colors.text}
                          rowRef={(instance) => {
                            if (instance) rows.current.set(index, instance);
                            else rows.current.delete(index);
                          }}
                          label={branch.name}
                          icon="git-branch"
                          checked={branch.name === selected}
                          disabled={!props.planned && branch.checkedOutElsewhere}
                          highlighted={cursor === index}
                          onSelect={() => choose(index)}
                          onHover={() => setCursor(index)}
                        />
                      ))
                    ) : (
                      <PickerEmpty>{t('branches.none_found')}</PickerEmpty>
                    )}
                  </div>
                  {!props.planned && (
                    <>
                      <PickerSeparator />
                      <PickerRow
                        label={t('branches.create_and_checkout_ellipsis')}
                        icon="plus"
                        highlighted={cursor === branches.length}
                        onSelect={() => choose(branches.length)}
                        onHover={() => setCursor(branches.length)}
                      />
                      <div style={{ height: 4 }} />
                    </>
                  )}
                </>
              )}
            </div>
          </anchored>
        </DismissableLayer>
      )}
    </div>
  );
}

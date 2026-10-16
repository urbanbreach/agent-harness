import { useCallback, useEffect, useRef, useState } from 'react';
import { useGpuix, type PublicInstance, type StyleDesc } from '@gpuix/react';
import type { Methods } from '../../backend/protocol';
import { getAppStore, useStore } from '../../store';
import { CODE_FONT_FAMILY, useTheme } from '../../theme';
import {
  Button,
  Dropdown,
  Icon,
  TextField,
  useToast,
  type IconName,
  type MenuItem,
} from '../../ui';
import { bare, ellipsis, row } from '../../ui/controls';
import { Markdown } from '../../markdown';
import { nativeRenderer } from '../../native';
import { openPath, openUrl, revealPath } from '../../workspace/os';

type Catalog = Methods['skills.list']['result'];
type Skill = Catalog['skills'][number];
const column: StyleDesc = { display: 'flex', flexDirection: 'column', minWidth: 0 };
const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
function updatedLabel(value: number, now: number) {
  const seconds = Math.max(0, (now - value) / 1000);
  return seconds < 90
    ? 'Just now'
    : seconds < 3600
      ? `${Math.floor(seconds / 60)}m ago`
      : seconds < 86400
        ? `${Math.floor(seconds / 3600)}h ago`
        : `${Math.floor(seconds / 86400)}d ago`;
}

export function SkillsPage({ cwd }: { cwd?: string }) {
  const { colors, sp } = useTheme();
  const { renderer } = useGpuix();
  const { showToast } = useToast();
  const home = useStore((state) => state.initialized?.home);
  const mathEnabled = useStore((state) => state.preferences.renderMath !== false);
  const referenceNow = useStore((state) => state.preferences.referenceNow);
  const [catalog, setCatalog] = useState<Catalog>();
  const [query, setQuery] = useState('');
  const [source, setSource] = useState('');
  const [selection, setSelection] = useState<string>();
  const [armed, setArmed] = useState<string>();
  const [busy, setBusy] = useState<string>();
  const [copied, setCopied] = useState<string>();
  const [body, setBody] = useState<{ id: string; text: string }>();
  const [scanning, setScanning] = useState(false);
  const list = useRef<PublicInstance>(null);
  const detail = useRef<PublicInstance>(null);
  const copyTimer = useRef<NodeJS.Timeout | undefined>(undefined);
  const generation = useRef(0);
  const context = useRef(cwd);
  context.current = cwd;
  const scan = useCallback(async () => {
    const request = ++generation.current;
    setScanning(true);
    try {
      const result = await getAppStore().backend.call('skills.list', { cwd });
      if (request !== generation.current) return;
      setCatalog(result);
      if (result.errors.length) showToast(result.errors.join('\n'));
    } catch (error) {
      if (request === generation.current)
        showToast(`Couldn’t scan skill folders: ${message(error)}`);
    } finally {
      if (request === generation.current) setScanning(false);
    }
  }, [cwd, showToast]);
  useEffect(() => {
    setCatalog(undefined);
    setSelection(undefined);
    setArmed(undefined);
    setBusy(undefined);
    void scan();
    return () => {
      generation.current++;
    };
  }, [scan]);
  useEffect(() => () => clearTimeout(copyTimer.current), []);

  const skills = catalog?.skills ?? [];
  const search = query.trim().toLowerCase();
  const filtered = skills.filter(
    (skill) =>
      (!source || skill.installs.some((install) => install.source === source)) &&
      (!search ||
        [
          skill.name,
          skill.description,
          skill.project?.split('/').filter(Boolean).pop(),
          ...skill.installs.map((install) => install.source),
        ].some((value) => value?.toLowerCase().includes(search))),
  );
  const selected = filtered.find((skill) => skill.id === selection) ?? filtered[0];
  const selectedId = selected?.id;
  useEffect(() => {
    setArmed(undefined);
    setCopied(undefined);
    setBody(undefined);
    if (detail.current) renderer?.scrollTo?.(detail.current.id, 0, 0);
  }, [selectedId, renderer]);
  useEffect(() => {
    if (!selectedId) return;
    let active = true;
    void getAppStore()
      .backend.call('skills.read', { cwd, id: selectedId })
      .then((result) => {
        if (active) setBody({ id: selectedId, text: result.body });
      })
      .catch((error) => {
        if (active) showToast(`Couldn’t read the skill: ${message(error)}`);
      });
    return () => {
      active = false;
    };
  }, [cwd, selectedId, catalog, showToast]);

  const groups = new Map<string, Skill[]>();
  for (const skill of filtered) {
    const label =
      skill.scope === 'user'
        ? 'User'
        : skill.project?.split('/').filter(Boolean).pop() || 'Project';
    const group = groups.get(label) ?? [];
    group.push(skill);
    groups.set(label, group);
  }
  const entries: ({ heading: string; count: number } | { skill: Skill })[] = [];
  for (const [heading, group] of groups) {
    entries.push({ heading, count: group.length }, ...group.map((skill) => ({ skill })));
  }
  function select(id: string) {
    setSelection(id);
    setArmed(undefined);
  }
  async function toggle(skill: Skill) {
    if (busy) return;
    const requestCwd = cwd;
    setBusy(skill.id);
    setArmed(undefined);
    setCatalog(
      (current) =>
        current && {
          ...current,
          skills: current.skills.map((item) =>
            item.id === skill.id
              ? {
                  ...item,
                  enabled: !skill.enabled,
                  installs: item.installs.map((install) => ({
                    ...install,
                    enabled: !skill.enabled,
                  })),
                }
              : item,
          ),
        },
    );
    try {
      await getAppStore().backend.call('skills.setEnabled', {
        cwd,
        id: skill.id,
        enabled: !skill.enabled,
      });
    } catch (error) {
      if (context.current === requestCwd) setCatalog(catalog);
      showToast(`Couldn’t update the skill: ${message(error)}`);
    } finally {
      if (context.current === requestCwd) {
        setBusy(undefined);
        await scan();
      }
    }
  }
  async function trash(skill: Skill) {
    if (busy) return;
    if (armed !== skill.id) {
      setArmed(skill.id);
      return;
    }
    const requestCwd = cwd;
    setBusy(skill.id);
    setArmed(undefined);
    setSelection(undefined);
    setCatalog(
      (current) =>
        current && { ...current, skills: current.skills.filter((item) => item.id !== skill.id) },
    );
    try {
      await getAppStore().backend.call('skills.trash', { cwd, id: skill.id });
      showToast(`Moved “${skill.name}” to the Trash`, 'success');
    } catch (error) {
      if (context.current === requestCwd) setCatalog(catalog);
      showToast(`Couldn’t delete the skill: ${message(error)}`);
    } finally {
      if (context.current === requestCwd) {
        setBusy(undefined);
        await scan();
      }
    }
  }
  async function pathAction(skill: Skill, action: 'open' | 'reveal') {
    setArmed(undefined);
    try {
      if (action === 'open') await openPath(skill.primaryFile);
      else await revealPath(skill.primaryDirectory);
    } catch (error) {
      showToast(message(error));
    }
  }
  function copy(skill: Skill) {
    setArmed(undefined);
    try {
      if (!renderer) throw new Error('Clipboard is unavailable');
      nativeRenderer(renderer).writeClipboardText(skill.primaryDirectory);
      setCopied(skill.id);
      clearTimeout(copyTimer.current);
      copyTimer.current = setTimeout(() => setCopied(undefined), 2000);
    } catch (error) {
      showToast(`Couldn’t copy the path: ${message(error)}`);
    }
  }
  const selectedSources = selected?.installs.map((install) => install.source).join(' · ');
  const textStyle: StyleDesc = {
    color: colors.text_secondary,
    fontSize: sp(12.5),
    lineHeight: sp(17),
  };
  function metadata(label: string, value: string, mono = false, last = false) {
    return (
      <div
        key={`${label}:${value}`}
        style={{
          ...row,
          paddingTop: 8,
          paddingBottom: 8,
          gap: 12,
          borderBottomWidth: last ? 0 : 1,
          borderColor: colors.border,
        }}
      >
        <text style={{ width: 84, flexShrink: 0, color: colors.text_tertiary, fontSize: sp(12.5) }}>
          {label}
        </text>
        <text
          style={{
            ...textStyle,
            ...ellipsis,
            flexGrow: 1,
            fontFamily: mono ? CODE_FONT_FAMILY : undefined,
            fontSize: mono ? 12.5 : sp(12.5),
          }}
        >
          {value}
        </text>
      </div>
    );
  }
  function action(
    label: string,
    icon: IconName,
    onClick: () => void,
    danger = false,
    props: { onMouseDownOutside?: () => void } = {},
  ) {
    return (
      <div
        {...props}
        tabIndex={busy ? -1 : 0}
        role="button"
        aria-label={label}
        onClick={() => {
          if (!busy) onClick();
        }}
        style={{
          ...row,
          height: 26,
          paddingLeft: 10,
          paddingRight: 10,
          borderWidth: 1,
          borderColor: danger ? colors.danger : colors.border_strong,
          color: danger ? colors.danger : colors.text_secondary,
          borderRadius: 6,
          gap: 5,
          cursor: 'default',
          opacity: busy ? 0.55 : 1,
          backgroundColor: danger ? `${colors.danger}1f` : undefined,
          hover: { backgroundColor: colors.overlay },
          focusVisible: { borderColor: colors.accent },
        }}
      >
        <Icon name={icon} size={11} color={danger ? colors.danger : colors.text_tertiary} />
        <text style={{ fontSize: sp(12.5) }}>{label}</text>
      </div>
    );
  }
  function tile(skill: Skill, large = false) {
    return (
      <div
        style={{
          ...row,
          width: large ? 38 : 26,
          height: large ? 38 : 26,
          flexShrink: 0,
          borderRadius: large ? 9 : 6,
          justifyContent: 'center',
          backgroundColor: colors.overlay,
          opacity: skill.enabled ? 1 : 0.45,
        }}
      >
        <Icon name="package" size={large ? 18 : 13} color={colors.text_secondary} />
      </div>
    );
  }
  const shell: StyleDesc = {
    ...row,
    alignItems: 'stretch',
    flexGrow: 1,
    flexBasis: 0,
    alignSelf: 'stretch',
    minHeight: 0,
    minWidth: 0,
    backgroundColor: colors.surface,
    borderLeftWidth: 1,
    borderColor: colors.sidebar_border,
  };
  if (!catalog)
    return (
      <div style={{ ...shell, paddingTop: 12, alignItems: 'flex-start' }}>
        <text
          style={{
            ...textStyle,
            color: colors.text_tertiary,
            paddingLeft: 18,
            paddingRight: 18,
            paddingTop: 16,
            paddingBottom: 16,
          }}
        >
          Scanning skill folders…
        </text>
        {!scanning && (
          <Button
            aria-label="Rescan skill folders"
            onClick={() => void scan()}
            style={{ marginTop: 12, width: 28, height: 28 }}
          >
            <Icon name="rotate-cw" />
          </Button>
        )}
      </div>
    );
  if (!skills.length)
    return (
      <div
        style={{
          ...shell,
          ...column,
          padding: 40,
          alignItems: 'center',
          justifyContent: 'center',
          gap: 10,
        }}
      >
        <div
          style={{
            ...row,
            width: 44,
            height: 44,
            borderRadius: 11,
            backgroundColor: colors.overlay,
            justifyContent: 'center',
          }}
        >
          <Icon name="package" size={21} />
        </div>
        <text style={{ color: colors.text, fontSize: sp(13), fontWeight: 500 }}>No skills yet</text>
        <text style={{ ...textStyle, maxWidth: 420, textAlign: 'center' }}>
          A skill is a folder with a SKILL.md — reusable instructions any agent can load, like a
          deploy runbook or a review checklist. Skills installed for Claude Code, Codex, and every
          other agent show up here, invoked as /name.
        </text>
        <Button
          disabled={scanning}
          onClick={() => void scan()}
          style={{ height: 26, paddingLeft: 10, paddingRight: 10, borderRadius: 6 }}
        >
          <Icon name="rotate-cw" size={11} />
          <text style={{ ...textStyle, marginLeft: 5 }}>Rescan skill folders</text>
        </Button>
      </div>
    );
  const disabled = skills.filter((skill) => !skill.enabled).length;
  const filterSources = [
    ...new Set(skills.flatMap((skill) => skill.installs.map((install) => install.source))),
  ];
  return (
    <div style={shell}>
      <div
        style={{
          ...column,
          width: 264,
          flexShrink: 0,
          minHeight: 0,
          borderRightWidth: 1,
          borderColor: colors.border,
        }}
      >
        <div
          style={{
            ...column,
            paddingLeft: 10,
            paddingRight: 10,
            paddingTop: 22,
            paddingBottom: 8,
            gap: 7,
          }}
        >
          <TextField
            icon="search"
            placeholder="Search skills…"
            value={query}
            onChange={(value) => {
              setQuery(value);
              setArmed(undefined);
            }}
            onKeyDown={(event) => {
              if (!bare(event) || !['up', 'down'].includes(event.key ?? '') || !filtered.length)
                return;
              event.preventDefault();
              event.stopPropagation();
              const at = filtered.findIndex((skill) => skill.id === selectedId);
              const next =
                filtered[
                  Math.max(0, Math.min(filtered.length - 1, at + (event.key === 'up' ? -1 : 1)))
                ];
              if (!next) return;
              select(next.id);
              const index = entries.findIndex(
                (entry) => 'skill' in entry && entry.skill.id === next.id,
              );
              if (list.current) renderer?.scrollToItem?.(list.current.id, index);
            }}
          />
          <Dropdown
            outlined
            icon="package"
            label={source || 'All skills'}
            style={{ width: '100%', height: 26, justifyContent: 'space-between' }}
            items={[
              {
                id: 'all',
                label: 'All skills',
                selected: !source,
                onSelect: () => {
                  setSource('');
                  setArmed(undefined);
                },
              },
              { id: 'separator', type: 'separator' },
              ...filterSources.map<MenuItem>((name) => {
                const count = skills.filter((skill) =>
                  skill.installs.some((install) => install.source === name),
                ).length;
                return {
                  id: name,
                  label: `${name}${count ? ` · ${count}` : ''}`,
                  icon: 'package',
                  selected: source === name,
                  onSelect: () => {
                    setSource(name);
                    setArmed(undefined);
                  },
                };
              }),
            ]}
          />
        </div>
        {entries.length ? (
          <virtual-list
            ref={list}
            estimatedItemHeight={51}
            style={{ flexGrow: 1, minHeight: 0, paddingLeft: 8, paddingRight: 8 }}
          >
            {entries.map((entry, index) =>
              'heading' in entry ? (
                <div
                  key={`section:${entry.heading}`}
                  style={{
                    ...row,
                    alignItems: 'baseline',
                    gap: 6,
                    paddingLeft: 9,
                    paddingRight: 9,
                    paddingTop: index ? 18 : 10,
                    paddingBottom: 4,
                  }}
                >
                  <text
                    style={{
                      ...ellipsis,
                      color: colors.text_tertiary,
                      fontSize: sp(12.5),
                      fontWeight: 600,
                    }}
                  >
                    {entry.heading.toUpperCase()}
                  </text>
                  <text style={{ color: colors.text_ghost, fontSize: sp(12.5) }}>
                    {entry.count}
                  </text>
                </div>
              ) : (
                <div key={entry.skill.id} style={{ paddingBottom: 1 }}>
                  <div
                    tabIndex={0}
                    role="option"
                    aria-selected={entry.skill.id === selectedId}
                    aria-label={entry.skill.name}
                    onClick={() => select(entry.skill.id)}
                    style={{
                      ...row,
                      paddingLeft: 8,
                      paddingRight: 8,
                      paddingTop: 6,
                      paddingBottom: 6,
                      borderRadius: 8,
                      borderWidth: 1,
                      borderColor: 'transparent',
                      gap: 9,
                      cursor: 'default',
                      backgroundColor:
                        entry.skill.id === selectedId ? colors.sidebar_item_background : undefined,
                      hover: {
                        backgroundColor:
                          entry.skill.id === selectedId
                            ? colors.sidebar_item_background
                            : colors.overlay,
                      },
                      focusVisible: { borderColor: colors.accent },
                    }}
                  >
                    {tile(entry.skill)}
                    <div style={{ ...column, flexGrow: 1 }}>
                      <div style={{ ...row, gap: 6 }}>
                        <text
                          style={{
                            ...ellipsis,
                            flexGrow: 1,
                            fontSize: sp(12.5),
                            fontWeight: 500,
                            color: entry.skill.enabled ? colors.text : colors.text_secondary,
                          }}
                        >
                          {entry.skill.name}
                        </text>
                        {!entry.skill.enabled && (
                          <text
                            style={{ color: colors.warning, fontSize: sp(12.5), flexShrink: 0 }}
                          >
                            Disabled
                          </text>
                        )}
                      </div>
                      <text
                        style={{
                          ...ellipsis,
                          marginTop: 1,
                          color: colors.text_tertiary,
                          fontSize: sp(12.5),
                        }}
                      >
                        {entry.skill.description ||
                          entry.skill.installs.map((install) => install.source).join(' · ')}
                      </text>
                    </div>
                  </div>
                </div>
              ),
            )}
          </virtual-list>
        ) : (
          <div style={{ ...column, flexGrow: 1, padding: 18 }}>
            <text style={{ ...textStyle, color: colors.text_tertiary }}>
              No skills match your search
            </text>
          </div>
        )}
        <div
          style={{
            ...row,
            justifyContent: 'center',
            height: 26,
            flexShrink: 0,
            paddingLeft: 12,
            paddingRight: 12,
            borderTopWidth: 1,
            borderColor: colors.border,
          }}
        >
          <text style={{ color: colors.text_ghost, fontSize: sp(12.5) }}>
            {search || source
              ? `${filtered.length} of ${skills.length} shown`
              : `${skills.length} ${skills.length === 1 ? 'skill' : 'skills'}${disabled ? ` · ${disabled} disabled` : ''}`}
          </text>
        </div>
      </div>
      {selected ? (
        <div
          key={selected.id}
          ref={detail}
          style={{
            ...column,
            flexGrow: 1,
            flexBasis: 0,
            minHeight: 0,
            overflow: 'scroll',
            paddingLeft: 24,
            paddingRight: 24,
            paddingTop: 18,
            paddingBottom: 20,
          }}
        >
          <div style={{ ...row, gap: 12 }}>
            {tile(selected, true)}
            <div style={{ ...column, flexGrow: 1 }}>
              <div style={{ ...row, gap: 7 }}>
                <text
                  style={{ ...ellipsis, fontSize: sp(15), fontWeight: 500, color: colors.text }}
                >
                  {selected.name}
                </text>
                {!selected.enabled && (
                  <text style={{ color: colors.warning, fontSize: sp(12.5) }}>Disabled</text>
                )}
              </div>
              <text
                style={{
                  ...ellipsis,
                  marginTop: 2,
                  color: colors.text_tertiary,
                  fontSize: sp(12.5),
                }}
              >
                {`${selectedSources} · ${selected.scope === 'user' ? 'available in every project' : `in ${selected.project?.split('/').filter(Boolean).pop() || 'this project'}`}`}
              </text>
            </div>
            <div
              tabIndex={busy ? -1 : 0}
              role="switch"
              aria-label={`${selected.enabled ? 'Disable' : 'Enable'} ${selected.name}`}
              aria-valuetext={selected.enabled ? 'On' : 'Off'}
              onClick={() => void toggle(selected)}
              style={{
                ...row,
                width: 36,
                height: 20,
                padding: 2,
                borderWidth: 1,
                borderRadius: 20,
                flexShrink: 0,
                justifyContent: selected.enabled ? 'flex-end' : 'flex-start',
                backgroundColor: selected.enabled ? colors.inverse : colors.inset,
                borderColor: selected.enabled ? colors.inverse : colors.border_strong,
                opacity: busy ? 0.55 : 1,
                focusVisible: { borderColor: colors.accent },
              }}
            >
              <div
                style={{
                  width: 14,
                  height: 14,
                  borderRadius: 14,
                  backgroundColor: selected.enabled ? colors.on_inverse : colors.text_tertiary,
                }}
              />
            </div>
          </div>
          <text style={{ ...textStyle, marginTop: 14 }}>
            {selected.description || 'No description'}
          </text>
          <div style={{ ...column, marginTop: 16 }}>
            {metadata('Invoke', `/${selected.name}`, true)}
            {selected.installs.map((install) =>
              metadata(
                selected.installs.length === 1 ? 'Location' : install.source,
                home && (install.directory === home || install.directory.startsWith(`${home}/`))
                  ? `~${install.directory.slice(home.length)}`
                  : install.directory,
                true,
              ),
            )}
            {metadata(
              'Contents',
              `${selected.supportingFiles ? `${selected.supportingFiles} supporting ${selected.supportingFiles === 1 ? 'file' : 'files'} · ` : ''}${selected.sizeBytes < 1024 ? `${selected.sizeBytes} B` : selected.sizeBytes < 1024 * 1024 ? `${(selected.sizeBytes / 1024).toFixed(1)} KB` : `${(selected.sizeBytes / (1024 * 1024)).toFixed(1)} MB`}`,
              false,
              selected.updatedAt === null && !selected.allowedTools.length,
            )}
            {selected.updatedAt !== null &&
              metadata(
                'Updated',
                updatedLabel(
                  selected.updatedAt,
                  typeof referenceNow === 'number' ? referenceNow : Date.now(),
                ),
                false,
                !selected.allowedTools.length,
              )}
            {selected.allowedTools.length > 0 &&
              metadata('Tools', selected.allowedTools.join(', '), true, true)}
          </div>
          {selected.installs.length > 1 && (
            <div style={{ ...row, marginTop: 12, gap: 6 }}>
              <Icon name="alert" size={11} color={colors.warning} />
              <text style={{ ...textStyle, color: colors.warning }}>
                {`Same name in ${selected.installs.length - 1} other ${selected.installs.length === 2 ? 'location' : 'locations'} — the most specific copy wins`}
              </text>
            </div>
          )}
          <div style={{ ...row, flexWrap: 'wrap', marginTop: 16, gap: 7 }}>
            {action('Open SKILL.md', 'pencil', () => void pathAction(selected, 'open'))}
            {action('Show in File Manager', 'folder', () => void pathAction(selected, 'reveal'))}
            {action(
              copied === selected.id ? 'Copied' : 'Copy Path',
              copied === selected.id ? 'check' : 'copy',
              () => copy(selected),
            )}
            <div style={{ flexGrow: 1 }} />
            {action(
              armed === selected.id ? 'Confirm delete' : 'Delete',
              'trash',
              () => void trash(selected),
              armed === selected.id,
              { onMouseDownOutside: () => setArmed(undefined) },
            )}
          </div>
          {(body?.id === selected.id ? body.text : selected.body)?.trim() && (
            <div
              style={{
                ...column,
                marginTop: 18,
                paddingTop: 14,
                borderTopWidth: 1,
                borderColor: colors.border,
              }}
            >
              <text
                style={{
                  fontFamily: CODE_FONT_FAMILY,
                  fontSize: sp(12.5),
                  color: colors.text_ghost,
                }}
              >
                SKILL.md
              </text>
              <Markdown
                source={body?.id === selected.id ? body.text : selected.body}
                context="compact"
                mathEnabled={mathEnabled}
                onOpenLink={(url) => {
                  void openUrl(url).catch((error) => showToast(message(error)));
                }}
                style={{ marginTop: 10, color: colors.text_secondary }}
              />
            </div>
          )}
        </div>
      ) : (
        <div
          style={{ ...column, flexGrow: 1, alignItems: 'center', justifyContent: 'center', gap: 8 }}
        >
          <Icon name="package" size={22} color={colors.text_ghost} />
          <text style={{ fontSize: sp(12.5), color: colors.text_ghost }}>Select a skill</text>
        </div>
      )}
    </div>
  );
}

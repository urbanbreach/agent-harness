import { useEffect, useMemo, useRef, useState } from 'react';
import {
  DismissableLayer,
  flushSync,
  useGpuix,
  type KeyEvent,
  type PublicInstance,
} from '@gpuix/react';
import type { AccessMode, ModelEntry, ModelSelection } from '../../backend/protocol';
import { getAppStore, useModels, useStore } from '../../store';
import { useTheme } from '../../theme';
import { providerColor } from '../../theme/colors';
import { Button, Icon, IconButton, MenuChip, TextField, ellipsis, row } from '../../ui/controls';
import { Dropdown, type MenuItem } from '../../ui/menus';
import { useToast } from '../../ui/overlays';
import { registerAction, runAction } from '../actions';

const accessChoices: { mode: AccessMode; name: string; description: string; icon: string }[] = [
  {
    mode: 'supervised',
    name: 'Supervised',
    description: 'Ask before commands and file changes',
    icon: 'lock',
  },
  {
    mode: 'autoEdits',
    name: 'Auto-accept edits',
    description: 'Auto-approve edits, ask before other actions',
    icon: 'pencil',
  },
  {
    mode: 'auto',
    name: 'Auto',
    description: 'An AI reviewer approves routine actions; risky ones still ask',
    icon: 'sparkle',
  },
  {
    mode: 'full',
    name: 'Full access',
    description: 'Allow commands and edits without prompts',
    icon: 'lock-open',
  },
];
const modelKey = (entry: ModelEntry) =>
  JSON.stringify([entry.profile, entry.provider, entry.model]);
const selectionFor = (entry: ModelEntry): ModelSelection => ({
  profile: entry.profile,
  model: entry.model,
  variant: entry.variant,
});
function providerIcon(provider: string) {
  const name = provider.toLowerCase();
  if (['openai', 'codex'].includes(name)) return 'provider-openai';
  if (name === 'anthropic') return 'provider-claude';
  return 'bot';
}

export function ModelControls({
  cwd,
  sessionId,
  selection,
  onSelection,
  accessMode,
  onAccessMode,
}: {
  cwd: string;
  sessionId?: string;
  selection?: ModelSelection;
  onSelection: (selection: ModelSelection) => void;
  accessMode: AccessMode;
  onAccessMode: (mode: AccessMode) => void;
}) {
  const { colors, sp } = useTheme();
  const { renderer } = useGpuix();
  const { showToast } = useToast();
  const catalog = useModels(cwd);
  const preferences = useStore((state) => state.preferences);
  const [open, setOpen] = useState(false);
  const [profileOpen, setProfileOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [query, setQuery] = useState('');
  const [tab, setTab] = useState('');
  const [highlight, setHighlight] = useState(-1);
  const trigger = useRef<PublicInstance>(null);
  const input = useRef<PublicInstance>(null);
  const settings = useRef<PublicInstance>(null);
  const rows = useRef(new Map<string, PublicInstance>());
  const current =
    catalog?.models.find(
      (entry) =>
        entry.profile === selection?.profile &&
        (!selection.model || entry.model === selection.model) &&
        entry.variant === selection.variant,
    ) ??
    catalog?.models.find(
      (entry) =>
        entry.profile === selection?.profile &&
        (!selection.model || entry.model === selection.model),
    ) ??
    catalog?.current ??
    undefined;
  const entries = useMemo(() => {
    const seen = new Set<string>();
    return [...(catalog?.models ?? []), ...(catalog?.current ? [catalog.current] : [])].filter(
      (entry) => {
        const key = modelKey(entry);
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
      },
    );
  }, [catalog]);
  const providers = [...new Set((catalog?.models ?? []).map((entry) => entry.provider))];
  const allowed = (provider: string) => !sessionId || !current || provider === current.provider;
  const usable = providers.filter(allowed);
  const favorites = Array.isArray(preferences.favoriteModels)
    ? preferences.favoriteModels.filter((value): value is string => typeof value === 'string')
    : [];
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const visible = entries
    .filter((entry) => allowed(entry.provider))
    .filter((entry) => {
      if (terms.length) {
        const text = [
          entry.label,
          entry.modelLabel,
          entry.model,
          entry.providerLabel,
          entry.provider,
          entry.profile,
        ]
          .join(' ')
          .toLowerCase();
        return terms.every((term) => text.includes(term));
      }
      return tab === 'favorites' ? favorites.includes(modelKey(entry)) : entry.provider === tab;
    });
  if (tab === 'favorites' && !terms.length)
    visible.sort((a, b) => favorites.indexOf(modelKey(a)) - favorites.indexOf(modelKey(b)));
  const visibleKey = visible.map(modelKey).join('\n');

  async function load(refresh = false) {
    setLoading(true);
    try {
      await getAppStore().loadModels(cwd, refresh);
    } catch (error) {
      showToast(`Could not load models: ${String(error)}`);
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => {
    void load();
  }, [cwd]);
  function changeOpen(value: boolean) {
    setOpen(value);
    if (value) {
      setQuery('');
      setHighlight(-1);
      setTab(current && usable.includes(current.provider) ? current.provider : (usable[0] ?? ''));
      void load(true);
    } else runAction('composer.focus');
  }
  useEffect(
    () =>
      registerAction({
        id: 'composer.modelPicker',
        label: 'action.modelPicker',
        run: () => changeOpen(!open),
      }),
    [open, cwd, current, usable.join('\n')],
  );
  useEffect(
    () =>
      registerAction({
        id: 'composer.profilePicker',
        label: 'Agents',
        run: () => setProfileOpen(true),
      }),
    [],
  );
  useEffect(() => {
    if (!open) return;
    if (highlight >= 0 && visible[highlight])
      rows.current.get(modelKey(visible[highlight]!))?.scrollIntoView?.();
    else if (!query && current) rows.current.get(modelKey(current))?.scrollIntoView?.();
  }, [open, highlight, visibleKey]);
  useEffect(() => {
    if (open && !tab && usable.length) setTab(usable[0]!);
  }, [open, tab, usable.join('\n')]);
  function choose(entry: ModelEntry) {
    onSelection(selectionFor(entry));
    changeOpen(false);
  }
  function keys(event: KeyEvent) {
    const key = event.key;
    const modifiers = event.modifiers;
    if (modifiers?.ctrl || modifiers?.cmd || modifiers?.alt) return;
    if (key === 'escape') {
      event.preventDefault();
      event.stopPropagation();
      changeOpen(false);
    }
    if ((key === 'up' || key === 'down') && !modifiers?.shift) {
      event.preventDefault();
      event.stopPropagation();
      setHighlight((at) =>
        visible.length
          ? at < 0
            ? key === 'up'
              ? visible.length - 1
              : 0
            : (at + (key === 'up' ? -1 : 1) + visible.length) % visible.length
          : -1,
      );
    }
    if (key === 'enter' && !modifiers?.shift) {
      event.preventDefault();
      event.stopPropagation();
      const entry = visible[highlight < 0 ? 0 : highlight];
      if (entry) choose(entry);
    }
    if (key === 'tab' && !query.trim()) {
      event.preventDefault();
      event.stopPropagation();
      const tabs = ['favorites', ...usable];
      const next =
        tabs[(tabs.indexOf(tab) + (modifiers?.shift ? -1 : 1) + tabs.length) % tabs.length]!;
      setTab(next);
      setHighlight(-1);
    }
  }
  function toggleFavorite(entry: ModelEntry) {
    const key = modelKey(entry);
    setHighlight(-1);
    getAppStore().setPreferences({
      favoriteModels: favorites.includes(key)
        ? favorites.filter((value) => value !== key)
        : [...favorites, key],
    });
  }
  const variants = (catalog?.models ?? []).filter(
    (entry) =>
      entry.profile === current?.profile &&
      entry.model === current?.model &&
      entry.provider === current?.provider,
  );
  const traitItems: MenuItem[] = [];
  const choices = variants.filter(
    (entry, index) =>
      (entry.variant || entry.reasoningEffort) &&
      variants.findIndex(
        (other) =>
          (other.variant ?? other.reasoningEffort) === (entry.variant ?? entry.reasoningEffort),
      ) === index,
  );
  if (choices.length) {
    traitItems.push({ id: 'reasoning', type: 'caption', label: 'Reasoning' });
    for (const entry of choices)
      traitItems.push({
        id: `variant-${entry.variant ?? entry.reasoningEffort}`,
        label: entry.variantLabel ?? entry.reasoningEffort ?? entry.variant,
        selected: entry.variant === current?.variant,
        onSelect: () => onSelection(selectionFor(entry)),
        content: (
          <div style={{ ...row, width: 190, paddingTop: 2, paddingBottom: 2, gap: 10 }}>
            <text style={{ ...ellipsis, flexGrow: 1, color: colors.text_secondary }}>
              {entry.variantLabel ?? entry.reasoningEffort ?? entry.variant}
            </text>
            {entry.variant === catalog?.current?.variant &&
              entry.model === catalog?.current?.model && (
                <text
                  style={{
                    height: 18,
                    paddingLeft: 5,
                    paddingRight: 5,
                    borderRadius: 4,
                    borderWidth: 1,
                    borderColor: colors.border_strong,
                    backgroundColor: colors.overlay,
                    color: colors.text_tertiary,
                    fontWeight: 600,
                  }}
                >
                  Default
                </text>
              )}
            {entry.variant === current?.variant && <Icon name="check" size={11} />}
          </div>
        ),
      });
  }
  const windows = variants.filter(
    (entry, index) =>
      entry.contextWindow &&
      variants.findIndex((other) => other.contextWindow === entry.contextWindow) === index,
  );
  if (windows.length > 1) {
    if (traitItems.length) traitItems.push({ id: 'window-divider', type: 'separator' });
    traitItems.push({ id: 'windows', type: 'caption', label: 'Context Window' });
    for (const entry of windows)
      traitItems.push({
        id: `window-${entry.contextWindow}`,
        label: `${entry.contextWindow! / 1000}K`,
        selected: entry.contextWindow === current?.contextWindow,
        onSelect: () => onSelection(selectionFor(entry)),
      });
  }
  const mode = accessChoices.find((choice) => choice.mode === accessMode)!;
  return (
    <div style={{ ...row, gap: 1, minWidth: 0 }}>
      <div style={{ position: 'relative', ...row }}>
        <MenuChip
          ref={trigger}
          chevron={false}
          open={open}
          onClick={() => changeOpen(!open)}
          icon={usable.length ? providerIcon(current?.provider ?? usable[0]!) : 'alert'}
          label={
            usable.length
              ? (current?.modelLabel ?? current?.label ?? 'Choose model')
              : loading
                ? 'Loading models…'
                : 'No providers'
          }
          style={{ maxWidth: 250 }}
        />
        {open && (
          <DismissableLayer
            initialFocus={() => (usable.length ? input.current : settings.current)?.id ?? null}
            finalFocus={() => {
              runAction('composer.focus');
              return null;
            }}
            onEscapeKeyDown={() => changeOpen(false)}
          >
            <anchored
              side="top"
              align="start"
              gap={4}
              fit="switch"
              snapMargin={8}
              deferred
              priority={2}
              occlude
            >
              <div
                onKeyDown={keys}
                onMouseDownOutside={(event) => {
                  const bounds =
                    trigger.current && renderer?.getElementBounds?.(trigger.current.id);
                  if (
                    bounds &&
                    event.x !== undefined &&
                    event.y !== undefined &&
                    event.x >= bounds.x &&
                    event.x <= bounds.x + bounds.width &&
                    event.y >= bounds.y &&
                    event.y <= bounds.y + bounds.height
                  )
                    return;
                  if (event.button === 0) changeOpen(false);
                }}
                style={{
                  ...row,
                  alignItems: 'stretch',
                  width: usable.length ? 460 : 320,
                  height: usable.length ? 390 : undefined,
                  borderRadius: 13,
                  overflow: 'hidden',
                  borderWidth: 1,
                  borderColor: colors.border_strong,
                  backgroundColor: colors.raised,
                  boxShadow: {
                    offsetX: 0,
                    offsetY: 10,
                    blurRadius: 24,
                    spreadRadius: -3,
                    color: '#00000030',
                  },
                }}
              >
                {!usable.length ? (
                  <div
                    style={{
                      display: 'flex',
                      flexDirection: 'column',
                      alignItems: 'center',
                      width: '100%',
                      minWidth: 0,
                      paddingLeft: 24,
                      paddingRight: 24,
                      paddingTop: 20,
                      paddingBottom: 20,
                      gap: 9,
                    }}
                  >
                    <div
                      style={{
                        ...row,
                        justifyContent: 'center',
                        width: 40,
                        height: 40,
                        borderRadius: 10,
                        backgroundColor: colors.overlay,
                      }}
                    >
                      <Icon name="alert" size={19} color={colors.warning} />
                    </div>
                    <text style={{ color: colors.text, fontSize: sp(12.5), fontWeight: 500 }}>
                      No providers available
                    </text>
                    <text
                      style={{
                        color: colors.text_secondary,
                        fontSize: sp(12.5),
                        lineHeight: sp(17),
                        textAlign: 'center',
                        width: '100%',
                        whiteSpace: 'normal',
                      }}
                    >
                      Install an agent CLI, or switch one back on in Provider settings.
                    </text>
                    <Button
                      ref={settings}
                      variant="secondary"
                      onClick={() => {
                        flushSync(() => changeOpen(false));
                        if (!runAction('settings.providers'))
                          showToast('Provider settings are unavailable');
                      }}
                      style={{
                        height: 28,
                        paddingLeft: 11,
                        paddingRight: 11,
                        borderRadius: 7,
                        borderColor: colors.border_strong,
                        gap: 6,
                        marginTop: 3,
                      }}
                    >
                      <Icon name="settings" size={11} />
                      <text style={{ fontSize: sp(12.5) }}>Open Provider Settings</text>
                    </Button>
                  </div>
                ) : (
                  <>
                    <div
                      style={{
                        display: 'flex',
                        flexDirection: 'column',
                        width: 50,
                        flexShrink: 0,
                        padding: 5,
                        gap: 4,
                        backgroundColor: colors.canvas,
                        borderRightWidth: 1,
                        borderColor: colors.border,
                        overflowY: 'scroll',
                      }}
                    >
                      <IconButton
                        name="star"
                        size={17}
                        label="Favorites"
                        tabIndex={-1}
                        onClick={() => {
                          setTab('favorites');
                          setHighlight(-1);
                          renderer?.focusElement?.(input.current?.id ?? 0);
                        }}
                        style={{
                          width: 38,
                          height: 38,
                          borderRadius: 7,
                          backgroundColor: tab === 'favorites' ? colors.overlay_strong : undefined,
                        }}
                      />
                      <div
                        style={{
                          height: 1,
                          flexShrink: 0,
                          width: 34,
                          marginTop: 3,
                          marginBottom: 3,
                          backgroundColor: colors.border,
                        }}
                      />
                      {providers.map((provider) => (
                        <IconButton
                          key={provider}
                          name={providerIcon(provider)}
                          size={18}
                          color={providerColor(provider, colors)}
                          label={
                            entries.find((entry) => entry.provider === provider)?.providerLabel ??
                            provider
                          }
                          tabIndex={-1}
                          disabled={!allowed(provider)}
                          onClick={() => {
                            setTab(provider);
                            setHighlight(-1);
                            renderer?.focusElement?.(input.current?.id ?? 0);
                          }}
                          style={{
                            width: 38,
                            height: 38,
                            flexShrink: 0,
                            borderRadius: 7,
                            opacity: allowed(provider) ? 1 : 0.35,
                            backgroundColor: tab === provider ? colors.overlay_strong : undefined,
                          }}
                        />
                      ))}
                    </div>
                    <div
                      style={{
                        display: 'flex',
                        flexDirection: 'column',
                        flexGrow: 1,
                        minWidth: 0,
                        backgroundColor: colors.surface,
                      }}
                    >
                      <div
                        style={{
                          paddingLeft: 12,
                          paddingRight: 12,
                          paddingTop: 10,
                          paddingBottom: 8,
                        }}
                      >
                        <TextField
                          ref={input}
                          icon="search"
                          iconSize={15}
                          neutralFocus
                          placeholder="Search models…"
                          aria-label="Search models"
                          value={query}
                          onChange={(value) => {
                            setQuery(value);
                            setHighlight(-1);
                          }}
                          style={{
                            height: 34,
                            gap: 9,
                            borderRadius: 9,
                            backgroundColor: colors.raised,
                          }}
                        />
                      </div>
                      <div
                        role="listbox"
                        aria-label="Models"
                        style={{
                          display: 'flex',
                          flexDirection: 'column',
                          flexGrow: 1,
                          minHeight: 0,
                          overflowY: 'scroll',
                          padding: 9,
                        }}
                      >
                        {!visible.length && (
                          <div style={{ ...row, flexGrow: 1, justifyContent: 'center' }}>
                            <text style={{ color: colors.text_ghost, fontSize: sp(12.5) }}>
                              {loading
                                ? 'Loading models…'
                                : terms.length
                                  ? 'No models found'
                                  : tab === 'favorites'
                                    ? 'Star a model to keep it here'
                                    : 'No models reported by this provider'}
                            </text>
                          </div>
                        )}
                        {visible.map((entry, index) => {
                          const key = modelKey(entry);
                          const selected = current && modelKey(current) === key;
                          const starred = favorites.includes(key);
                          const subtitle = entry.providerLabel ?? entry.provider;
                          return (
                            <div
                              key={key}
                              ref={(instance) => {
                                if (instance) rows.current.set(key, instance);
                                else rows.current.delete(key);
                              }}
                              role="option"
                              aria-selected={Boolean(selected)}
                              aria-label={entry.modelLabel ?? entry.label}
                              onMouseDown={(event) => {
                                const instance = rows.current.get(key);
                                const bounds =
                                  instance && renderer?.getElementBounds?.(instance.id);
                                if (
                                  bounds &&
                                  event.x !== undefined &&
                                  event.x >= bounds.x + bounds.width - 41
                                )
                                  return;
                                if (event.button === 0) choose(entry);
                              }}
                              style={{
                                ...row,
                                height: 58,
                                flexShrink: 0,
                                paddingLeft: 12,
                                paddingRight: 12,
                                gap: 10,
                                borderRadius: 9,
                                borderWidth: 1,
                                borderColor: highlight === index ? colors.accent : 'transparent',
                                backgroundColor:
                                  highlight === index
                                    ? colors.overlay
                                    : selected
                                      ? colors.overlay_strong
                                      : undefined,
                                hover: { backgroundColor: colors.overlay },
                                active: { opacity: 0.85 },
                              }}
                            >
                              <div
                                style={{
                                  display: 'flex',
                                  flexDirection: 'column',
                                  flexGrow: 1,
                                  minWidth: 0,
                                  gap: 0,
                                }}
                              >
                                <text
                                  style={{
                                    ...ellipsis,
                                    color: colors.text,
                                    fontSize: sp(13),
                                    fontWeight: 600,
                                  }}
                                >
                                  {entry.modelLabel ?? entry.label}
                                </text>
                                <div style={{ ...row, gap: 6 }}>
                                  <Icon
                                    name={providerIcon(entry.provider)}
                                    size={10.5}
                                    color={providerColor(entry.provider, colors)}
                                  />
                                  <text
                                    style={{
                                      ...ellipsis,
                                      color: colors.text_tertiary,
                                      fontSize: sp(12.5),
                                    }}
                                  >
                                    {subtitle}
                                  </text>
                                </div>
                              </div>
                              <IconButton
                                name={starred ? 'star-filled' : 'star'}
                                size={14}
                                color={starred ? colors.favorite : colors.text_ghost}
                                label={`${starred ? 'Unstar' : 'Star'} ${entry.modelLabel ?? entry.label}`}
                                onClick={() => toggleFavorite(entry)}
                                style={{
                                  width: 28,
                                  height: 28,
                                  borderRadius: 6,
                                  hover: { backgroundColor: colors.overlay_strong },
                                }}
                              />
                            </div>
                          );
                        })}
                      </div>
                    </div>
                  </>
                )}
              </div>
            </anchored>
          </DismissableLayer>
        )}
      </div>
      {traitItems.length > 0 && (
        <Dropdown
          menuSide="top"
          chevron={false}
          label={`${current?.variantLabel ?? current?.reasoningEffort ?? current?.variant ?? 'Model traits'}${windows.length > 1 && current?.contextWindow !== windows[0]?.contextWindow ? ` · ${current?.contextWindow! / 1000}K` : ''}`}
          items={traitItems}
          menuStyle={{ minWidth: 214 }}
        />
      )}
      {!traitItems.length && (current?.variantLabel || current?.variant) && (
        <MenuChip disabled chevron={false} label={current.variantLabel ?? current.variant!} />
      )}
      {((catalog?.profiles.length ?? 0) > 1 || profileOpen) && (
        <Dropdown
          open={profileOpen}
          onOpenChange={setProfileOpen}
          menuSide="top"
          chevron={false}
          icon="bot"
          label={selection?.profile ?? current?.profile ?? 'Profile'}
          menuStyle={{ minWidth: 364, maxWidth: 364 }}
          items={catalog!.profiles.map((profile) => ({
            id: profile,
            label: profile,
            selected: profile === (selection?.profile ?? current?.profile),
            onSelect: () => onSelection({ profile }),
            content: (
              <div style={{ ...row, width: 340, paddingTop: 5, paddingBottom: 5, gap: 10 }}>
                <div
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    flexGrow: 1,
                    minWidth: 0,
                    gap: 2,
                  }}
                >
                  <text
                    style={{
                      ...ellipsis,
                      color: colors.text,
                      fontWeight: profile === (selection?.profile ?? current?.profile) ? 600 : 500,
                    }}
                  >
                    {profile}
                  </text>
                  <text style={{ color: colors.text_tertiary, lineHeight: sp(14) }}>
                    No description.
                  </text>
                </div>
                {profile === (selection?.profile ?? current?.profile) && (
                  <Icon name="check" size={11} />
                )}
              </div>
            ),
          }))}
        />
      )}
      <Dropdown
        menuSide="top"
        chevron={false}
        icon={mode.icon}
        label={mode.name}
        menuStyle={{ minWidth: 312, maxWidth: 320 }}
        items={accessChoices.map((choice) => ({
          id: choice.mode,
          selected: accessMode === choice.mode,
          label: choice.name,
          onSelect: () => onAccessMode(choice.mode),
          content: (
            <div style={{ ...row, width: 288, paddingTop: 4, paddingBottom: 4, gap: 10 }}>
              <Icon name={choice.icon} size={14} />
              <div
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  minWidth: 0,
                  flexGrow: 1,
                  flexShrink: 1,
                  gap: 2,
                }}
              >
                <text
                  style={{
                    ...ellipsis,
                    color: colors.text,
                    fontSize: sp(12.5),
                    fontWeight: accessMode === choice.mode ? 600 : 500,
                  }}
                >
                  {choice.name}
                </text>
                <text
                  style={{
                    color: colors.text_tertiary,
                    fontSize: sp(12.5),
                    lineHeight: sp(14),
                    whiteSpace: 'normal',
                  }}
                >
                  {choice.description}
                </text>
              </div>
              {accessMode === choice.mode && <Icon name="check" size={11} />}
            </div>
          ),
        }))}
      />
    </div>
  );
}

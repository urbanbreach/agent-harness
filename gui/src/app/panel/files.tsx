import { useEffect, useMemo, useRef, useState } from 'react';
import { useGpuix, type KeyEvent, type PublicInstance } from '@gpuix/react';
import { listTree, readText, writeText, type TreeEntry } from '../../workspace/files';
import { useTheme, CODE_FONT_FAMILY as MONO_FONT } from '../../theme';
import { Button, Icon, IconButton, TextField, Tooltip, ellipsis, row, useToast } from '../../ui';
import { nativeRenderer } from '../../native';
import { highlightCode } from '../../syntax';
import { Markdown } from '../../markdown';
import { getAppStore } from '../../store';
import { FileIcon, Toolbar } from './shared';
import { searchText, replaceMatch, replaceMatches, type SearchOptions } from './search';

interface Buffer {
  text: string;
  saved: string;
  error?: string;
  loaded: boolean;
}
export interface FilesState {
  selected?: string;
  expanded: Set<string>;
  buffers: Map<string, Buffer>;
  treeWidth: number;
}
export function createFilesState(): FilesState {
  return { expanded: new Set(), buffers: new Map(), treeWidth: 184 };
}
export async function saveFileState(cwd: string, state: FilesState): Promise<void> {
  const path = state.selected;
  const buffer = path ? state.buffers.get(path) : undefined;
  if (!path || !buffer?.loaded || buffer.error) return;
  const text = buffer.text;
  await writeText(cwd, path, text);
  buffer.saved = text;
}
export function FilesView({
  cwd,
  state,
  onChange,
}: {
  cwd: string;
  state: FilesState;
  onChange: () => void;
}) {
  const { colors, codeFontSize, sp } = useTheme();
  const { renderer } = useGpuix();
  const { showToast } = useToast();
  const input = useRef<PublicInstance>(null);
  const queryInput = useRef<PublicInstance>(null);
  const selection = useRef({ start: 0, end: 0 });
  const documentSelectionAnchor = useRef<number | undefined>(undefined);
  const [tree, setTree] = useState<TreeEntry[]>([]);
  const [treeVersion, setTreeVersion] = useState(0);
  const [find, setFind] = useState(false);
  const [replace, setReplace] = useState(false);
  const [query, setQuery] = useState('');
  const [queryFocused, setQueryFocused] = useState(false);
  const [editorFocused, setEditorFocused] = useState(false);
  const [replacement, setReplacement] = useState('');
  const [options, setOptions] = useState<SearchOptions>({
    caseSensitive: false,
    wholeWord: false,
    regex: false,
  });
  const [activeMatch, setActiveMatch] = useState(0);
  const [preview, setPreview] = useState(
    () => getAppStore().getSnapshot().preferences['panel.markdownPreview'] === true,
  );
  const [drag, setDrag] = useState<{ x: number; width: number }>();
  const [width, setWidth] = useState(684);
  const buffer = state.selected ? state.buffers.get(state.selected) : undefined;
  const text = buffer?.text ?? '';
  const search = useMemo(() => searchText(text, query, options), [text, query, options]);
  const runs = useMemo(
    () => highlightCode(text, state.selected?.split('.').pop(), colors.is_dark),
    [text, state.selected, colors.is_dark],
  );
  useEffect(() => {
    if (!renderer || !input.current) return;
    const id = input.current.id;
    nativeRenderer(renderer).setInputSelectionListener(id, (error, event) => {
      if (!error)
        selection.current = { start: event.selectionStart ?? 0, end: event.selectionEnd ?? 0 };
    });
    return () => nativeRenderer(renderer).setInputSelectionListener(id, null);
  }, [renderer, state.selected, preview]);
  useEffect(() => {
    let active = true;
    void listTree(cwd, state.expanded)
      .then((entries) => {
        if (active) setTree(entries);
      })
      .catch((error) => showToast(String(error)));
    return () => {
      active = false;
    };
  }, [cwd, state, treeVersion]);
  useEffect(() => {
    setFind(false);
    documentSelectionAnchor.current = undefined;
  }, [state]);
  useEffect(() => {
    const path = state.selected;
    if (!path || state.buffers.has(path)) return;
    const next: Buffer = { text: '', saved: '', loaded: false };
    state.buffers.set(path, next);
    void readText(cwd, path)
      .then((content) => {
        next.text = content;
        next.saved = content;
        next.loaded = true;
        onChange();
      })
      .catch((error) => {
        next.error = String(error);
        next.text = `Unable to edit this file: ${error}`;
        next.loaded = true;
        onChange();
      });
  }, [cwd, state.selected]);
  function reveal(index: number) {
    if (!search.matches.length) return;
    const next = (index + search.matches.length) % search.matches.length;
    setActiveMatch(next);
    const match = search.matches[next]!;
    if (renderer && input.current) {
      const native = nativeRenderer(renderer);
      native.inputSetSelection(input.current.id, match.start, match.end);
      native.inputRevealRange(input.current.id, match.start, match.end);
    }
  }
  useEffect(() => {
    if (find) reveal(Math.min(activeMatch, Math.max(0, search.matches.length - 1)));
  }, [query, options]);
  useEffect(() => {
    setActiveMatch((index) => Math.min(index, Math.max(0, search.matches.length - 1)));
  }, [text, state.selected]);
  function openFind(withReplace = false) {
    const selected = text.slice(selection.current.start, selection.current.end);
    const seed = selected && !/[\r\n]/.test(selected) ? selected : query;
    setQuery(seed);
    setFind(true);
    setReplace(withReplace);
    setTimeout(() => {
      if (queryInput.current && renderer) {
        renderer.focusElement?.(queryInput.current.id);
        setQueryFocused(true);
        setEditorFocused(false);
        nativeRenderer(renderer).inputSetSelection(queryInput.current.id, 0, seed.length);
      }
    }, 0);
  }
  function closeFind() {
    setFind(false);
    if (input.current) renderer?.focusElement?.(input.current.id);
  }
  function edit(value: string) {
    if (!buffer || buffer.error) return;
    buffer.text = value;
    onChange();
  }
  function replaceCurrent() {
    const match = search.matches[activeMatch];
    if (!search.matches.length || search.error) return;
    if (!match || buffer?.error) return;
    edit(replaceMatch(text, query, replacement, options, match));
  }
  function keys(event: KeyEvent) {
    if (event.defaultPrevented) return;
    const key = event.key?.toLowerCase();
    const m = event.modifiers;
    let handled = true;
    if (m?.ctrl && key === 's')
      void saveFileState(cwd, state)
        .then(onChange)
        .catch((error) => showToast(String(error)));
    else if (m?.ctrl && key === 'f') openFind(!!m.alt);
    else if (m?.ctrl && key === 'g') reveal(activeMatch + (m.shift ? -1 : 1));
    else if (find && key === 'escape') closeFind();
    else if (m?.ctrl && m.alt && ['c', 'w', 'r'].includes(key ?? '')) {
      const field = key === 'c' ? 'caseSensitive' : key === 'w' ? 'wholeWord' : 'regex';
      setOptions((old) => ({ ...old, [field]: !old[field] }));
    } else handled = false;
    if (handled) {
      event.preventDefault();
      event.stopPropagation();
    }
  }
  const markdown = /\.(md|markdown|mdx)$/i.test(state.selected ?? '');
  const treeWidth = Math.max(140, Math.min(state.treeWidth, 360, width - 140));
  const findWidth = Math.max(96, Math.min(380, width - treeWidth - 1 - 252));
  return (
    <div
      onKeyDown={keys}
      onLayout={(event) => {
        if (event.width) setWidth(event.width);
      }}
      onMouseMove={(event) => {
        if (drag && event.x !== undefined) {
          state.treeWidth = Math.max(140, Math.min(360, drag.width + drag.x - event.x));
          onChange();
        }
      }}
      onMouseUp={() => setDrag(undefined)}
      style={{ ...row, alignItems: 'stretch', flexGrow: 1, minHeight: 0, minWidth: 0 }}
    >
      {state.selected && (
        <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minWidth: 0 }}>
          <Toolbar>
            <FileIcon path={state.selected} size={13} />
            <text
              style={{ ...ellipsis, flexGrow: 1, fontSize: sp(12.5), color: colors.text_secondary }}
            >
              {state.selected}
            </text>
            {markdown && (
              <Tooltip content={preview ? 'Edit source' : 'Preview markdown'}>
                <IconButton
                  name={preview ? 'pencil' : 'eye'}
                  size={12}
                  style={{ width: 26, height: 26 }}
                  onClick={() => {
                    setPreview(!preview);
                    getAppStore().setPreferences({ 'panel.markdownPreview': !preview });
                  }}
                />
              </Tooltip>
            )}
          </Toolbar>
          {find && !(markdown && preview) && (
            <div
              onKeyDown={(event) => {
                if (event.defaultPrevented || event.key !== 'enter') return;
                if (event.modifiers?.ctrl && event.modifiers.alt) {
                  if (!buffer?.error && !search.error)
                    edit(replaceMatches(text, query, replacement, options));
                } else if (
                  event.modifiers?.shift &&
                  !event.modifiers.ctrl &&
                  !event.modifiers.alt
                ) {
                  reveal(activeMatch - 1);
                } else return;
                event.preventDefault();
                event.stopPropagation();
              }}
              style={{
                display: 'flex',
                flexDirection: 'column',
                paddingTop: 6,
                paddingBottom: 6,
                paddingLeft: 4,
                paddingRight: 10,
                borderBottomWidth: 1,
                borderColor: colors.border,
                gap: 4,
                flexShrink: 0,
                overflow: 'hidden',
              }}
            >
              <div style={{ ...row, gap: 4 }}>
                <IconButton
                  name={replace ? 'chevron-down' : 'chevron-right'}
                  size={10}
                  disabled={!!buffer?.error}
                  style={{ width: 18, height: 24 }}
                  onClick={() => setReplace(!replace)}
                />
                <div
                  style={{
                    ...row,
                    width: findWidth,
                    flexShrink: 0,
                    height: 24,
                    paddingLeft: 5,
                    paddingRight: 5,
                    borderWidth: 1,
                    borderRadius: 5,
                    borderColor: search.error
                      ? colors.danger
                      : queryFocused
                        ? colors.accent
                        : colors.border_strong,
                    backgroundColor: colors.inset,
                  }}
                >
                  <input
                    ref={queryInput}
                    tabIndex={0}
                    onMouseDown={() => {
                      setQueryFocused(true);
                      setEditorFocused(false);
                    }}
                    value={query}
                    onChange={(event) => setQuery(event.value ?? '')}
                    placeholder="Find"
                    aria-label="Find in file"
                    onFocus={() => setQueryFocused(true)}
                    onBlur={() => setQueryFocused(false)}
                    theme={{
                      appearance: colors.is_dark ? 'dark' : 'light',
                      text: colors.text,
                      caret: colors.accent,
                      ...{ placeholder: colors.text_ghost },
                    }}
                    style={{
                      flexGrow: 1,
                      minWidth: 0,
                      height: sp(16),
                      fontSize: sp(12.5),
                      lineHeight: sp(16),
                      color: colors.text,
                      backgroundColor: 'transparent',
                    }}
                    onKeyDown={(event) => {
                      if (
                        event.key === 'enter' &&
                        !event.modifiers?.ctrl &&
                        !event.modifiers?.alt
                      ) {
                        event.preventDefault();
                        event.stopPropagation();
                        reveal(activeMatch + (event.modifiers?.shift ? -1 : 1));
                      }
                    }}
                  />
                  {(['caseSensitive', 'wholeWord', 'regex'] as const).map((field, i) => (
                    <Tooltip
                      key={field}
                      content={['Match case', 'Whole word', 'Regular expression'][i]}
                    >
                      <IconButton
                        name={(['case-sensitive', 'whole-word', 'regex'] as const)[i]!}
                        size={12}
                        style={{
                          width: 18,
                          height: 18,
                          borderRadius: 4,
                          borderWidth: 1,
                          borderColor: options[field] ? `${colors.accent}80` : 'transparent',
                          backgroundColor: options[field] ? `${colors.accent}38` : 'transparent',
                          hover: { backgroundColor: colors.overlay_strong },
                        }}
                        color={options[field] ? colors.text : colors.text_tertiary}
                        onClick={() => setOptions((old) => ({ ...old, [field]: !old[field] }))}
                      />
                    </Tooltip>
                  ))}
                </div>
                <text
                  style={{
                    minWidth: 56,
                    flexShrink: 0,
                    fontSize: sp(12.5),
                    color: search.error ? colors.danger : colors.text_tertiary,
                  }}
                >
                  {!query
                    ? ''
                    : search.error
                      ? 'Bad pattern'
                      : search.matches.length
                        ? `${Math.min(activeMatch + 1, search.matches.length)} of ${search.matches.length}${search.capped ? '+' : ''}`
                        : 'No results'}
                </text>
                <div
                  style={{
                    width: 1,
                    height: 16,
                    flexShrink: 0,
                    backgroundColor: colors.border_strong,
                  }}
                />
                <IconButton
                  name="arrow-up"
                  size={12}
                  style={{ width: 22, height: 22, borderRadius: 5 }}
                  disabled={!search.matches.length}
                  onClick={() => reveal(activeMatch - 1)}
                />
                <IconButton
                  name="arrow-down"
                  size={12}
                  style={{ width: 22, height: 22, borderRadius: 5 }}
                  disabled={!search.matches.length}
                  onClick={() => reveal(activeMatch + 1)}
                />
                <div style={{ flexGrow: 1 }} />
                <IconButton
                  name="x"
                  size={12}
                  style={{ width: 22, height: 22, borderRadius: 5 }}
                  onClick={closeFind}
                />
              </div>
              {replace && !buffer?.error && (
                <div style={{ ...row, gap: 4, paddingLeft: 22 }}>
                  <TextField
                    value={replacement}
                    onChange={setReplacement}
                    placeholder="Replace"
                    style={{
                      width: findWidth,
                      flexShrink: 0,
                      height: 24,
                      paddingLeft: 5,
                      paddingRight: 5,
                      borderRadius: 5,
                    }}
                    onKeyDown={(event) => {
                      if (
                        event.key === 'enter' &&
                        !event.modifiers?.ctrl &&
                        !event.modifiers?.alt
                      ) {
                        event.preventDefault();
                        event.stopPropagation();
                        if (event.modifiers?.shift) reveal(activeMatch - 1);
                        else replaceCurrent();
                      }
                    }}
                  />
                  <IconButton
                    name="replace"
                    size={12}
                    style={{ width: 22, height: 22, borderRadius: 5 }}
                    disabled={!search.matches.length || !!buffer?.error}
                    onClick={replaceCurrent}
                  />
                  <IconButton
                    name="replace-all"
                    size={12}
                    style={{ width: 22, height: 22, borderRadius: 5 }}
                    disabled={!search.matches.length || !!buffer?.error}
                    onClick={() => edit(replaceMatches(text, query, replacement, options))}
                  />
                </div>
              )}
            </div>
          )}
          {markdown && preview ? (
            <div
              style={{
                flexGrow: 1,
                overflowY: 'scroll',
                paddingLeft: 16,
                paddingRight: 16,
                paddingTop: 14,
                paddingBottom: 24,
              }}
            >
              <Markdown source={text} context="document" />
            </div>
          ) : (
            <textarea
              ref={input}
              tabIndex={0}
              value={text}
              interceptedKeys={['ctrl-home', 'ctrl-end', 'ctrl-shift-home', 'ctrl-shift-end']}
              onKeyDown={(event) => {
                const key = event.key?.toLowerCase();
                if (
                  !event.modifiers?.ctrl ||
                  event.modifiers.alt ||
                  !['home', 'end'].includes(key ?? '')
                ) {
                  documentSelectionAnchor.current = undefined;
                  return;
                }
                if (!renderer || !input.current) return;
                event.preventDefault();
                event.stopPropagation();
                const target = key === 'home' ? 0 : text.length;
                if (!event.modifiers.shift) documentSelectionAnchor.current = undefined;
                else documentSelectionAnchor.current ??= selection.current.start;
                const anchor = documentSelectionAnchor.current ?? target;
                selection.current = { start: anchor, end: target };
                const native = nativeRenderer(renderer);
                native.inputSetSelection(input.current.id, anchor, target);
                native.inputRevealRange(input.current.id, target, target);
              }}
              onMouseDown={() => {
                setEditorFocused(true);
                setQueryFocused(false);
                documentSelectionAnchor.current = undefined;
              }}
              onBlur={() => {
                setEditorFocused(false);
                documentSelectionAnchor.current = undefined;
              }}
              theme={{
                text: colors.text_secondary,
                caret: colors.accent,
                bg: colors.surface,
                ...{ selectionWash: find && !editorFocused ? 'transparent' : colors.selection },
              }}
              readOnly={!buffer?.loaded || !!buffer?.error}
              onChange={(event) => edit(event.value ?? '')}
              onFocus={() => {
                setEditorFocused(true);
                const path = state.selected;
                if (path && buffer?.loaded && !buffer.error && buffer.text === buffer.saved) {
                  const saved = buffer.saved;
                  void readText(cwd, path)
                    .then((content) => {
                      if (buffer.text === saved && buffer.saved === saved) {
                        buffer.text = content;
                        buffer.saved = content;
                        onChange();
                      }
                    })
                    .catch((error) => showToast(String(error)));
                }
              }}
              runs={runs}
              highlights={
                find
                  ? search.matches.map((match, index) => ({
                      ...match,
                      color: colors.is_dark ? '#E0B36A38' : '#A66B2038',
                      active: index === activeMatch,
                      activeColor: colors.is_dark ? '#E0B36A80' : '#A66B2080',
                    }))
                  : []
              }
              gutter={{
                color: colors.text_ghost,
                width: 12 + Math.ceil(codeFontSize * 0.6) * String(text.split('\n').length).length,
                gap: 8,
                fontSize: codeFontSize,
              }}
              style={{
                flexGrow: 1,
                minHeight: 0,
                fontFamily: MONO_FONT,
                fontSize: codeFontSize,
                lineHeight: Math.round(codeFontSize * 1.5),
                color: colors.text_secondary,
                backgroundColor: colors.surface,
                paddingTop: 6,
                paddingBottom: 6,
                paddingRight: 10,
                paddingLeft: 0,
              }}
            />
          )}
        </div>
      )}
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          width: state.selected ? treeWidth : undefined,
          flexGrow: state.selected ? 0 : 1,
          flexShrink: 0,
          minHeight: 0,
          borderLeftWidth: state.selected ? 1 : 0,
          borderColor: colors.border,
          position: 'relative',
        }}
      >
        {state.selected && (
          <div
            onMouseDown={(event) => {
              if (event.button === 0 && event.x !== undefined)
                setDrag({ x: event.x, width: treeWidth });
            }}
            style={{
              position: 'absolute',
              left: -5,
              top: 0,
              bottom: 0,
              width: 10,
              cursor: 'col-resize',
              hover: { backgroundColor: colors.resize_handle },
            }}
          />
        )}
        <Toolbar>
          <Icon name="folder" size={13} color={colors.text_tertiary} />
          <text
            style={{
              ...ellipsis,
              fontSize: sp(12.5),
              fontWeight: 500,
              color: colors.text_secondary,
            }}
          >
            {cwd.split('/').filter(Boolean).pop() ?? 'workspace'}
          </text>
        </Toolbar>
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            flexGrow: 1,
            overflowY: 'scroll',
            paddingTop: 6,
            paddingBottom: 6,
          }}
        >
          {tree.map((entry) => (
            <Button
              key={entry.path}
              onClick={() => {
                if (entry.directory) {
                  if (state.expanded.has(entry.path)) state.expanded.delete(entry.path);
                  else state.expanded.add(entry.path);
                  setTreeVersion((value) => value + 1);
                } else {
                  if (!state.selected) {
                    const store = getAppStore();
                    store.setLayout({
                      rightWidth: Math.max(
                        684,
                        Number(store.getSnapshot().layout.rightWidth) || 460,
                      ),
                    });
                  }
                  state.selected = entry.path;
                  onChange();
                }
              }}
              style={{
                justifyContent: 'flex-start',
                flexShrink: 0,
                height: 30,
                marginLeft: 8,
                marginRight: 8,
                paddingLeft: 8 + entry.depth * 16,
                paddingRight: 8,
                borderRadius: 6,
                gap: 6,
                backgroundColor: entry.path === state.selected ? colors.overlay_strong : undefined,
              }}
            >
              {entry.directory ? (
                <Icon
                  name={entry.expanded ? 'chevron-down' : 'chevron-right'}
                  size={10}
                  color={colors.text_ghost}
                />
              ) : (
                <div style={{ width: 10 }} />
              )}
              {entry.directory ? <Icon name="folder" size={14} /> : <FileIcon path={entry.path} />}
              <text style={{ ...ellipsis, fontSize: sp(12.5), color: colors.text_secondary }}>
                {entry.name}
              </text>
            </Button>
          ))}
        </div>
      </div>
    </div>
  );
}

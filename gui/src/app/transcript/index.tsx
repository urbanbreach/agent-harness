import { useEffect, useMemo, useRef, useState } from 'react';
import { findRanges, useGpuix, useTextSearch, type PublicInstance } from '@gpuix/react';
import { useSessionView, usePreferences } from '../../store';
import { emptySessionView } from '../../model/session';
import { nativeRenderer } from '../../native';
import { parseMarkdown, type MarkdownBlock } from '../../markdown';
import { useTheme } from '../../theme';
import { Button, Icon, IconButton } from '../../ui';
import { registerAction } from '../actions';
import { transcriptRows, duration } from './rows';
import { ActivityGroup } from './activity';
import { AssistantMessage, UserMessage, ResponseFooter } from './messages';
import { BackgroundWork } from './background';
import { useTurnCheckpoints } from './checkpoints';
import { CheckpointCard } from './checkpoint-card';
import { Scrollbar } from '../../ui/scrollbar';

function renderedText(blocks: MarkdownBlock[]): string {
  return blocks
    .map((block) => {
      switch (block.type) {
        case 'heading':
        case 'paragraph':
          return block.spans
            .map((span) => (span.type === 'text' ? span.text : span.source))
            .join('');
        case 'quote':
          return renderedText(block.blocks);
        case 'list':
          return block.items.map((item) => renderedText(item.blocks)).join('\n');
        case 'table':
          return [block.header, ...block.rows]
            .map((row) =>
              row
                .map((cell) =>
                  cell.map((span) => (span.type === 'text' ? span.text : span.source)).join(''),
                )
                .join('\t'),
            )
            .join('\n');
        case 'code':
          return block.text;
        case 'math':
          return block.source;
        case 'image':
          return block.alt;
        case 'rule':
          return '';
      }
    })
    .join('\n\n');
}
const empty = emptySessionView();
export function TranscriptView({ sessionId }: { sessionId: string }) {
  const view = useSessionView(sessionId) ?? empty;
  const preferences = usePreferences();
  const { colors, sp, reducedMotion } = useTheme();
  const { renderer } = useGpuix();
  const list = useRef<PublicInstance>(null);
  const root = useRef<PublicInstance>(null);
  const input = useRef<PublicInstance>(null);
  const restoredFocus = useRef<number | null>(null);
  const railTicks = useRef<(PublicInstance | null)[]>([]);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [hoverTurn, setHoverTurn] = useState<string>();
  const [findOpen, setFindOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [width, setWidth] = useState(0);
  const [height, setHeight] = useState(0);
  const [visible, setVisible] = useState({ start: 0, end: 0 });
  const [rowHeights, setRowHeights] = useState<Record<string, number>>({});
  const [scrollTop, setScrollTop] = useState(0);
  const [following, setFollowing] = useState(true);
  const [anchoredTurn, setAnchoredTurn] = useState<string>();
  const [railHover, setRailHover] = useState<number>();
  const [now, setNow] = useState(Date.now());
  const workingStart = useMemo(() => {
    const turn = view.turns.at(-1);
    const date = Date.parse(
      view.entries.find((entry) => entry.turnId === turn?.id && entry.timestamp)?.timestamp ?? '',
    );
    return Number.isFinite(date) ? date : Date.now();
  }, [sessionId, view.turns.at(-1)?.id]);
  const checkpoints = useTurnCheckpoints(sessionId, view);
  const rows = useMemo(
    () => transcriptRows(view, expanded, preferences.showReasoning !== false),
    [view, expanded, preferences.showReasoning],
  );
  const offsets = [0];
  for (const row of rows) offsets.push(offsets[offsets.length - 1]! + (rowHeights[row.id] ?? 110));
  const anchorIndex = anchoredTurn
    ? rows.findIndex(
        (row) =>
          row.kind === 'message' && row.entry.turnId === anchoredTurn && row.entry.kind === 'user',
      )
    : -1;
  const spacer =
    anchorIndex < 0 ? 0 : Math.max(0, height - (offsets.at(-1)! - offsets[anchorIndex]!));
  const contentHeight = offsets.at(-1)! + spacer;
  const messageRows = rows
    .map((row, index) => ({ row, index }))
    .filter(
      ({ row }) =>
        row.kind === 'message' && (row.entry.kind === 'user' || row.entry.kind === 'text'),
    );
  const matches = useMemo(() => {
    if (!query) return [];
    const found: { row: number; offset: number }[] = [];
    for (const { row, index } of messageRows) {
      if (row.kind !== 'message' || !('text' in row.entry)) continue;
      for (const [offset] of findRanges({
        text: renderedText(parseMarkdown(row.entry.text)),
        query,
      })) {
        found.push({ row: index, offset });
        if (found.length === 20000) return found;
      }
    }
    return found;
  }, [rows, query]);
  const search = useTextSearch({
    query: findOpen ? query : '',
    color: colors.is_dark ? '#efc0062e' : '#f9ce1f33',
    activeColor: colors.is_dark ? '#ff8000c7' : '#ff8a14b3',
    matches: { total: matches.length, indexOffset: 0 },
  });
  useEffect(() => setFollowing(true), [sessionId]);
  function reveal(index: number) {
    setFollowing(false);
    if (list.current) renderer?.scrollToItem?.(list.current.id, index, -24);
  }
  function followTail() {
    setFollowing(true);
    setAnchoredTurn(undefined);
    if (list.current) renderer?.scrollToItem?.(list.current.id, rows.length, 0);
  }
  useEffect(() => {
    const remove = ['follow', 'tail'].map((command) =>
      registerAction({
        id: `transcript.${command}`,
        label: command === 'follow' ? 'Follow latest output' : 'Scroll to latest output',
        enabled: () => rows.length > 0,
        run: followTail,
      }),
    );
    return () => remove.forEach((unregister) => unregister());
  }, [rows.length, renderer]);
  function navigate(back: boolean) {
    if (!matches.length) return;
    const next =
      (((search.active + (back ? -1 : 1)) % matches.length) + matches.length) % matches.length;
    search.goTo(next);
    reveal(matches[next]!.row);
  }
  function openFind() {
    if (!view.entries.length) return;
    restoredFocus.current = renderer?.getFocusedElementId?.() ?? null;
    const selected = renderer?.getSelectedText?.() ?? '';
    if (selected && !selected.includes('\n')) setQuery(selected);
    setFindOpen(true);
  }
  function closeFind() {
    setFindOpen(false);
    if (restoredFocus.current) renderer?.focusElement?.(restoredFocus.current);
  }
  useEffect(
    () =>
      registerAction({
        id: 'transcript.find',
        label: 'input.find',
        shortcut: 'Ctrl+F',
        run: openFind,
        enabled: () => view.entries.length > 0,
      }),
    [view.entries.length, renderer],
  );
  useEffect(
    () =>
      registerAction({
        id: 'transcript.copySelection',
        label: 'common.copy_selection',
        enabled: () => !!renderer?.getSelectedText?.(),
        run: () => {
          const selected = renderer?.getSelectedText?.();
          if (renderer && selected) nativeRenderer(renderer).writeClipboardText(selected);
        },
      }),
    [renderer],
  );
  useEffect(() => {
    const failed = view.turns.filter((turn) => turn.status === 'failed').at(-1);
    const streaming = view.turns.filter((turn) => turn.status === 'working').at(-1);
    function jump(turnId: string | undefined) {
      if (!turnId) return;
      const index = rows.findIndex((row) => row.kind === 'message' && row.entry.turnId === turnId);
      if (index >= 0) reveal(index);
    }
    const removeFailed = registerAction({
      id: 'transcript.latestFailed',
      label: 'Jump to latest failed turn',
      enabled: () => !!failed,
      run: () => jump(failed?.id),
    });
    const removeStreaming = registerAction({
      id: 'transcript.streaming',
      label: 'Jump to live streaming turn',
      enabled: () => !!streaming,
      run: () => jump(streaming?.id),
    });
    return () => {
      removeFailed();
      removeStreaming();
    };
  }, [rows, view.turns, renderer]);
  useEffect(() => {
    if (findOpen && input.current) {
      renderer?.focusElement?.(input.current.id);
      if (renderer) nativeRenderer(renderer).inputSetSelection(input.current.id, 0, query.length);
    }
  }, [findOpen, renderer]);
  useEffect(() => {
    if (!query) return;
    const nearest = matches.findIndex((match) => match.row >= visible.start);
    search.goTo(Math.max(0, nearest));
  }, [query]);
  useEffect(() => {
    if (!['working', 'waiting', 'background'].includes(view.status)) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [view.status]);
  useEffect(() => {
    setExpanded(new Set());
    setFindOpen(false);
    setHoverTurn(undefined);
  }, [sessionId]);
  const previousTurn = useRef<string | undefined>(undefined);
  useEffect(() => {
    const latest = view.turns.at(-1);
    if (latest?.status === 'working' && latest.id !== previousTurn.current) {
      setAnchoredTurn(latest.id);
      const index = rows.findIndex(
        (row) =>
          row.kind === 'message' && row.entry.turnId === latest.id && row.entry.kind === 'user',
      );
      if (index >= 0) reveal(index);
    }
    previousTurn.current = latest?.id;
  }, [view.turns.length, view.turns.at(-1)?.id]);
  const opening = rows
    .map((row, index) => ({ row, index }))
    .filter(
      ({ row }) => row.kind === 'message' && row.entry.kind === 'user' && !row.entry.steering,
    );
  const detached = visible.end > 0 && visible.end < rows.length;
  const overflowing = visible.start > 0 || detached;
  return (
    <div
      ref={root}
      tabIndex={0}
      onLayout={(event) => {
        setWidth(event.width ?? 0);
        setHeight(event.height ?? 0);
      }}
      onMouseDown={(event) => {
        if (event.elementId === root.current?.id && root.current)
          renderer?.focusElement?.(root.current.id);
      }}
      onKeyDown={(event) => {
        if (event.modifiers?.ctrl && event.key?.toLowerCase() === 'f') {
          event.preventDefault();
          event.stopPropagation();
          openFind();
        } else if (event.modifiers?.ctrl && event.key?.toLowerCase() === 'g') {
          event.preventDefault();
          event.stopPropagation();
          navigate(!!event.modifiers.shift);
        } else if (findOpen && event.key?.toLowerCase() === 'escape') {
          event.preventDefault();
          closeFind();
        }
      }}
      style={{
        flexGrow: 1,
        minHeight: 0,
        width: '100%',
        position: 'relative',
        overflow: 'hidden',
        backgroundColor: colors.surface,
      }}
    >
      <virtual-list
        ref={list}
        alignment="bottom"
        followTail={following && !detached}
        estimatedItemHeight={110}
        onVisibleRange={(event) => {
          const start = event.startIndex ?? 0;
          const end = event.endIndex ?? 0;
          if (end > 0 && end < rows.length) setFollowing(false);
          setVisible((current) =>
            current.start === start && current.end === end ? current : { start, end },
          );
          const anchor = list.current ? renderer?.getListScrollTop?.(list.current.id) : null;
          if (anchor) setScrollTop((offsets[anchor[0] ?? 0] ?? 0) + Math.abs(anchor[1] ?? 0));
        }}
        style={{ width: '100%', height: '100%' }}
      >
        {rows.map((row, index) => {
          const turnId =
            row.kind === 'message'
              ? row.entry.turnId
              : row.kind === 'fold' || row.kind === 'footer'
                ? row.turn.id
                : row.kind === 'activity'
                  ? row.entries[0]?.turnId
                  : row.turn?.id;
          const newTurn =
            row.kind === 'message' && row.entry.kind === 'user' && !row.entry.steering && index > 0;
          const offset = matches.findIndex((match) => match.row === index);
          return (
            <div
              key={row.id}
              onLayout={(event) => {
                const measured = event.height ?? 0;
                if (measured > 0)
                  setRowHeights((current) =>
                    current[row.id] === measured ? current : { ...current, [row.id]: measured },
                  );
              }}
              onMouseEnter={() => {
                if (row.kind !== 'message' || row.entry.kind !== 'user') setHoverTurn(turnId);
              }}
              onMouseLeave={() => setHoverTurn(undefined)}
              style={{
                display: 'flex',
                flexDirection: 'column',
                width: '100%',
                paddingLeft: 20,
                paddingRight: 20,
                paddingTop: row.kind === 'footer' ? 0 : index === 0 ? 22 : newTurn ? 48 : 8,
                paddingBottom: index === rows.length - 1 ? 25 : 8,
                alignItems: 'center',
              }}
            >
              <div style={{ width: '100%', maxWidth: 720, minWidth: 0 }}>
                {row.kind === 'message' && row.entry.kind === 'user' && (
                  <div
                    highlight={
                      findOpen && query
                        ? {
                            query,
                            activeIndex: search.active,
                            matchIndexOffset: Math.max(0, offset),
                            color: colors.is_dark ? '#efc0062e' : '#f9ce1f33',
                            activeColor: '#ff8000c7',
                          }
                        : undefined
                    }
                  >
                    <UserMessage
                      sessionId={sessionId}
                      entry={row.entry}
                      view={view}
                      checkpoint={row.entry.turnId ? checkpoints[row.entry.turnId] : undefined}
                    />
                  </div>
                )}
                {row.kind === 'message' && row.entry.kind === 'text' && (
                  <div
                    highlight={
                      findOpen && query
                        ? {
                            query,
                            activeIndex: search.active,
                            matchIndexOffset: Math.max(0, offset),
                            color: colors.is_dark ? '#efc0062e' : '#f9ce1f33',
                            activeColor: '#ff8000c7',
                          }
                        : undefined
                    }
                    style={{ paddingTop: 4, paddingBottom: 4, minWidth: 0 }}
                  >
                    <AssistantMessage
                      sessionId={sessionId}
                      view={view}
                      turnId={row.entry.turnId}
                      text={row.entry.text}
                      streaming={row.entry.streaming && !reducedMotion}
                    />
                  </div>
                )}
                {row.kind === 'activity' && (
                  <ActivityGroup entries={row.entries} view={view} live={row.live} />
                )}
                {row.kind === 'fold' && (
                  <Button
                    onClick={() =>
                      setExpanded((current) => {
                        const next = new Set(current);
                        if (next.has(row.turn.id)) next.delete(row.turn.id);
                        else next.add(row.turn.id);
                        return next;
                      })
                    }
                    aria-expanded={expanded.has(row.turn.id)}
                    style={{ height: 24, width: '100%', gap: 10 }}
                  >
                    <div style={{ height: 1, flexGrow: 1, backgroundColor: colors.border }} />
                    <text
                      style={{ color: colors.text_tertiary, fontSize: sp(13.5), fontWeight: '500' }}
                    >
                      {`${row.turn.status === 'interrupted' ? 'You stopped after' : 'Worked for'} ${duration(row.turn.durationMs ?? (row.turn.endedMs ?? row.turn.startedMs) - row.turn.startedMs)}`}
                    </text>
                    <Icon
                      name={expanded.has(row.turn.id) ? 'chevron-down' : 'chevron-right'}
                      size={11.5}
                    />
                    <div style={{ height: 1, flexGrow: 1, backgroundColor: colors.border }} />
                  </Button>
                )}
                {row.kind === 'footer' && (
                  <>
                    <CheckpointCard sessionId={sessionId} turnId={row.turn.id} />
                    {row.turn.id === view.turns.at(-1)?.id && (
                      <BackgroundWork sessionId={sessionId} view={view} />
                    )}
                    {!!row.text && (
                      <ResponseFooter
                        sessionId={sessionId}
                        view={view}
                        turn={row.turn}
                        text={row.text}
                        visible={hoverTurn === row.turn.id}
                      />
                    )}
                  </>
                )}
                {row.kind === 'working' && (
                  <>
                    <div
                      style={{
                        height: 22,
                        display: 'flex',
                        flexDirection: 'row',
                        alignItems: 'center',
                        gap: 8,
                      }}
                    >
                      <WorkingDots />
                      <text
                        style={{
                          color: colors.text_tertiary,
                          fontSize: sp(13.5),
                          fontWeight: '500',
                        }}
                      >
                        {view.status === 'background'
                          ? 'Waiting for background tasks'
                          : Object.values(view.retries).length
                            ? `Retrying (${Object.values(view.retries).at(-1)!.retry.attempt}/${Object.values(view.retries).at(-1)!.retry.max_attempts})…`
                            : `Working for ${duration(Math.max(0, now - workingStart), true)}`}
                      </text>
                    </div>
                    <BackgroundWork sessionId={sessionId} view={view} />
                  </>
                )}
              </div>
            </div>
          );
        })}
        {spacer > 0 && <div key="turn-anchor-space" style={{ height: spacer, width: '100%' }} />}
      </virtual-list>
      <Scrollbar
        viewport={height}
        content={contentHeight}
        offset={scrollTop}
        onScroll={(offset) => {
          setFollowing(false);
          let index = offsets.findIndex((value) => value > offset) - 1;
          if (index < 0) index = rows.length - 1;
          if (list.current)
            renderer?.scrollToItem?.(list.current.id, index, offset - (offsets[index] ?? 0));
        }}
      />
      {width >= 872 && overflowing && opening.length >= 2 && (
        <div
          style={{
            position: 'absolute',
            left: 16,
            top: Math.max(12, (height - Math.min(opening.length * 12, height * 0.8)) / 2),
            width: 44,
            maxHeight: height * 0.8,
            overflow: 'scroll',
          }}
        >
          {opening.map(({ row, index }, tick) => (
            <Button
              key={row.id}
              ref={(element) => {
                railTicks.current[tick] = element;
              }}
              onMouseEnter={() => setRailHover(tick)}
              onMouseLeave={() => setRailHover(undefined)}
              onFocus={() => setRailHover(tick)}
              onBlur={() => setRailHover(undefined)}
              onClick={() => reveal(index)}
              onKeyDown={(event) => {
                const modifiers = event.modifiers;
                if (modifiers?.ctrl || modifiers?.cmd || modifiers?.alt || modifiers?.shift) return;
                const key = event.key?.toLowerCase();
                const next =
                  key === 'up'
                    ? Math.max(0, tick - 1)
                    : key === 'down'
                      ? Math.min(opening.length - 1, tick + 1)
                      : key === 'home'
                        ? 0
                        : key === 'end'
                          ? opening.length - 1
                          : undefined;
                if (next === undefined) return;
                event.preventDefault();
                event.stopPropagation();
                const target = railTicks.current[next];
                if (target) {
                  renderer?.focusElement?.(target.id);
                  renderer?.scrollIntoView?.(target.id);
                }
              }}
              aria-label={`Turn ${tick + 1}`}
              style={{ width: 44, height: 12, borderRadius: 4 }}
            >
              <div
                style={{
                  width:
                    railHover === undefined
                      ? 8
                      : ([32, 21.76, 14.08][Math.abs(railHover - tick)] ?? 8),
                  height: 2,
                  borderRadius: 1,
                  backgroundColor: tick === railHover ? colors.text : colors.text_ghost,
                  opacity: tick === railHover ? 1 : 0.45,
                }}
              />
            </Button>
          ))}
        </div>
      )}
      {railHover !== undefined && opening[railHover]?.row.kind === 'message' && (
        <div
          style={{
            position: 'absolute',
            left: 76,
            top: Math.max(
              12,
              Math.min(height - 138, (height - opening.length * 12) / 2 + railHover * 12),
            ),
            width: 320,
            maxHeight: 126,
            borderRadius: 14,
            borderWidth: 1,
            borderColor: colors.border_strong,
            backgroundColor: colors.raised,
            paddingLeft: 15,
            paddingRight: 15,
            paddingTop: 12,
            paddingBottom: 12,
          }}
        >
          <text
            style={{
              color: colors.text,
              fontSize: sp(14),
              fontWeight: '600',
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
            }}
          >
            {'text' in opening[railHover]!.row.entry ? opening[railHover]!.row.entry.text : ''}
          </text>
        </div>
      )}
      {detached && (
        <IconButton
          name="arrow-down"
          size={16}
          label="Scroll to bottom"
          onClick={followTail}
          style={{
            position: 'absolute',
            bottom: 8,
            left: Math.max(0, width / 2 - 16),
            width: 32,
            height: 32,
            borderRadius: 16,
            borderColor: colors.border_strong,
            backgroundColor: colors.composer,
          }}
        />
      )}
      {findOpen && (
        <div
          style={{
            position: 'absolute',
            top: 8,
            right: 12,
            width: Math.max(260, Math.min(430, width - 24)),
            maxWidth: '100%',
            display: 'flex',
            flexDirection: 'row',
            alignItems: 'center',
            gap: 5,
            padding: 6,
            borderRadius: 8,
            borderWidth: 1,
            borderColor: colors.border_strong,
            backgroundColor: colors.raised,
          }}
        >
          <input
            ref={input}
            value={query}
            placeholder="Find"
            onChange={(event) => setQuery(event.value ?? '')}
            onKeyDown={(event) => {
              if (event.key?.toLowerCase() === 'enter') {
                event.preventDefault();
                navigate(!!event.modifiers?.shift);
              } else if (event.key?.toLowerCase() === 'escape') {
                event.preventDefault();
                closeFind();
              }
            }}
            style={{
              width: Math.max(104, Math.max(260, Math.min(430, width - 24)) - 154),
              color: colors.text,
              fontSize: sp(13),
            }}
          />
          <text
            style={{
              minWidth: 62,
              flexGrow: 1,
              color: query && !matches.length ? colors.danger : colors.text_tertiary,
              fontSize: sp(12.5),
            }}
          >
            {query
              ? matches.length
                ? `${search.active + 1} of ${matches.length}${matches.length === 20000 ? '+' : ''}`
                : 'No results'
              : ''}
          </text>
          <IconButton
            name="arrow-up"
            label="Previous match"
            disabled={!matches.length}
            onClick={() => navigate(true)}
          />
          <IconButton
            name="arrow-down"
            label="Next match"
            disabled={!matches.length}
            onClick={() => navigate(false)}
          />
          <IconButton name="x" label="Close find" onClick={closeFind} />
        </div>
      )}
    </div>
  );
}

function WorkingDots() {
  const { colors, reducedMotion } = useTheme();
  const [phase, setPhase] = useState(0);
  useEffect(() => {
    if (reducedMotion) return;
    const timer = setInterval(() => setPhase((Date.now() % 1400) / 1400), 67);
    return () => clearInterval(timer);
  }, [reducedMotion]);
  return (
    <div style={{ display: 'flex', flexDirection: 'row', gap: 3.5 }}>
      {[0, 1, 2].map((dot) => (
        <div
          key={dot}
          style={{
            width: 4.5,
            height: 4.5,
            borderRadius: 3,
            backgroundColor: colors.text_tertiary,
            opacity: reducedMotion
              ? [1, 0.6, 0.25][dot]
              : 0.25 + 0.75 * ((Math.cos((phase - dot * 0.18) * Math.PI * 2) + 1) / 2),
          }}
        />
      ))}
    </div>
  );
}

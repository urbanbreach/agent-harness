import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useGpuix, type PublicInstance } from '@gpuix/react';
import {
  Button,
  Icon,
  IconButton,
  Menu,
  MenuChip,
  Spinner,
  TextField,
  Tooltip,
  ellipsis,
  row,
  useToast,
  type MenuItem,
} from '../../ui';
import { useTheme, CODE_FONT_FAMILY } from '../../theme';
import {
  collectReviewDiff,
  expandReviewContext,
  gitCommand,
  inspectBranches,
  type ReviewSource,
} from '../../workspace/git';
import { listCheckpointRefs } from '../../workspace/checkpoints';
import { highlightCode } from '../../syntax';
import { getTurnCheckpoint } from '../transcript/checkpoints';
import {
  foldContext,
  parseReviewPatch,
  reviewTree,
  type DiffLine,
  type ReviewData,
  type ReviewFile,
} from './review-model';
import { FileIcon } from './shared';

export { CommitDialog } from './commit';
type Selection = { cwd: string } | { sessionId: string; turnId: string };
const selections = new Set<(selection: Selection) => void>();
let queuedSelection: Selection | undefined;
export function openReviewTurn(sessionId: string, turnId: string) {
  const selection = { sessionId, turnId };
  queuedSelection = selection;
  for (const listener of selections) listener(selection);
}
export function openUncommittedReview(cwd: string) {
  const selection = { cwd };
  queuedSelection = selection;
  for (const listener of selections) listener(selection);
}
const gitChanges = new Set<() => void>();
export function notifyGitChanges() {
  for (const listener of gitChanges) listener();
}

export function GitChangesChip({ cwd, onOpen }: { cwd: string; onOpen: () => void }) {
  const { colors, sp } = useTheme();
  const [counts, setCounts] = useState({ additions: 0, deletions: 0 });
  useEffect(() => {
    let active = true;
    const controller = new AbortController();
    const refresh = () => {
      void inspectBranches(cwd, controller.signal)
        .then((value) => {
          if (active) setCounts(value ?? { additions: 0, deletions: 0 });
        })
        .catch(() => {});
    };
    setCounts({ additions: 0, deletions: 0 });
    refresh();
    gitChanges.add(refresh);
    const timer = setInterval(refresh, 5000);
    return () => {
      active = false;
      controller.abort();
      clearInterval(timer);
      gitChanges.delete(refresh);
    };
  }, [cwd]);
  if (!counts.additions && !counts.deletions) return null;
  return (
    <Tooltip content="Changes">
      <Button
        aria-label="Changes"
        onClick={() => {
          openUncommittedReview(cwd);
          onOpen();
        }}
        style={{
          height: 28,
          paddingLeft: 7,
          paddingRight: 7,
          borderRadius: 7,
          gap: 6,
          flexShrink: 0,
        }}
      >
        {counts.additions > 0 && (
          <text
            style={{
              fontSize: sp(12.5),
              fontWeight: 500,
              color: colors.success,
              whiteSpace: 'nowrap',
              flexShrink: 0,
            }}
          >
            {`+${counts.additions.toLocaleString()}`}
          </text>
        )}
        {counts.deletions > 0 && (
          <text
            style={{
              fontSize: sp(12.5),
              fontWeight: 500,
              color: colors.danger,
              whiteSpace: 'nowrap',
              flexShrink: 0,
            }}
          >
            {`-${counts.deletions.toLocaleString()}`}
          </text>
        )}
      </Button>
    </Tooltip>
  );
}

type DocumentRow = { file: ReviewFile; line?: DiffLine };
const labels = {
  uncommitted: 'Uncommitted',
  unstaged: 'Unstaged',
  staged: 'Staged',
  committed: 'Committed',
  branch: 'Branch',
};
type ExpandedContext = Record<string, Record<string, { above: number; below: number }>>;
interface SavedReview {
  source: ReviewSource;
  snapshot?: ReviewData;
  query: string;
  collapsed: Set<string>;
  selected?: string;
  cursor: number;
  expanded: ExpandedContext;
  treeWidth: number;
  visible: number;
}
const savedReviews = new Map<string, SavedReview>();
export function Review(props: { cwd: string; sessionId: string | null }) {
  return <ReviewBody key={`${props.cwd}\0${props.sessionId ?? ''}`} {...props} />;
}
function ReviewBody({ cwd, sessionId }: { cwd: string; sessionId: string | null }) {
  const { colors, sp, codeFontSize } = useTheme();
  const { renderer } = useGpuix();
  const { showToast } = useToast();
  const key = `${cwd}\0${sessionId ?? ''}`;
  const initial = useRef(savedReviews.get(key)).current;
  const [source, setSource] = useState<ReviewSource>(initial?.source ?? 'uncommitted');
  const [latest, setLatest] = useState<number>();
  const [snapshot, setSnapshot] = useState<ReviewData | undefined>(initial?.snapshot);
  const snapshotRef = useRef<ReviewData | undefined>(initial?.snapshot);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  const [menu, setMenu] = useState(false);
  const [query, setQuery] = useState(initial?.query ?? '');
  const [collapsed, setCollapsed] = useState<Set<string>>(initial?.collapsed ?? new Set());
  const [selected, setSelected] = useState<string | undefined>(initial?.selected);
  const [cursor, setCursor] = useState(initial?.cursor ?? 0);
  const [expanded, setExpanded] = useState<ExpandedContext>(initial?.expanded ?? {});
  const [window, setWindow] = useState({
    start: Math.max(0, (initial?.visible ?? 0) - 30),
    end: (initial?.visible ?? 0) + 100,
  });
  const [visible, setVisible] = useState(initial?.visible ?? 0);
  const [pinnedTop, setPinnedTop] = useState(0);
  const fileHeaders = useRef(new Map<string, PublicInstance>());
  const [treeWidth, setTreeWidth] = useState(initial?.treeWidth ?? 220);
  const drag = useRef<{ x: number; width: number } | undefined>(undefined);
  const list = useRef<PublicInstance>(null);
  const tree = useRef<PublicInstance>(null);
  const navigationRefs = useRef(new Map<string, PublicInstance>());
  const [treeFocused, setTreeFocused] = useState(false);
  const anchor = useRef<{ path: string; line?: DiffLine; offset: number } | undefined>(undefined);
  const request = useRef(0);
  const controller = useRef<AbortController | undefined>(undefined);
  useEffect(() => {
    savedReviews.set(key, {
      source,
      snapshot,
      query,
      collapsed,
      selected,
      cursor,
      expanded,
      treeWidth,
      visible,
    });
  }, [key, source, snapshot, query, collapsed, selected, cursor, expanded, treeWidth, visible]);
  const refresh = useCallback(
    async (chosen: ReviewSource) => {
      const serial = ++request.current;
      controller.current?.abort();
      const abort = new AbortController();
      controller.current = abort;
      setLoading(true);
      setError(undefined);
      try {
        const captured = await collectReviewDiff(cwd, chosen, abort.signal);
        const hydrated = await expandReviewContext(captured, 2_147_483_647, abort.signal);
        const base = hydrated.patch.length > 32 * 1024 * 1024 ? captured : hydrated;
        const names = (
          await gitCommand(
            base.root,
            [
              'diff',
              '--no-ext-diff',
              '--no-renames',
              '--name-status',
              '-z',
              base.baseRevision,
              base.targetRevision,
              '--',
            ],
            { signal: abort.signal },
          )
        ).stdout.split('\0');
        const statuses: NonNullable<ReviewData['statuses']> = {};
        for (let index = 0; index + 1 < names.length; index += 2)
          statuses[names[index + 1]!] =
            names[index] === 'A' ? 'A' : names[index] === 'D' ? 'D' : 'M';
        const complete: ReviewData = { ...base, statuses };
        if (serial !== request.current) return;
        snapshotRef.current = complete;
        setSnapshot(complete);
        setExpanded({});
        setSelected((old) =>
          complete.files.some((file) => file.path === old) ? old : complete.files[0]?.path,
        );
        setCollapsed(
          (old) =>
            new Set(
              [...old].filter((path) =>
                complete.files.some((file) => file.path.startsWith(`${path}/`)),
              ),
            ),
        );
        setWindow({ start: 0, end: 100 });
        setVisible(0);
        if (list.current) renderer?.scrollToItem?.(list.current.id, 0);
      } catch (failure) {
        if (serial !== request.current || abort.signal.aborted) return;
        const message = failure instanceof Error ? failure.message : String(failure);
        if (snapshotRef.current) {
          setSource(snapshotRef.current.source);
          showToast(`Could not refresh changes: ${message}`);
        } else setError(message);
      } finally {
        if (serial === request.current) setLoading(false);
      }
    },
    [cwd, renderer, showToast],
  );
  useEffect(() => {
    let active = true;
    const initialRequest = request.current;
    if (initial?.snapshot) {
      setLoading(false);
    } else if (!(
      queuedSelection &&
      ('cwd' in queuedSelection
        ? queuedSelection.cwd === cwd
        : queuedSelection.sessionId === sessionId)
    )) {
      void (async () => {
        let chosen: ReviewSource = initial?.source ?? 'uncommitted';
        if (!initial && sessionId) {
          try {
            const refs = await listCheckpointRefs(cwd, sessionId);
            const ready = refs.flatMap((ref) => {
              const match = /\/(\d+)-end$/.exec(ref);
              return match && Number(match[1]) > 0 && refs.includes(ref.replace(/-end$/, '-start'))
                ? [Number(match[1])]
                : [];
            });
            if (ready.length)
              chosen = { kind: 'lastTurn', session: sessionId, turn: Math.max(...ready) };
          } catch {
            /* Review reports repository errors through its snapshot request. */
          }
        }
        if (active && request.current === initialRequest) {
          setSource(chosen);
          void refresh(chosen);
        }
      })();
    }
    const receive = (selection: Selection) => {
      if ('cwd' in selection) {
        if (selection.cwd === cwd) {
          queuedSelection = undefined;
          choose('uncommitted');
        }
      } else if (selection.sessionId === sessionId) {
        queuedSelection = undefined;
        const expected = ++request.current;
        controller.current?.abort();
        setLoading(true);
        void getTurnCheckpoint(selection.sessionId, selection.turnId)
          .then((checkpoint) => {
            if (!active || request.current !== expected) return;
            if (!checkpoint || checkpoint.state !== 'ready')
              throw new Error('The turn checkpoint is unavailable');
            const chosen: ReviewSource = {
              kind: 'lastTurn',
              session: selection.sessionId,
              turn: checkpoint.turn,
            };
            choose(chosen);
          })
          .catch((failure) => {
            if (!active || request.current !== expected) return;
            setLoading(false);
            showToast(failure instanceof Error ? failure.message : String(failure));
          });
      }
    };
    selections.add(receive);
    if (queuedSelection) receive(queuedSelection);
    if (initial?.snapshot && list.current)
      renderer?.scrollToItem?.(list.current.id, initial.visible);
    const changed = () => {
      void refresh(sourceRef.current);
    };
    gitChanges.add(changed);
    return () => {
      active = false;
      selections.delete(receive);
      gitChanges.delete(changed);
      request.current++;
      controller.current?.abort();
    };
  }, [cwd, sessionId, refresh, showToast]);
  const sourceRef = useRef(source);
  sourceRef.current = source;
  useEffect(() => {
    if (!sessionId) return;
    const abort = new AbortController();
    const update = async () => {
      try {
        const refs = await listCheckpointRefs(cwd, sessionId, abort.signal);
        const ready = refs.flatMap((ref) => {
          const match = /\/(\d+)-end$/.exec(ref);
          return match && Number(match[1]) > 0 && refs.includes(ref.replace(/-end$/, '-start'))
            ? [Number(match[1])]
            : [];
        });
        if (!abort.signal.aborted) setLatest(ready.length ? Math.max(...ready) : undefined);
      } catch (failure) {
        if (!abort.signal.aborted)
          showToast(failure instanceof Error ? failure.message : String(failure));
      }
    };
    void update();
    const timer = setInterval(() => void update(), 5000);
    return () => {
      abort.abort();
      clearInterval(timer);
    };
  }, [cwd, sessionId, showToast]);
  const files = useMemo(() => (snapshot ? parseReviewPatch(snapshot) : []), [snapshot]);
  const rows = useMemo(
    () =>
      files
        .flatMap((file) => [
          { file } as DocumentRow,
          ...foldContext(file.lines, expanded[file.path]).map((line) => ({ file, line })),
        ])
        .slice(0, 50_000),
    [files, expanded],
  );
  const navigation = useMemo(() => reviewTree(files, query, collapsed), [files, query, collapsed]);
  const pinned = rows[Math.min(visible, rows.length - 1)]?.file;
  useLayoutEffect(() => {
    if (!anchor.current || !list.current) return;
    const saved = anchor.current;
    anchor.current = undefined;
    const index = rows.findIndex(
      (item) =>
        item.file.path === saved.path &&
        (item.line === saved.line ||
          (item.line?.kind === 'gap' &&
            saved.line?.kind === 'gap' &&
            item.line.id === saved.line.id)),
    );
    if (index >= 0) renderer?.scrollToItem?.(list.current.id, index, saved.offset);
  }, [rows, renderer]);
  useEffect(() => {
    const target = navigationRefs.current.get(navigation[cursor]?.path ?? '');
    if (treeFocused && target) renderer?.scrollIntoView?.(target.id);
  }, [cursor, navigation, treeFocused, renderer]);
  function jump(path: string) {
    setSelected(path);
    const index = rows.findIndex((item) => item.file.path === path);
    if (index >= 0 && list.current) {
      setWindow({ start: Math.max(0, index - 20), end: index + 100 });
      renderer?.scrollToItem?.(list.current.id, index);
    }
  }
  function activate(index: number) {
    const item = navigation[index];
    if (!item) return;
    if (tree.current) renderer?.focusElement?.(tree.current.id);
    setCursor(index);
    if (item.directory)
      setCollapsed((old) => {
        const next = new Set(old);
        if (next.has(item.path)) next.delete(item.path);
        else next.add(item.path);
        return next;
      });
    else jump(item.path);
  }
  function choose(chosen: ReviewSource) {
    controller.current?.abort();
    snapshotRef.current = undefined;
    setSnapshot(undefined);
    setSelected(undefined);
    setCollapsed(new Set());
    setExpanded({});
    setSource(chosen);
    void refresh(chosen);
  }
  const items: MenuItem[] = [
    {
      id: 'last',
      label: 'Last Turn',
      disabled: latest === undefined,
      selected: typeof source === 'object' && source.turn === latest,
      onSelect: () => {
        if (sessionId && latest !== undefined)
          choose({ kind: 'lastTurn', session: sessionId, turn: latest });
      },
    },
    { id: 'first-separator', type: 'separator' },
    ...(['uncommitted', 'unstaged', 'staged'] as const).map((id) => ({
      id,
      label: labels[id],
      selected: source === id,
      onSelect: () => choose(id),
    })),
    { id: 'second-separator', type: 'separator' },
    ...(['committed', 'branch'] as const).map((id) => ({
      id,
      label: labels[id],
      selected: source === id,
      onSelect: () => choose(id),
    })),
  ];
  const lineHeight = Math.round(codeFontSize * 1.5);
  const gutter = Math.round(codeFontSize * 3 + 14);
  function fileHeader(file: ReviewFile) {
    return (
      <div
        style={{
          ...row,
          height: 36,
          paddingLeft: 12,
          paddingRight: 12,
          gap: 8,
          backgroundColor: colors.surface,
          borderBottomWidth: 1,
          borderColor: colors.border,
        }}
      >
        <FileIcon path={file.path} size={14} />
        <Tooltip content={file.path}>
          <text
            style={{
              ...ellipsis,
              flexGrow: 1,
              fontSize: 12.5,
              fontWeight: 500,
              color: colors.text_secondary,
            }}
          >
            {file.path}
          </text>
        </Tooltip>
        <text style={{ fontSize: 12.5, color: colors.success }}>
          {`+${file.additions.toLocaleString()}`}
        </text>
        <text style={{ fontSize: 12.5, color: colors.danger }}>
          {`-${file.deletions.toLocaleString()}`}
        </text>
      </div>
    );
  }
  function renderLine(file: ReviewFile, line: DiffLine) {
    if (line.kind === 'gap') {
      if (!line.lines.length)
        return (
          <div style={{ ...row, height: 32, backgroundColor: colors.overlay }}>
            <div
              style={{ width: gutter, height: 32, borderRightWidth: 1, borderColor: colors.border }}
            />
            <text style={{ fontSize: 12.5, color: colors.text_tertiary, paddingLeft: 12 }}>
              {line.text}
            </text>
          </div>
        );
      const reveal = (direction: 'above' | 'below' | 'both', all: boolean) => {
        if (list.current) {
          const top = renderer?.getListScrollTop?.(list.current.id);
          const item = rows[top?.[0] ?? visible];
          if (item)
            anchor.current = { path: item.file.path, line: item.line, offset: top?.[1] ?? 0 };
        }
        setExpanded((old) => {
          const current = old[file.path]?.[line.id] ?? { above: 0, below: 0 };
          const count = all ? line.lines.length : 100;
          return {
            ...old,
            [file.path]: {
              ...old[file.path],
              [line.id]: {
                above: current.above + (direction !== 'below' ? count : 0),
                below: current.below + (direction !== 'above' ? count : 0),
              },
            },
          };
        });
      };
      return (
        <div style={{ ...row, height: 32, backgroundColor: colors.overlay }}>
          <div
            style={{
              width: gutter,
              height: 32,
              borderRightWidth: 1,
              borderColor: colors.border,
              flexShrink: 0,
            }}
          >
            {!line.leading && !line.trailing && line.lines.length > 100 ? (
              <>
                <IconButton
                  name="chevron-up"
                  label="Show 100 lines above · Shift-click for all"
                  size={11}
                  style={{ height: 16, width: gutter }}
                  onKeyDown={(event) => {
                    if (
                      event.modifiers?.shift &&
                      ['enter', 'space', ' '].includes(event.key ?? '')
                    ) {
                      event.preventDefault();
                      reveal('both', true);
                    }
                  }}
                  onClick={(event) => reveal('above', Boolean(event.modifiers?.shift))}
                />
                <IconButton
                  name="chevron-down"
                  label="Show 100 lines below · Shift-click for all"
                  size={11}
                  style={{
                    height: 16,
                    width: gutter,
                    borderTopWidth: 1,
                    borderColor: colors.border,
                  }}
                  onKeyDown={(event) => {
                    if (
                      event.modifiers?.shift &&
                      ['enter', 'space', ' '].includes(event.key ?? '')
                    ) {
                      event.preventDefault();
                      reveal('both', true);
                    }
                  }}
                  onClick={(event) => reveal('below', Boolean(event.modifiers?.shift))}
                />
              </>
            ) : (
              <IconButton
                name={
                  line.leading ? 'chevron-up' : line.trailing ? 'chevron-down' : 'chevrons-up-down'
                }
                label="Expand unmodified lines · Shift-click for all"
                size={11}
                style={{ width: gutter, height: 32 }}
                onKeyDown={(event) => {
                  if (event.modifiers?.shift && ['enter', 'space', ' '].includes(event.key ?? '')) {
                    event.preventDefault();
                    reveal('both', true);
                  }
                }}
                onClick={(event) =>
                  reveal(
                    line.leading ? 'below' : line.trailing ? 'above' : 'both',
                    Boolean(event.modifiers?.shift),
                  )
                }
              />
            )}
          </div>
          <Button
            aria-label="Expand unmodified lines · Shift-click for all"
            onClick={(event) =>
              reveal(
                line.leading ? 'below' : line.trailing ? 'above' : 'both',
                Boolean(event.modifiers?.shift),
              )
            }
            onKeyDown={(event) => {
              if (event.modifiers?.shift && ['enter', 'space', ' '].includes(event.key ?? '')) {
                event.preventDefault();
                reveal('both', true);
              }
            }}
            style={{
              height: 32,
              paddingLeft: 12,
              paddingRight: 12,
              flexGrow: 1,
              justifyContent: 'flex-start',
            }}
          >
            <text style={{ fontSize: 12.5, color: colors.text_tertiary }}>{line.text}</text>
          </Button>
        </div>
      );
    }
    const added = line.kind === 'added';
    const removed = line.kind === 'removed';
    const special = line.kind === 'hunk' || line.kind === 'meta';
    const hue = added
      ? colors.is_dark
        ? '98,201,135'
        : '47,143,82'
      : colors.is_dark
        ? '226,114,106'
        : '198,74,66';
    return (
      <div
        style={{
          ...row,
          alignItems: 'stretch',
          minHeight: special ? 24 : lineHeight,
          backgroundColor:
            added || removed
              ? `rgba(${hue},${colors.is_dark ? '.20' : '.12'})`
              : line.kind === 'hunk'
                ? colors.overlay
                : undefined,
        }}
      >
        <div
          style={{
            width: gutter,
            flexShrink: 0,
            borderRightWidth: 1,
            borderColor: colors.border,
            position: 'relative',
            paddingRight: 9,
            backgroundColor:
              added || removed ? `rgba(${hue},${colors.is_dark ? '.15' : '.09'})` : undefined,
          }}
        >
          {(added || removed) && (
            <div
              style={{
                position: 'absolute',
                left: 0,
                top: 0,
                bottom: 0,
                width: 2,
                backgroundColor: added ? colors.success : colors.danger,
              }}
            />
          )}
          <text
            style={{
              fontFamily: CODE_FONT_FAMILY,
              fontSize: codeFontSize,
              lineHeight,
              textAlign: 'right',
              color: colors.text_ghost,
            }}
          >
            {special ? '' : (line.next ?? line.old)}
          </text>
        </div>
        <rich-text
          text={line.text || ' '}
          runs={
            special ? [] : highlightCode(line.text, file.path.split('.').at(-1), colors.is_dark)
          }
          style={{
            flexGrow: 1,
            minWidth: 0,
            flexShrink: 1,
            paddingLeft: 12,
            paddingRight: 10,
            paddingTop: special ? 4 : 0,
            paddingBottom: special ? 4 : 0,
            fontFamily: CODE_FONT_FAMILY,
            fontSize: special ? 12.5 : codeFontSize,
            lineHeight: special ? 16 : lineHeight,
            color: special ? colors.text_tertiary : colors.text_secondary,
            whiteSpace: 'normal',
            userSelect: 'text',
            selectionColor: colors.selection,
          }}
        />
      </div>
    );
  }
  return (
    <div
      onMouseMove={(event) => {
        if (drag.current)
          setTreeWidth(
            Math.max(140, Math.min(360, drag.current.width - ((event.x ?? 0) - drag.current.x))),
          );
      }}
      onMouseUp={() => {
        drag.current = undefined;
      }}
      style={{
        display: 'flex',
        flexDirection: 'column',
        flexGrow: 1,
        minHeight: 0,
        minWidth: 0,
        backgroundColor: colors.surface,
      }}
    >
      <div
        style={{
          ...row,
          height: 44,
          flexShrink: 0,
          paddingLeft: 12,
          paddingRight: 12,
          gap: 8,
          borderBottomWidth: 1,
          borderColor: colors.border,
        }}
      >
        <div>
          <MenuChip
            label={
              typeof source === 'string'
                ? labels[source]
                : source.turn === latest
                  ? 'Last Turn'
                  : `Turn ${source.turn}`
            }
            open={menu}
            onClick={() => setMenu(!menu)}
            style={{ height: 28, backgroundColor: colors.surface }}
          />
          {menu && <Menu items={items} onClose={() => setMenu(false)} />}
        </div>
        <text style={{ fontSize: sp(12.5), fontWeight: 500, color: colors.success }}>
          {`+${snapshot?.additions.toLocaleString() ?? 0}`}
        </text>
        <text style={{ fontSize: sp(12.5), fontWeight: 500, color: colors.danger }}>
          {`-${snapshot?.deletions.toLocaleString() ?? 0}`}
        </text>
        {snapshot?.truncated && (
          <text style={{ ...ellipsis, fontSize: sp(12), color: colors.warning }}>
            Large diff · showing first 50,000 lines
          </text>
        )}
        <div style={{ flexGrow: 1 }} />
        <Tooltip content="Refresh changes">
          <Button
            aria-label="Refresh changes"
            onClick={() => void refresh(source)}
            style={{ height: 28, width: 28, borderRadius: 7 }}
          >
            {loading ? <Spinner size={12} /> : <Icon name="rotate-cw" size={12} />}
          </Button>
        </Tooltip>
      </div>
      {!snapshot || !files.length ? (
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            flexGrow: 1,
            justifyContent: 'center',
            alignItems: 'center',
            padding: 20,
            paddingBottom: 32,
            gap: 6,
          }}
        >
          <text style={{ fontSize: sp(13), fontWeight: 500, color: colors.text }}>
            {!snapshot ? (error ? 'Changes unavailable' : 'Loading changes…') : 'No changes'}
          </text>
          <text
            style={{
              maxWidth: 300,
              textAlign: 'center',
              fontSize: sp(12.5),
              lineHeight: sp(17),
              color: colors.text_tertiary,
            }}
          >
            {!snapshot
              ? (error ?? 'Capturing a consistent Git snapshot for review')
              : 'This source has no file changes to review'}
          </text>
        </div>
      ) : (
        <div style={{ ...row, flexGrow: 1, minHeight: 0, alignItems: 'stretch' }}>
          <div
            style={{
              display: 'flex',
              flexDirection: 'column',
              flexGrow: 1,
              minWidth: 0,
              minHeight: 0,
              position: 'relative',
              overflow: 'hidden',
            }}
          >
            <virtual-list
              ref={list}
              itemCount={rows.length}
              windowStart={window.start}
              estimatedItemHeight={lineHeight}
              overdraw={600}
              onVisibleRange={(event) => {
                const start = event.startIndex ?? 0;
                const end = event.endIndex ?? start + 50;
                setVisible(start);
                setWindow({ start: Math.max(0, start - 30), end: Math.min(rows.length, end + 50) });
                const next = rows.slice(start + 1, end + 1).find((item) => !item.line);
                const header = next && fileHeaders.current.get(next.file.path);
                const bounds = header && renderer?.getElementBounds?.(header.id);
                const viewport = list.current && renderer?.getElementBounds?.(list.current.id);
                setPinnedTop(bounds && viewport ? Math.min(0, bounds.y - viewport.y - 36) : 0);
              }}
              style={{
                flexGrow: 1,
                minHeight: 0,
                scrollbarColor: colors.text_ghost,
                scrollbarWidth: 6,
              }}
            >
              {rows.slice(window.start, window.end).map((item, index) => (
                <div
                  key={`${item.file.path}:${window.start + index}`}
                  ref={
                    item.line
                      ? undefined
                      : (value) => {
                          if (value) fileHeaders.current.set(item.file.path, value);
                          else fileHeaders.current.delete(item.file.path);
                        }
                  }
                >
                  {item.line ? renderLine(item.file, item.line) : fileHeader(item.file)}
                </div>
              ))}
            </virtual-list>
            {pinned && rows[visible]?.line && (
              <div
                onScroll={(event) => {
                  if (!list.current) return;
                  const top = renderer?.getListScrollTop?.(list.current.id);
                  if (top)
                    renderer?.scrollToItem?.(
                      list.current.id,
                      top[0]!,
                      top[1]! - (event.deltaY ?? 0) * (event.precise ? 1 : lineHeight),
                    );
                }}
                style={{ position: 'absolute', top: pinnedTop, left: 0, right: 0 }}
              >
                {fileHeader(pinned)}
              </div>
            )}
          </div>
          <div
            style={{
              width: 1,
              flexShrink: 0,
              backgroundColor: colors.border,
              position: 'relative',
            }}
          >
            <div
              role="separator"
              aria-label="Resize diff file tree"
              onMouseDown={(event) => {
                if (event.button === 0) drag.current = { x: event.x ?? 0, width: treeWidth };
              }}
              onMouseMove={(event) => {
                if (drag.current)
                  setTreeWidth(
                    Math.max(
                      140,
                      Math.min(360, drag.current.width - ((event.x ?? 0) - drag.current.x)),
                    ),
                  );
              }}
              onMouseUp={() => {
                drag.current = undefined;
              }}
              style={{
                position: 'absolute',
                width: 10,
                left: -5,
                top: 0,
                bottom: 0,
                cursor: 'col-resize',
                hover: { backgroundColor: colors.resize_handle },
              }}
            />
          </div>
          <div
            style={{
              display: 'flex',
              flexDirection: 'column',
              width: treeWidth,
              maxWidth: '48%',
              minWidth: 140,
              flexShrink: 0,
              minHeight: 0,
            }}
          >
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                flexShrink: 0,
                height: 44,
                paddingLeft: 8,
                paddingRight: 8,
                justifyContent: 'center',
                borderBottomWidth: 1,
                borderColor: colors.border,
              }}
            >
              <TextField
                icon="search"
                placeholder="Filter files…"
                aria-label="Filter files"
                value={query}
                onChange={(value) => {
                  setQuery(value);
                  setCursor(0);
                }}
              />
            </div>
            <div
              ref={tree}
              role="tree"
              aria-label="Changed files"
              tabIndex={0}
              onFocus={() => setTreeFocused(true)}
              onBlur={() => setTreeFocused(false)}
              onKeyDown={(event) => {
                const key = event.key;
                if (
                  !['up', 'down', 'left', 'right', 'home', 'end', 'enter', 'space', ' '].includes(
                    key ?? '',
                  )
                )
                  return;
                event.preventDefault();
                if (key === 'up' || key === 'down' || key === 'home' || key === 'end')
                  setCursor((old) =>
                    key === 'home'
                      ? 0
                      : key === 'end'
                        ? navigation.length - 1
                        : Math.max(
                            0,
                            Math.min(navigation.length - 1, old + (key === 'up' ? -1 : 1)),
                          ),
                  );
                else if (key === 'left' || key === 'right') {
                  const item = navigation[cursor];
                  if (!item) return;
                  if (key === 'right' && item.directory) {
                    if (!query.trim() && collapsed.has(item.path))
                      setCollapsed((old) => {
                        const next = new Set(old);
                        next.delete(item.path);
                        return next;
                      });
                    else if (navigation[cursor + 1]?.depth === item.depth + 1)
                      setCursor(cursor + 1);
                  } else if (key === 'left') {
                    if (item.directory && !collapsed.has(item.path) && !query.trim())
                      setCollapsed((old) => new Set([...old, item.path]));
                    else {
                      const parent = item.path.slice(0, item.path.lastIndexOf('/'));
                      const index = navigation.findIndex((row) => row.path === parent);
                      if (index >= 0) setCursor(index);
                    }
                  }
                } else activate(cursor);
              }}
              style={{
                flexGrow: 1,
                minHeight: 0,
                overflowY: 'scroll',
                scrollbarColor: colors.text_ghost,
                scrollbarWidth: 6,
                paddingTop: 4,
                paddingBottom: 4,
                borderWidth: 1,
                borderColor: 'transparent',
                focusVisible: { borderColor: colors.accent },
              }}
            >
              {navigation.map((item, index) => {
                const status = item.file?.status;
                const statusColor =
                  status === 'A' ? colors.success : status === 'D' ? colors.danger : colors.warning;
                return (
                  <div
                    key={item.path}
                    ref={(value) => {
                      if (value) navigationRefs.current.set(item.path, value);
                      else navigationRefs.current.delete(item.path);
                    }}
                    style={{
                      display: 'flex',
                      flexDirection: 'column',
                      height: 30,
                      paddingLeft: 6,
                      paddingRight: 6,
                      justifyContent: 'center',
                    }}
                  >
                    <Button
                      tabIndex={-1}
                      role="treeitem"
                      aria-label={item.path}
                      aria-selected={selected === item.path}
                      aria-expanded={
                        item.directory
                          ? query.trim()
                            ? true
                            : !collapsed.has(item.path)
                          : undefined
                      }
                      onClick={() => activate(index)}
                      style={{
                        height: 26,
                        justifyContent: 'flex-start',
                        paddingLeft: (item.directory ? 7 : 23) + item.depth * 14,
                        paddingRight: 7,
                        gap: 6,
                        borderRadius: 5,
                        backgroundColor:
                          treeFocused &&
                          cursor === index &&
                          (item.directory || selected === item.path)
                            ? colors.overlay_strong
                            : (treeFocused && cursor === index) || selected === item.path
                              ? colors.overlay
                              : undefined,
                      }}
                    >
                      {item.directory ? (
                        <>
                          <Icon
                            name={
                              !query.trim() && collapsed.has(item.path)
                                ? 'chevron-right'
                                : 'chevron-down'
                            }
                            size={10}
                            color={colors.text_ghost}
                          />
                          <Icon name="folder" size={13} />
                        </>
                      ) : (
                        <FileIcon path={item.path} size={13} />
                      )}
                      <Tooltip content={item.path}>
                        <text
                          style={{
                            ...ellipsis,
                            flexGrow: 1,
                            fontSize: sp(12.5),
                            color: selected === item.path ? colors.text : colors.text_secondary,
                          }}
                        >
                          {item.name}
                        </text>
                      </Tooltip>
                      {status && (
                        <div
                          style={{
                            ...row,
                            width: 18,
                            height: 18,
                            borderRadius: 4,
                            borderWidth: 1,
                            borderColor: `${statusColor}a6`,
                            justifyContent: 'center',
                            flexShrink: 0,
                          }}
                        >
                          <text style={{ fontSize: sp(12.5), fontWeight: 600, color: statusColor }}>
                            {status}
                          </text>
                        </div>
                      )}
                    </Button>
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

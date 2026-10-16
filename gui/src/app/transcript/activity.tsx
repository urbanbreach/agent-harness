import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useGpuix, type PublicInstance, type StyleDesc } from '@gpuix/react';
import type {
  CompactionEntry,
  SessionView,
  TextEntry,
  ToolActivity,
  TranscriptEntry,
} from '../../model/session';
import type { ToolDiffFile, ToolOutput } from '../../model/tools';
import { Markdown, parseMarkdown } from '../../markdown';
import { useNativeRenderer } from '../../native';
import { CODE_FONT_FAMILY, useTheme } from '../../theme';
import { Button, Icon, IconButton, Tooltip, ellipsis, row } from '../../ui/controls';
import { Scrollbar } from '../../ui/scrollbar';
import { openPanelFile } from '../panel/index';
import { runAction } from '../actions';
import { highlightCode } from '../../syntax';

const column: StyleDesc = {
  display: 'flex',
  flexDirection: 'column',
  minWidth: 0,
  width: '100%',
  flexShrink: 0,
};
const complete = (tool: ToolActivity) => ['succeeded', 'failed', 'cancelled'].includes(tool.status);
const print = (value: unknown): string =>
  typeof value === 'string' ? value : value == null ? '' : JSON.stringify(value, null, 2);
const object = (value: unknown): Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};

function thoughtTitle(entry: TextEntry) {
  const seconds = Math.max(
    1,
    Math.ceil(((entry.endedMs ?? entry.startedMs) - entry.startedMs) / 1000),
  );
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const unit = (n: number, label: string) => `${n} ${label}${n === 1 ? '' : 's'}`;
  const duration = hours
    ? [unit(hours, 'hour'), minutes % 60 ? unit(minutes % 60, 'minute') : '']
        .filter(Boolean)
        .join(' ')
    : minutes
      ? [unit(minutes, 'minute'), seconds % 60 ? unit(seconds % 60, 'second') : '']
          .filter(Boolean)
          .join(' ')
      : unit(seconds, 'second');
  return entry.streaming ? 'Thinking' : `Thought for ${duration}`;
}

/** A nested viewport follows independently of the transcript, including thumb drags. */
function DetailViewport({
  children,
  revision,
  follow = false,
  padded = false,
}: {
  children: ReactNode;
  revision: unknown;
  follow?: boolean;
  padded?: boolean;
}) {
  const { colors } = useTheme();
  const { renderer } = useGpuix();
  const viewport = useRef<PublicInstance>(null);
  const pinned = useRef(true);
  const [size, setSize] = useState({ viewport: 0, content: 0, offset: 0 });
  const range = Math.max(0, size.content - size.viewport);
  function scroll(offset: number) {
    pinned.current = range - offset <= 0.5;
    if (viewport.current) renderer?.scrollTo?.(viewport.current.id, 0, -offset);
  }
  useLayoutEffect(() => {
    if (follow && pinned.current && viewport.current)
      renderer?.scrollTo?.(viewport.current.id, 0, -range);
  }, [renderer, follow, revision, range]);
  return (
    <div style={{ ...column, position: 'relative', maxHeight: 400, overflow: 'hidden' }}>
      <div
        ref={viewport}
        onLayout={(e) => setSize((s) => ({ ...s, viewport: e.height ?? 0 }))}
        onScrollChange={(e) => {
          const offset = Math.max(0, -(e.scrollY ?? 0));
          pinned.current = range - offset <= 0.5;
          setSize((s) => ({ ...s, offset }));
        }}
        style={{
          ...column,
          flexShrink: 1,
          maxHeight: 400,
          overflow: 'scroll',
          pointerEvents: 'auto',
        }}
      >
        <div
          onLayout={(e) => setSize((s) => ({ ...s, content: e.height ?? 0 }))}
          style={{
            ...column,
            paddingTop: padded ? 4 : 0,
            paddingBottom: padded ? 4 : 0,
            paddingRight: padded ? 8 : 0,
          }}
        >
          {children}
        </div>
      </div>
      <Scrollbar {...size} onScroll={scroll} edgeFades surface={colors.surface} />
    </div>
  );
}

function CopySection({
  label,
  content,
  children,
  command = false,
  live = false,
}: {
  label: string;
  content: string;
  children?: ReactNode;
  command?: boolean;
  live?: boolean;
}) {
  const { colors, sp, codeFontSize } = useTheme();
  const native = useNativeRenderer();
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(timer);
  }, [copied]);
  if (!content.trim() && !children) return null;
  const body = (
    <>
      <text
        style={{
          color: colors.text_secondary,
          fontFamily: CODE_FONT_FAMILY,
          fontSize: codeFontSize,
          lineHeight: Math.round(codeFontSize * 3) / 2,
          whiteSpace: 'normal',
          userSelect: 'text',
        }}
      >
        {content}
      </text>
      {children}
    </>
  );
  return (
    <div style={{ ...column, gap: 3 }}>
      <div style={{ ...row, height: 20, gap: 8 }}>
        <text
          style={{
            flexGrow: 1,
            color: colors.text_secondary,
            fontFamily: label === 'Command' || command ? CODE_FONT_FAMILY : undefined,
            fontWeight: 500,
            fontSize: sp(12.5),
          }}
        >
          {label}
        </text>
        {content.trim() && (
          <Tooltip content={copied ? 'Copied' : `Copy ${label.toLowerCase()}`}>
            <IconButton
              name={copied ? 'check' : 'copy'}
              label={copied ? 'Copied' : `Copy ${label.toLowerCase()}`}
              size={11}
              color={colors.text_ghost}
              style={{ width: 20, height: 20, borderRadius: 5 }}
              onClick={() => {
                native.writeClipboardText(content);
                setCopied(true);
              }}
            />
          </Tooltip>
        )}
      </div>
      {command ? (
        <DetailViewport revision={content} follow={live} padded>
          {body}
        </DetailViewport>
      ) : (
        body
      )}
    </div>
  );
}

function ActivityCard({
  action,
  detail,
  icon,
  children,
  defaultOpen = false,
  failed = false,
  running = false,
  extras,
}: {
  action: string;
  detail: string;
  icon: string;
  children?: ReactNode;
  defaultOpen?: boolean;
  failed?: boolean;
  running?: boolean;
  extras?: ReactNode;
}) {
  const { colors, sp } = useTheme();
  const [override, setOverride] = useState<boolean>();
  const open = override ?? defaultOpen;
  const header = (
    <>
      <Icon name={icon} size={12} />
      <text style={{ fontWeight: 600, flexShrink: 0 }}>{action}</text>
      <text style={{ ...ellipsis, flexGrow: 1 }}>{detail}</text>
      {failed ? <Icon name="x" size={10} color={colors.danger} /> : running ? <Pulse /> : null}
    </>
  );
  return (
    <div
      style={{
        ...column,
        borderRadius: 9,
        borderWidth: 1,
        borderColor: colors.border_strong,
        backgroundColor: colors.overlay.replace(/0\.05\)/, '0.035)'),
        overflow: 'hidden',
      }}
    >
      <div style={{ ...row, minWidth: 0 }}>
        {children ? (
          <Button
            aria-expanded={open}
            aria-label={`${action}: ${detail}`}
            onClick={() => setOverride(!open)}
            style={{
              ...row,
              flexGrow: 1,
              minWidth: 0,
              justifyContent: 'flex-start',
              height: 28,
              paddingLeft: 8,
              paddingRight: 8,
              gap: 8,
              fontSize: sp(12.5),
              lineHeight: sp(16),
              color: colors.text_secondary,
            }}
          >
            {header}
          </Button>
        ) : (
          <div
            style={{
              ...row,
              flexGrow: 1,
              minWidth: 0,
              height: 28,
              paddingLeft: 8,
              paddingRight: 8,
              gap: 8,
              fontSize: sp(12.5),
              color: colors.text_secondary,
            }}
          >
            {header}
          </div>
        )}
        {extras}
        {children && (
          <div style={{ ...row, paddingRight: 8 }}>
            <Icon name={open ? 'chevron-down' : 'chevron-right'} size={10} />
          </div>
        )}
      </div>
      {children && open && (
        <div style={{ ...column, borderTopWidth: 1, borderColor: colors.border_strong }}>
          {children}
        </div>
      )}
    </div>
  );
}

function Pulse() {
  const { colors, reducedMotion } = useTheme();
  const [opacity, setOpacity] = useState(1);
  useEffect(() => {
    if (reducedMotion) return;
    const start = Date.now();
    const timer = setInterval(
      () => setOpacity(0.65 + Math.cos(((Date.now() - start) * Math.PI) / 800) * 0.35),
      66,
    );
    return () => clearInterval(timer);
  }, [reducedMotion]);
  return (
    <div
      role="status"
      aria-label="In progress"
      style={{
        width: 5,
        height: 5,
        borderRadius: 5,
        flexShrink: 0,
        backgroundColor: colors.accent,
        opacity: reducedMotion ? 1 : opacity,
      }}
    />
  );
}

function Reasoning({ entry, live }: { entry: TextEntry; live: boolean }) {
  const active = live && entry.streaming;
  const anchor = useRef(0);
  const source = useMemo(() => {
    if (!active) return entry.text;
    if (entry.text.length - anchor.current > 18 * 1024 || anchor.current > entry.text.length) {
      anchor.current = Math.max(0, entry.text.length - 6 * 1024);
      const boundary = entry.text.indexOf('\n', anchor.current);
      if (boundary >= 0) anchor.current = boundary + 1;
    }
    let tail = entry.text.slice(anchor.current);
    // Keep even pathological streams bounded without changing stored reasoning.
    while (parseMarkdown(tail, { streaming: true }).length > 48) {
      const boundary = tail.indexOf('\n');
      if (boundary < 0) break;
      tail = tail.slice(boundary + 1);
    }
    return tail;
  }, [entry.text, active]);
  return (
    <ActivityCard
      action="Think"
      icon="sparkle"
      detail={thoughtTitle(entry)}
      running={active}
      defaultOpen={active}
    >
      {entry.text.trim() ? (
        <div
          style={{ ...column, paddingLeft: 12, paddingRight: 12, paddingTop: 8, paddingBottom: 8 }}
        >
          <DetailViewport revision={source} follow={active}>
            <Markdown source={source} context="compact" reasoning />
          </DetailViewport>
        </div>
      ) : null}
    </ActivityCard>
  );
}

function outputText(output?: ToolOutput): string {
  if (!output) return '';
  switch (output.kind) {
    case 'command':
      return [
        output.text || [output.stdout, output.stderr].filter(Boolean).join('\n'),
        output.exitCode === undefined ? '' : `Exit code: ${output.exitCode}`,
        output.truncated ? '… output truncated' : '',
      ]
        .filter(Boolean)
        .join('\n\n');
    case 'diff':
      return output.error || '';
    case 'search':
      return [
        output.text ||
          output.groups
            .map((group) =>
              [
                group.path,
                ...group.rows.map(
                  (row) =>
                    `${row.line === undefined ? '' : `${row.line}:`}${row.text}${row.count === undefined ? '' : ` (${row.count})`}`,
                ),
              ].join('\n'),
            )
            .join('\n\n'),
        output.truncated ? '… output truncated' : '',
      ]
        .filter(Boolean)
        .join('\n\n');
    case 'web':
      return [
        output.text,
        ...output.sources.map(
          (source) => `${source.title ? `${source.title}: ` : ''}${source.url}`,
        ),
      ]
        .filter(Boolean)
        .join('\n');
    case 'mcp':
      return output.text || output.error || '';
    case 'json': {
      const value = object(output.value);
      if (Array.isArray(value.content))
        return value.content
          .map((part) =>
            object(part).type === 'image'
              ? ''
              : typeof object(part).text === 'string'
                ? String(object(part).text)
                : print(part),
          )
          .filter(Boolean)
          .join('\n');
      return print(output.value);
    }
    case 'text':
      return output.text;
  }
}

function filePatch(file: ToolDiffFile): string {
  if (file.diff)
    return file.diff.startsWith('diff --git ') || file.diff.startsWith('--- ')
      ? file.diff
      : `--- a/${file.path}\n+++ b/${file.path}\n${file.diff}`;
  if (file.before === undefined || file.after === undefined || file.before === file.after)
    return '';
  const before = file.before ? file.before.replace(/\n$/, '').split('\n') : [];
  const after = file.after ? file.after.replace(/\n$/, '').split('\n') : [];
  let start = 0;
  while (start < before.length && start < after.length && before[start] === after[start]) start++;
  let oldEnd = before.length,
    newEnd = after.length;
  while (oldEnd > start && newEnd > start && before[oldEnd - 1] === after[newEnd - 1]) {
    oldEnd--;
    newEnd--;
  }
  const contextStart = Math.max(0, start - 3);
  const contextEnd = Math.min(before.length - oldEnd, 3);
  return `--- a/${file.path}\n+++ b/${file.path}\n@@ -${contextStart + 1},${oldEnd - contextStart + contextEnd} +${contextStart + 1},${newEnd - contextStart + contextEnd} @@\n${before
    .slice(contextStart, start)
    .map((line) => ` ${line}`)
    .concat(before.slice(start, oldEnd).map((line) => `-${line}`))
    .concat(after.slice(start, newEnd).map((line) => `+${line}`))
    .concat(before.slice(oldEnd, oldEnd + contextEnd).map((line) => ` ${line}`))
    .join('\n')}\n`;
}

function diffLines(patch: string) {
  let before = 0;
  let after = 0;
  return patch.split('\n').flatMap((line) => {
    const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line);
    if (hunk) {
      before = Number(hunk[1]);
      after = Number(hunk[2]);
      return [];
    }
    if ((!before && !after) || /^(---|\+\+\+|\\)/.test(line) || !line) return [];
    const kind = line[0];
    if (kind !== '+' && kind !== '-' && kind !== ' ') return [];
    const number = kind === '-' ? before : after;
    if (kind !== '+') before++;
    if (kind !== '-') after++;
    return [{ kind, number, text: line.slice(1) }];
  });
}

function InlineDiff({ files }: { files: ToolDiffFile[] }) {
  const { colors, codeFontSize, sp } = useTheme();
  const patches = useMemo(() => {
    let remaining = 400,
      hidden = 0;
    const visible = files.flatMap((file) => {
      const patch = filePatch(file);
      if (!patch) return [];
      const lines = patch.split('\n');
      const rows = lines.filter(
        (line) => !/^(diff --git |index |--- |\+\+\+ |@@)/.test(line) && line !== '',
      );
      let count = 0;
      const clipped: string[] = [];
      for (const line of lines) {
        const body = !/^(diff --git |index |--- |\+\+\+ |@@)/.test(line) && line !== '';
        if (body && count++ >= remaining) continue;
        if (count <= remaining) clipped.push(line);
      }
      hidden += Math.max(0, rows.length - remaining);
      remaining = Math.max(0, remaining - rows.length);
      return [{ file, patch: clipped.join('\n') }];
    });
    return { visible, hidden };
  }, [files]);
  return (
    <DetailViewport revision={patches}>
      {patches.visible.map(({ file, patch }) => (
        <div key={file.path} style={column}>
          {files.length > 1 && (
            <div style={{ ...row, gap: 8, padding: 8 }}>
              <text
                style={{
                  ...ellipsis,
                  flexGrow: 1,
                  fontSize: sp(12.5),
                  color: colors.text_secondary,
                }}
              >
                {file.path}
              </text>
              <DiffCounts files={[file]} />
              <OpenFile path={file.path} />
            </div>
          )}
          {file.before === undefined || file.after === undefined ? (
            <diff
              patch={patch}
              wordDiff
              style={{
                minWidth: 0,
                fontFamily: CODE_FONT_FAMILY,
                fontSize: codeFontSize,
                lineHeight: Math.round(codeFontSize * 3) / 2,
              }}
            />
          ) : (
            diffLines(patch).map((line, index) => (
              <div
                key={index}
                style={{
                  ...row,
                  fontFamily: CODE_FONT_FAMILY,
                  fontSize: codeFontSize,
                  lineHeight: Math.round(codeFontSize * 3) / 2,
                  backgroundColor:
                    line.kind === '+'
                      ? colors.is_dark
                        ? '#344a37'
                        : '#d7ecd7'
                      : line.kind === '-'
                        ? colors.is_dark
                          ? '#4a3030'
                          : '#f7dada'
                        : undefined,
                  borderLeftWidth: 2,
                  borderColor:
                    line.kind === '+'
                      ? colors.success
                      : line.kind === '-'
                        ? colors.danger
                        : 'transparent',
                }}
              >
                <text
                  style={{
                    width: 52,
                    paddingRight: 10,
                    textAlign: 'right',
                    color:
                      line.kind === '+'
                        ? colors.success
                        : line.kind === '-'
                          ? colors.danger
                          : colors.text_tertiary,
                  }}
                >
                  {line.number}
                </text>
                <rich-text
                  text={line.text || ' '}
                  runs={highlightCode(
                    line.text || ' ',
                    file.path.split('.').at(-1),
                    colors.is_dark,
                  )}
                  style={{
                    flexGrow: 1,
                    paddingLeft: 12,
                    color: colors.text_secondary,
                    userSelect: 'text',
                  }}
                />
              </div>
            ))
          )}
        </div>
      ))}
      {patches.hidden > 0 && (
        <text style={{ padding: 8, color: colors.text_tertiary, fontSize: sp(12.5) }}>
          {patches.hidden} more {patches.hidden === 1 ? 'line' : 'lines'} · open Review to read the
          whole change
        </text>
      )}
      {files
        .filter((file) => !filePatch(file))
        .map((file) => (
          <text
            key={file.path}
            style={{ padding: 8, color: colors.text_tertiary, fontSize: sp(12.5) }}
          >
            {file.artifact
              ? `Diff stored in ${file.artifact}`
              : `No inline diff available for ${file.path}`}
          </text>
        ))}
    </DetailViewport>
  );
}

function DiffCounts({ files }: { files: ToolDiffFile[] }) {
  const { colors, sp } = useTheme();
  if (
    !files.length ||
    files.some((file) => file.additions === undefined || file.deletions === undefined)
  )
    return null;
  return (
    <div style={{ ...row, gap: 6, fontSize: sp(12.5), flexShrink: 0 }}>
      <text style={{ color: colors.success, whiteSpace: 'nowrap' }}>
        {`+${files.reduce((sum, file) => sum + file.additions!, 0)}`}
      </text>
      <text style={{ color: colors.danger, whiteSpace: 'nowrap' }}>
        {`−${files.reduce((sum, file) => sum + file.deletions!, 0)}`}
      </text>
    </div>
  );
}

function OpenFile({ path }: { path: string }) {
  return (
    <Tooltip content="Open file">
      <IconButton
        name="file-bottom-left-arrow"
        label={`Open file ${path}`}
        size={14}
        style={{ width: 20, height: 20, borderRadius: 5, marginRight: 6 }}
        onClick={() => openPanelFile(path)}
      />
    </Tooltip>
  );
}

function ToolImages({ tool }: { tool: ToolActivity }) {
  const { colors } = useTheme();
  const raw = object(tool.output);
  const content = Array.isArray(raw.content) ? raw.content : [];
  const images = [
    ...(tool.metadata?.attachments ?? [])
      .filter((item) => item.mime.startsWith('image/'))
      .map((item) => ({ id: item.id, ref: item.content_ref })),
    ...content.flatMap((item, index) => {
      const part = object(item);
      const mime = String(part.mimeType ?? part.mime_type ?? '');
      return part.type === 'image'
        ? [
            {
              id: `output-${index}`,
              ref:
                typeof part.data === 'string' && mime.startsWith('image/')
                  ? `data:${mime};base64,${part.data}`
                  : String(part.content_ref ?? ''),
            },
          ]
        : [];
    }),
  ];
  return (
    <>
      {images.map((image) =>
        /^(data:image\/|file:\/\/)/.test(image.ref) ? (
          <img
            key={image.id}
            src={image.ref}
            objectFit="contain"
            style={{ width: 300, maxWidth: '100%', maxHeight: 200, marginTop: 8, borderRadius: 4 }}
          />
        ) : (
          <div
            key={image.id}
            role="img"
            aria-label="Image output unavailable"
            style={{
              ...row,
              width: 300,
              maxWidth: '100%',
              height: 80,
              marginTop: 8,
              borderRadius: 4,
              justifyContent: 'center',
              backgroundColor: colors.inset,
            }}
          >
            <Icon name="file" size={18} color={colors.text_ghost} />
          </div>
        ),
      )}
    </>
  );
}

function ToolCard({ tool, view, live }: { tool: ToolActivity; view: SessionView; live: boolean }) {
  const { colors, sp } = useTheme();
  const presentation = tool.presentation;
  const output = presentation.output;
  const files = output?.kind === 'diff' ? output.files : [];
  const hasDiff = files.some((file) => Boolean(filePatch(file)));
  const suppressRaw = hasDiff && tool.status === 'succeeded';
  const command = presentation.kind === 'command';
  const args = suppressRaw
    ? ''
    : command
      ? output?.kind === 'command'
        ? output.command || presentation.target || ''
        : presentation.target || ''
      : output?.kind === 'mcp'
        ? print(output.arguments)
        : print(tool.args) || tool.inputDelta || tool.argsSummary;
  const result = suppressRaw ? '' : outputText(output) || tool.outputSummary || '';
  const server = output?.kind === 'mcp' ? output.server : undefined;
  const nativeName = tool.metadata?.canonical_tool_id || tool.toolId;
  const images = Boolean(
    tool.metadata?.attachments?.some((image) => image.mime.startsWith('image/')) ||
    (Array.isArray(object(tool.output).content) &&
      (object(tool.output).content as unknown[]).some((item) => object(item).type === 'image')),
  );
  const explicitTask = object(tool.output).task_id ?? object(tool.args).task_id;
  const background = Object.values(view.tasks).find(
    (task) =>
      task.background &&
      (task.id === explicitTask || object(task.lineage).parent_tool_call_id === tool.id),
  );
  const expandable = Boolean(args.trim() || result.trim() || files.length || images || server);
  const icon = command
    ? 'terminal'
    : ['edit', 'write'].includes(presentation.kind)
      ? 'pencil'
      : presentation.action === 'Read'
        ? 'file'
        : ['search', 'web'].includes(presentation.kind)
          ? 'search'
          : presentation.kind === 'list'
            ? 'folder'
            : ['plan', 'todo'].includes(presentation.kind)
              ? 'list'
              : 'wrench';
  const detail =
    presentation.detail?.toLowerCase() === 'failed'
      ? result.split('\n').find((line) => line.trim()) || presentation.detail
      : presentation.detail ||
        presentation.target ||
        (images ? 'Image output' : presentation.title);
  return (
    <ActivityCard
      action={presentation.action}
      detail={detail || ''}
      icon={icon}
      failed={tool.status === 'failed'}
      running={!complete(tool)}
      extras={
        <>
          {tool.status === 'succeeded' && <DiffCounts files={files} />}
          {background && (
            <Tooltip content="Inspect background work">
              <Button
                aria-label={`Inspect background work: ${background.summary || background.id}`}
                onClick={() => runAction(`transcript.background:${background.id}`)}
                style={{
                  height: 20,
                  borderRadius: 5,
                  borderColor: colors.border_strong,
                  marginRight: 6,
                  paddingLeft: 6,
                  paddingRight: 6,
                  fontSize: sp(12.5),
                  color: ['failed', 'cancelled', 'timed_out'].includes(background.state)
                    ? colors.danger
                    : background.state === 'completed'
                      ? colors.success
                      : colors.text_secondary,
                }}
              >
                <text>{background.state.replace(/_/g, ' ')}</text>
              </Button>
            </Tooltip>
          )}
          {files.length === 1 && <OpenFile path={files[0]!.path} />}
        </>
      }
    >
      {expandable ? (
        <>
          {(!suppressRaw || server) && (
            <div
              style={{
                ...column,
                paddingLeft: 12,
                paddingRight: 12,
                paddingTop: 8,
                paddingBottom: 8,
                gap: 8,
              }}
            >
              {server && (
                <div style={{ ...row, gap: 8, fontSize: sp(12.5) }}>
                  <text style={{ color: colors.text_tertiary }}>MCP server:</text>
                  <text style={{ color: colors.text_secondary }}>{server}</text>
                </div>
              )}
              {server && (
                <div style={{ ...row, gap: 8, fontSize: sp(12.5) }}>
                  <text style={{ color: colors.text_tertiary }}>Tool:</text>
                  <text style={{ color: colors.text_secondary, userSelect: 'text' }}>
                    {nativeName}
                  </text>
                </div>
              )}
              <CopySection label={command ? 'Command' : 'Arguments'} content={args} />
              <CopySection
                label="Output"
                content={result}
                command={command}
                live={live && !complete(tool)}
              >
                {images ? <ToolImages tool={tool} /> : undefined}
              </CopySection>
            </div>
          )}
          {files.length > 0 && (
            <div style={{ ...column, borderTopWidth: 1, borderColor: colors.border_strong }}>
              <InlineDiff files={files} />
            </div>
          )}
        </>
      ) : null}
    </ActivityCard>
  );
}

function ContextCard({ entry }: { entry: CompactionEntry }) {
  const { colors, sp } = useTheme();
  const event = entry.event;
  const data = object(event.data);
  const summary = [data.short_summary, data.summary, data.message, data.reason].find(
    (value) => typeof value === 'string' && value.trim(),
  ) as string | undefined;
  const failed = event.event_type === 'compaction_failed';
  const running = event.event_type === 'compaction_requested';
  const title = failed
    ? 'Context compaction failed'
    : running
      ? 'Compacting context'
      : event.event_type === 'context_pruned'
        ? 'Pruned context'
        : event.event_type === 'branch_summary'
          ? 'Branch summary'
          : 'Compacted context';
  const before = typeof data.tokens_before === 'number' ? data.tokens_before : undefined;
  const after = typeof data.tokens_after === 'number' ? data.tokens_after : undefined;
  return (
    <ActivityCard
      icon="history"
      action="Context"
      detail={`${title}${entry.superseded ? ' · superseded' : ''}`}
      failed={failed}
      running={running}
    >
      {summary || entry.warning || (before !== undefined && after !== undefined) ? (
        <div style={{ ...column, padding: 12, gap: 8 }}>
          {before !== undefined && after !== undefined && (
            <text style={{ color: colors.text_secondary, fontSize: sp(12.5) }}>
              {`${before.toLocaleString()} → ${after.toLocaleString()} tokens`}
            </text>
          )}
          {summary && <Markdown source={summary} context="compact" />}
          {entry.warning && <Markdown source={entry.warning} context="compact" />}
          {entry.readerViews &&
            Object.entries(entry.readerViews).map(([key, value]) =>
              typeof object(value).summary === 'string' ? (
                <Markdown key={key} source={String(object(value).summary)} context="compact" />
              ) : null,
            )}
        </div>
      ) : null}
    </ActivityCard>
  );
}

export function ActivityGroup({
  entries,
  view,
  live,
}: {
  entries: TranscriptEntry[];
  view: SessionView;
  live: boolean;
}) {
  const { colors, sp } = useTheme();
  const [override, setOverride] = useState<boolean>();
  const open = override ?? live;
  const latest = entries[entries.length - 1];
  const counts = new Map<string, number>();
  for (const entry of entries) {
    const kind =
      entry.kind === 'tool' ? view.tools[entry.toolCallId]?.presentation.kind : entry.kind;
    const noun =
      kind === 'reasoning'
        ? 'thought'
        : kind === 'command'
          ? 'command'
          : kind === 'read'
            ? 'file read'
            : kind === 'edit' || kind === 'write'
              ? 'file edit'
              : kind === 'search'
                ? 'file search'
                : kind === 'list'
                  ? 'file listing'
                  : kind === 'web'
                    ? 'search'
                    : kind === 'plan' || kind === 'todo'
                      ? 'plan step'
                      : kind === 'compaction'
                        ? 'context update'
                        : kind === 'notice'
                          ? 'notice'
                          : 'tool call';
    counts.set(noun, (counts.get(noun) ?? 0) + 1);
  }
  const summary = [...counts]
    .map(
      ([noun, count]) =>
        `${count} ${noun}${count === 1 ? '' : noun === 'search' || noun === 'file search' ? 'es' : 's'}`,
    )
    .join(' · ');
  const title =
    live && latest?.kind === 'reasoning'
      ? thoughtTitle(latest)
      : live && latest?.kind === 'tool'
        ? view.tools[latest.toolCallId]?.presentation.title || 'Working'
        : `${live ? 'Running' : 'Ran'} ${summary}`;
  if (!entries.length) return null;
  return (
    <div style={column}>
      <Button
        aria-expanded={open}
        aria-label={title}
        onClick={() => setOverride(!open)}
        style={{
          ...row,
          justifyContent: 'flex-start',
          width: '100%',
          height: 26,
          gap: 6,
          padding: 0,
          fontSize: sp(12.5),
          lineHeight: sp(16),
          fontWeight: 500,
          color: colors.text_secondary,
          hover: { color: colors.text },
        }}
      >
        <text style={ellipsis}>{title}</text>
        <Icon name={open ? 'chevron-down' : 'chevron-right'} size={10} />
      </Button>
      {open && (
        <div
          style={{
            ...column,
            width: 'auto',
            marginTop: 4,
            marginLeft: 6,
            marginRight: -6,
            paddingLeft: 12,
            paddingBottom: 2,
            borderLeftWidth: 1,
            borderColor: colors.border,
            gap: 8,
          }}
        >
          {entries.map((entry) =>
            entry.kind === 'reasoning' ? (
              <Reasoning key={entry.id} entry={entry} live={live} />
            ) : entry.kind === 'tool' ? (
              view.tools[entry.toolCallId] ? (
                <ToolCard
                  key={entry.id}
                  tool={view.tools[entry.toolCallId]!}
                  view={view}
                  live={live}
                />
              ) : null
            ) : entry.kind === 'compaction' ? (
              <ContextCard key={entry.id} entry={entry} />
            ) : entry.kind === 'notice' ? (
              <div
                key={entry.id}
                style={{
                  ...row,
                  gap: 8,
                  padding: 8,
                  color:
                    entry.level === 'error'
                      ? colors.danger
                      : entry.level === 'warning'
                        ? colors.warning
                        : colors.text_tertiary,
                }}
              >
                <Icon name={entry.level === 'info' ? 'info' : 'triangle-alert'} size={12} />
                <text style={{ fontSize: sp(12.5), userSelect: 'text' }}>{entry.message}</text>
              </div>
            ) : entry.kind === 'text' ? (
              <Markdown key={entry.id} source={entry.text} context="compact" />
            ) : null,
          )}
        </div>
      )}
    </div>
  );
}

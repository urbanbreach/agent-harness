import { useEffect, useRef, useState } from 'react';
import { useWindowSize, type EventPayload, type KeyEvent, type PublicInstance } from '@gpuix/react';
import { basename, resolve } from 'node:path';
import { homedir } from 'node:os';
import { access } from 'node:fs/promises';
import { Menu, Scrollbar, useToast } from '../../ui';
import { useTheme } from '../../theme';
import { useNativeRenderer, type Run } from '../../native';
import {
  startTerminal,
  openUrl,
  revealPath,
  type TerminalSession,
  type TerminalSnapshot,
  type TerminalPosition,
} from '../../workspace';

/** xterm key encoding; null leaves application/platform shortcuts unconsumed. */
export function terminalKey(
  event: Pick<EventPayload, 'key' | 'keyChar' | 'modifiers'>,
  applicationCursor: boolean,
): string | null {
  const key = event.key ?? '',
    m = event.modifiers;
  if (m?.cmd) return null;
  const modifier = 1 + (m?.shift ? 1 : 0) + (m?.alt ? 2 : 0) + (m?.ctrl ? 4 : 0);
  const arrows: Record<string, string> = {
    up: 'A',
    down: 'B',
    right: 'C',
    left: 'D',
    home: 'H',
    end: 'F',
  };
  if (arrows[key])
    return modifier > 1
      ? `\x1b[1;${modifier}${arrows[key]}`
      : `\x1b${applicationCursor ? 'O' : '['}${arrows[key]}`;
  const tilde: Record<string, number> = {
    insert: 2,
    delete: 3,
    pageup: 5,
    pagedown: 6,
    f5: 15,
    f6: 17,
    f7: 18,
    f8: 19,
    f9: 20,
    f10: 21,
    f11: 23,
    f12: 24,
  };
  if (tilde[key]) return `\x1b[${tilde[key]}${modifier > 1 ? `;${modifier}` : ''}~`;
  if (/^f[1-4]$/.test(key)) {
    const final = 'PQRS'[Number(key.slice(1)) - 1];
    return modifier > 1 ? `\x1b[1;${modifier}${final}` : `\x1bO${final}`;
  }
  let text: string | undefined;
  if (key === 'tab') text = m?.shift ? '\x1b[Z' : '\t';
  else if (key === 'enter') text = '\r';
  else if (key === 'escape') text = '\x1b';
  else if (key === 'backspace') text = m?.ctrl ? '\b' : '\x7f';
  else if (m?.ctrl) {
    const character = key === 'space' ? ' ' : key.toLowerCase();
    if (/^[a-z]$/.test(character)) text = String.fromCharCode(character.charCodeAt(0) - 96);
    else
      text = (
        {
          ' ': '\0',
          '@': '\0',
          '[': '\x1b',
          '\\': '\x1c',
          ']': '\x1d',
          '^': '\x1e',
          _: '\x1f',
          '?': '\x7f',
          '2': '\0',
          '3': '\x1b',
          '4': '\x1c',
          '5': '\x1d',
          '6': '\x1e',
          '7': '\x1f',
          '8': '\x7f',
        } as Record<string, string>
      )[character];
  } else
    text =
      event.keyChar || (key === 'space' ? ' ' : Array.from(key).length === 1 ? key : undefined);
  return text === undefined ? null : (m?.alt ? '\x1b' : '') + text;
}

type Selection = { anchor: TerminalPosition; end: TerminalPosition };
const compare = (a: TerminalPosition, b: TerminalPosition) => a.row - b.row || a.column - b.column;
const graphemes = new Intl.Segmenter(undefined, { granularity: 'grapheme' });
const CELL_MEASUREMENTS = ['m'.repeat(64), 'm'.repeat(128)] as const;

// Native editor bindings run before JS handlers. Intercept every terminal chord
// with an editor action, including modified navigation and function keys.
const terminalInterceptedKeys = [
  ...[
    '',
    'shift-',
    'alt-',
    'alt-shift-',
    'ctrl-',
    'ctrl-shift-',
    'ctrl-alt-',
    'ctrl-alt-shift-',
  ].flatMap((modifier) =>
    [
      'enter',
      'backspace',
      'tab',
      'escape',
      'up',
      'down',
      'left',
      'right',
      'home',
      'end',
      'insert',
      'delete',
      'pageup',
      'pagedown',
      ...Array.from({ length: 12 }, (_, index) => `f${index + 1}`),
    ].map((key) => `${modifier}${key}`),
  ),
  ...['ctrl-', 'ctrl-shift-', 'alt-', 'alt-shift-', 'ctrl-alt-', 'ctrl-alt-shift-'].flatMap(
    (modifier) =>
      Array.from('abcdefghijklmnopqrstuvwxyz0123456789@[]\\^_?', (key) => `${modifier}${key}`),
  ),
  'ctrl-space',
  'ctrl-shift-space',
  'ctrl-alt-space',
  'ctrl-alt-shift-space',
  'alt-space',
];

export function TerminalView({ cwd, active }: { cwd: string | null; active: boolean }) {
  const { colors, sp } = useTheme();
  const { showToast } = useToast();
  const native = useNativeRenderer();
  const window = useWindowSize();
  const grid = useRef<PublicInstance>(null);
  const input = useRef<PublicInstance>(null);
  const session = useRef<TerminalSession | undefined>(undefined);
  const [snapshot, setSnapshot] = useState<TerminalSnapshot>();
  const [error, setError] = useState<string>();
  const [exited, setExited] = useState(false);
  const [focused, setFocused] = useState(false);
  const [windowActive, setWindowActive] = useState(() => native.isWindowActive());
  const [blink, setBlink] = useState(true);
  const [cellWidth, setCellWidth] = useState(7.8);
  const measurementWidths = useRef<[number, number]>([0, 0]);
  const [size, setSize] = useState({ width: 425.6, height: 664 });
  const [selection, setSelection] = useState<Selection>();
  const [dragging, setDragging] = useState(false);
  const dragStart = useRef<(Selection & { extend: boolean }) | undefined>(undefined);
  const [menu, setMenu] = useState<{ x: number; y: number; paste: string | null }>();
  const [link, setLink] = useState<{ row: number; start: number; end: number; target: string }>();
  const pauseBlink = useRef(0);
  const wheel = useRef(0);
  const theme = colors.is_dark ? 'dark' : 'light';
  const themeRef = useRef<'dark' | 'light'>(theme);
  themeRef.current = theme;
  const refresh = () => {
    if (session.current) setSnapshot(session.current.screen.snapshot());
  };
  useEffect(() => {
    setSnapshot(undefined);
    setError(undefined);
    setExited(false);
    setSelection(undefined);
    if (!cwd) return;
    const controller = new AbortController();
    let owned: TerminalSession | undefined;
    void startTerminal({
      cwd,
      theme: themeRef.current,
      signal: controller.signal,
      onUpdate: () => {
        if (!controller.signal.aborted && owned) setSnapshot(owned.screen.snapshot());
      },
      onClipboard: (text) => native.writeClipboardText(text),
    })
      .then((terminal) => {
        owned = terminal;
        if (controller.signal.aborted) {
          void terminal.close();
          return;
        }
        session.current = terminal;
        setSnapshot(terminal.screen.snapshot());
        void terminal.exited.then(() => {
          if (!controller.signal.aborted) {
            setExited(true);
            setSnapshot(terminal.screen.snapshot());
          }
        });
      })
      .catch((failure: unknown) => {
        if (!controller.signal.aborted)
          setError(failure instanceof Error ? failure.message : String(failure));
      });
    return () => {
      session.current = undefined;
      controller.abort();
      void owned?.close();
    };
  }, [cwd, native]);
  useEffect(() => {
    session.current?.screen.setTheme(theme);
    refresh();
  }, [theme]);
  useEffect(() => {
    if (active && input.current) native.focusElement?.(input.current.id);
    if (!active) {
      setFocused(false);
      setMenu(undefined);
      setDragging(false);
    }
  }, [active, native, cwd]);
  const cols = Math.max(20, Math.floor((size.width - 20) / cellWidth));
  const rows = Math.max(8, Math.floor((Math.max(120, size.height) - 16) / 18));
  useEffect(() => {
    if (!active || !session.current) return;
    if (
      session.current.screen.emulator.cols !== cols ||
      session.current.screen.emulator.rows !== rows
    ) {
      setSelection(undefined);
      setLink(undefined);
      setDragging(false);
      session.current.resize(cols, rows);
    }
  }, [cols, rows, active, snapshot !== undefined]);
  useEffect(() => {
    if (!active) return;
    const timer = setInterval(() => {
      const foreground = native.isWindowActive();
      setWindowActive(foreground);
      setBlink(
        foreground && Date.now() > pauseBlink.current
          ? Math.floor((Date.now() - pauseBlink.current) / 500) % 2 === 0
          : true,
      );
    }, 50);
    return () => clearInterval(timer);
  }, [active, native]);
  const wakeCursor = () => {
    pauseBlink.current = Date.now() + 300;
    setBlink(true);
  };
  const selectedText = () =>
    selection && session.current
      ? session.current.screen.selectionText(selection.anchor, selection.end)
      : '';
  const copy = () => {
    const text = selectedText();
    if (text) native.writeClipboardText(text);
  };
  const paste = (text = native.readClipboardText()) => {
    if (!text || !session.current) return;
    setSelection(undefined);
    wakeCursor();
    session.current.paste(text);
    session.current.screen.scrollTo(session.current.screen.snapshot().scrollback);
    refresh();
  };
  const selectAll = () => {
    if (snapshot)
      setSelection({
        anchor: { row: 0, column: 0 },
        end: { row: snapshot.totalRows - 1, column: snapshot.cols },
      });
  };
  function keys(e: KeyEvent) {
    if (!active || menu || e.defaultPrevented || !session.current) return;
    if (
      e.modifiers?.ctrl &&
      e.modifiers.shift &&
      !e.modifiers.alt &&
      !e.modifiers.cmd &&
      ['c', 'v', 'a'].includes(e.key?.toLowerCase() ?? '')
    ) {
      e.preventDefault();
      e.stopPropagation();
      if (e.key?.toLowerCase() === 'c') copy();
      else if (e.key?.toLowerCase() === 'v') paste();
      else selectAll();
      return;
    }
    const data = terminalKey(e, session.current.screen.emulator.modes.applicationCursorKeysMode);
    if (data === null) return;
    e.preventDefault();
    e.stopPropagation();
    wakeCursor();
    setSelection(undefined);
    session.current.input(data);
    session.current.screen.scrollTo(session.current.screen.snapshot().scrollback);
    refresh();
  }
  function position(e: EventPayload): TerminalPosition | undefined {
    if (!grid.current || !snapshot) return;
    const bounds = native.getElementBounds?.(grid.current.id);
    if (!bounds) return;
    return {
      row:
        snapshot.viewport +
        Math.max(
          0,
          Math.min(snapshot.rows.length - 1, Math.floor(((e.y ?? bounds.y) - bounds.y - 8) / 18)),
        ),
      column: Math.max(
        0,
        Math.min(snapshot.cols - 1, Math.floor(((e.x ?? bounds.x) - bounds.x - 10) / cellWidth)),
      ),
    };
  }
  function detectLink(p: TerminalPosition) {
    const line = session.current?.screen.emulator.buffer.active.getLine(p.row);
    if (!line) return undefined;
    let text = '';
    const columns: number[] = [];
    for (let column = 0; column < (snapshot?.cols ?? 0); column++) {
      const cell = line.getCell(column);
      if (!cell || !cell.getWidth()) continue;
      const chars = cell.getChars() || ' ';
      for (let offset = 0; offset < chars.length; offset++) columns.push(column);
      text += chars;
    }
    columns.push(snapshot?.cols ?? 0);
    for (const match of text.matchAll(
      /(?:[a-z][a-z\d+.-]*:\/\/[^\s<>"']+|(?:~\/|\.{1,2}\/|\/)[^\s<>"']+|[\w.-]+\/[^\s<>"']+)/gi,
    )) {
      const target = match[0].replace(/[),.;]+$/, '');
      const start = columns[match.index]!,
        end = columns[match.index + target.length]!;
      if (p.column >= start && p.column < end) return { row: p.row, start, end, target };
    }
  }
  async function openLink(target: string) {
    try {
      if (/^[a-z][a-z\d+.-]*:/i.test(target) && !target.startsWith('file:')) await openUrl(target);
      else {
        const path = target.startsWith('file:')
          ? new URL(target).pathname
          : resolve(cwd!, target.replace(/^~\//, `${homedir()}/`).replace(/:\d+(?::\d+)?$/, ''));
        await access(path);
        await revealPath(path);
      }
    } catch (failure) {
      showToast(String(failure));
    }
  }
  function down(e: EventPayload) {
    const bounds = grid.current && native.getElementBounds?.(grid.current.id);
    if (snapshot?.scrollback && bounds && (e.x ?? 0) >= bounds.x + bounds.width - 11) return;
    if (e.button === 2) {
      setMenu({ x: e.x ?? 0, y: e.y ?? 0, paste: native.readClipboardText() });
      return;
    }
    if (e.button !== 0) return;
    if (input.current) native.focusElement?.(input.current.id);
    const p = position(e);
    if (!p) return;
    const target = e.modifiers?.ctrl ? detectLink(p) : undefined;
    if (target) {
      void openLink(target.target);
      return;
    }
    let anchor = p,
      end = p;
    if ((e.clickCount ?? 1) >= 3) {
      anchor = { row: p.row, column: 0 };
      end = { row: p.row, column: snapshot!.cols };
    } else if (e.clickCount === 2) {
      const line = session.current!.screen.emulator.buffer.active.getLine(p.row)!;
      const word = (column: number) =>
        /[\p{L}\p{N}_./~:-]/u.test(line.getCell(column)?.getChars() ?? ' ');
      let start = p.column,
        stop = p.column + 1;
      while (start > 0 && word(start - 1) === word(p.column)) start--;
      while (stop < snapshot!.cols && word(stop) === word(p.column)) stop++;
      anchor = { row: p.row, column: start };
      end = { row: p.row, column: stop };
    } else if (e.modifiers?.shift && selection) {
      anchor = selection.anchor;
      end = { ...p, column: p.column + 1 };
    }
    dragStart.current = { anchor, end, extend: Boolean(e.modifiers?.shift && selection) };
    setSelection({ anchor, end });
    setDragging(true);
  }
  function move(e: EventPayload) {
    const p = position(e);
    if (!p) return;
    if (dragging && dragStart.current) {
      if (e.pressedButton !== undefined && e.pressedButton !== 0) {
        setDragging(false);
        return;
      }
      const start = dragStart.current;
      const forward = compare(p, start.anchor) >= 0;
      setSelection({
        anchor:
          forward || start.extend
            ? start.anchor
            : compare(start.anchor, start.end) === 0
              ? { ...start.anchor, column: start.anchor.column + 1 }
              : start.end,
        end: forward
          ? !start.extend && compare(p, start.end) < 0
            ? start.end
            : { ...p, column: p.column + 1 }
          : p,
      });
    } else setLink(e.modifiers?.ctrl ? detectLink(p) : undefined);
  }
  if (!cwd)
    return (
      <div
        style={{
          flexGrow: 1,
          flexShrink: 1,
          flexBasis: 0,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          paddingBottom: 32,
          backgroundColor: colors.surface,
        }}
      >
        <text style={{ fontSize: sp(13), fontWeight: 500, color: colors.text }}>
          Terminal unavailable
        </text>
        <text
          style={{
            marginTop: 6,
            maxWidth: 300,
            fontSize: sp(12.5),
            lineHeight: sp(17),
            textAlign: 'center',
            color: colors.text_tertiary,
          }}
        >
          Open the Terminal surface again to start a shell
        </text>
      </div>
    );
  const solid = focused && active && windowActive;
  const ordered = selection
    ? compare(selection.anchor, selection.end) <= 0
      ? [selection.anchor, selection.end]
      : [selection.end, selection.anchor]
    : undefined;
  return (
    <div
      style={{
        flexGrow: 1,
        flexShrink: 1,
        flexBasis: 0,
        minWidth: 0,
        minHeight: 0,
        display: 'flex',
        flexDirection: 'column',
        backgroundColor: colors.terminal,
      }}
    >
      <div
        style={{
          height: 34,
          flexShrink: 0,
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'center',
          paddingLeft: 10,
          paddingRight: 10,
          gap: 7,
          backgroundColor: colors.surface,
          borderBottomWidth: 1,
          borderColor: colors.border,
        }}
      >
        <div
          aria-label={exited ? 'Shell exited' : 'Shell running'}
          style={{
            width: 6,
            height: 6,
            flexShrink: 0,
            borderRadius: 3,
            backgroundColor: exited ? colors.danger : colors.accent,
          }}
        />
        <text
          style={{
            flexGrow: 1,
            flexShrink: 1,
            flexBasis: 0,
            minWidth: 0,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            fontSize: sp(12.5),
            color: colors.text_secondary,
          }}
        >
          {snapshot?.title || 'Terminal'}
        </text>
        <text style={{ flexShrink: 0, fontSize: sp(12.5), color: colors.text_tertiary }}>
          {basename(cwd) || 'workspace'}
        </text>
      </div>
      <div
        ref={grid}
        tabIndex={-1}
        role="region"
        aria-label="Terminal"
        onKeyUp={(e) => {
          if (!e.modifiers?.ctrl) setLink(undefined);
        }}
        onFocus={() => setFocused(true)}
        onBlur={() => {
          setFocused(false);
          setLink(undefined);
        }}
        onLayout={(event) => {
          const width = event.width ?? size.width;
          const height = event.height ?? size.height;
          if (width !== size.width || height !== size.height) setSize({ width, height });
        }}
        onMouseDown={down}
        onMouseMove={move}
        onMouseUp={() => setDragging(false)}
        onMouseLeave={() => setLink(undefined)}
        onScroll={(e) => {
          if (!session.current || !snapshot) return;
          wheel.current += -(e.deltaY ?? 0) / (e.precise ? 18 : 1);
          const lines = Math.trunc(wheel.current);
          wheel.current -= lines;
          if (lines) {
            session.current.screen.scrollTo(
              Math.max(0, Math.min(snapshot.scrollback, snapshot.viewport + lines)),
            );
            refresh();
          }
        }}
        style={{
          flexGrow: 1,
          flexShrink: 1,
          flexBasis: 0,
          minHeight: 0,
          position: 'relative',
          overflow: 'hidden',
          cursor: link ? 'pointer' : 'text',
          backgroundColor: colors.terminal,
        }}
      >
        <input
          ref={input}
          value=""
          aria-label="Terminal input"
          readOnly
          interceptedKeys={terminalInterceptedKeys}
          onKeyDown={keys}
          tabIndex={0}
          onFocus={() => {
            setFocused(true);
            wakeCursor();
          }}
          onBlur={() => setFocused(false)}
          style={{ position: 'absolute', left: 10, top: 8, width: 1, height: 18, opacity: 0 }}
        />
        <div
          style={{
            position: 'absolute',
            width: 1000,
            height: 18,
            opacity: 0,
            pointerEvents: 'none',
          }}
        >
          {CELL_MEASUREMENTS.map((sample, index) => (
            <text
              key={index}
              onLayout={(e) => {
                if (!e.width || e.width <= 0) return;
                measurementWidths.current[index] = e.width;
                const [short, long] = measurementWidths.current;
                // Subtract the common caret overhang and whole-pixel rounding.
                if (long > short && short > 0)
                  setCellWidth((long - short) / CELL_MEASUREMENTS[0].length);
              }}
              style={{
                position: 'absolute',
                fontFamily: 'JetBrains Mono',
                fontSize: 12.5,
                lineHeight: 18,
                whiteSpace: 'nowrap',
              }}
            >
              {sample}
            </text>
          ))}
        </div>
        {!snapshot && (
          <text
            style={{
              padding: 12,
              color: error ? colors.danger : colors.text_tertiary,
              fontSize: sp(12.5),
              lineHeight: sp(17),
            }}
          >
            {error ?? 'Starting terminal…'}
          </text>
        )}
        {snapshot?.rows.map((row, y) => {
          let text = '';
          let column = 0;
          const runs: Run[] = [];
          const backgrounds: { column: number; width: number; color: string }[] = [];
          for (const run of row.runs) {
            if (run.column > column) text += ' '.repeat(run.column - column);
            column = run.column;
            for (const [index, segment] of Array.from(graphemes.segment(run.text)).entries()) {
              const at = column;
              const width = run.widths[index] ?? 1;
              column += width;
              const selected =
                ordered &&
                compare({ row: row.row, column: at + width }, ordered[0]!) > 0 &&
                compare({ row: row.row, column: at }, ordered[1]!) < 0;
              const filled =
                snapshot.cursor.visible &&
                snapshot.cursor.row === y &&
                snapshot.cursor.column >= at &&
                snapshot.cursor.column < at + width &&
                solid &&
                blink;
              const foreground = filled ? colors.terminal : selected ? colors.text : run.foreground;
              const background = filled
                ? colors.text
                : selected
                  ? colors.selection
                  : run.background;
              if (background) {
                const previous = backgrounds.at(-1);
                if (previous?.color === background && previous.column + previous.width === at)
                  previous.width += width;
                else backgrounds.push({ column: at, width, color: background });
              }
              runs.push({
                start: text.length,
                end: text.length + segment.segment.length,
                color: foreground,
                fontWeight: run.bold ? 700 : 400,
                italic: run.italic,
                underline:
                  run.underline || (link?.row === row.row && at >= link.start && at < link.end)
                    ? { color: foreground, thickness: 1 }
                    : undefined,
                strikethrough: run.strike ? { color: foreground, thickness: 1 } : undefined,
              });
              text += segment.segment;
            }
          }
          return (
            <div
              key={row.row}
              style={{
                position: 'absolute',
                top: 8 + y * 18,
                left: 10,
                width: snapshot.cols * cellWidth,
                height: 18,
                pointerEvents: 'none',
              }}
            >
              {backgrounds.map((background) => (
                <div
                  key={background.column}
                  style={{
                    position: 'absolute',
                    left: background.column * cellWidth,
                    width: background.width * cellWidth,
                    height: 18,
                    backgroundColor: background.color,
                  }}
                />
              ))}
              <rich-text
                text={text}
                runs={runs}
                style={{
                  fontFamily: 'JetBrains Mono',
                  fontSize: 12.5,
                  lineHeight: 18,
                  whiteSpace: 'nowrap',
                  color: colors.text,
                }}
              />
            </div>
          );
        })}
        {snapshot?.cursor.visible && !solid && (
          <div
            style={{
              position: 'absolute',
              left: 10 + snapshot.cursor.column * cellWidth,
              top: 8 + snapshot.cursor.row * 18,
              width: cellWidth,
              height: 18,
              borderWidth: 1,
              borderColor: colors.text,
              pointerEvents: 'none',
            }}
          />
        )}
        {snapshot && (
          <Scrollbar
            viewport={size.height}
            content={size.height + snapshot.scrollback * 18}
            offset={snapshot.viewport * 18}
            onScroll={(offset) => {
              session.current?.screen.scrollTo(Math.round(offset / 18));
              refresh();
            }}
          />
        )}
      </div>
      {dragging && (
        <anchored position={{ x: 0, y: 0 }} deferred priority={1}>
          <div
            onMouseMove={move}
            onMouseUp={() => setDragging(false)}
            style={{ width: window.width, height: window.height, cursor: 'text' }}
          />
        </anchored>
      )}
      {menu && (
        <Menu
          position={menu}
          restoreFocusId={input.current?.id}
          onClose={() => setMenu(undefined)}
          items={[
            { id: 'copy', label: 'Copy', disabled: !selectedText(), onSelect: copy },
            {
              id: 'paste',
              label: 'Paste',
              disabled: !menu.paste,
              onSelect: () => paste(menu.paste),
            },
            { id: 'separator', type: 'separator' },
            { id: 'all', label: 'Select All', disabled: !session.current, onSelect: selectAll },
          ]}
        />
      )}
    </div>
  );
}

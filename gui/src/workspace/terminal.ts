import { Terminal } from '@xterm/headless';
import { access, constants } from 'node:fs/promises';
import { homedir } from 'node:os';

export type TerminalTheme = 'dark' | 'light';
export const terminalPalettes = {
  dark: {
    foreground: '#E2E2E2',
    background: '#151515',
    selection: 'rgba(0,123,255,.55)',
    fallback: '#E5E5E5',
    ansi: [
      '#1D1F21',
      '#CC6666',
      '#B5BD68',
      '#F0C674',
      '#81A2BE',
      '#B294BB',
      '#8ABEB7',
      '#C5C8C6',
      '#666666',
      '#D54E53',
      '#B9CA4A',
      '#E7C547',
      '#7AA6DA',
      '#C397D8',
      '#70C0B1',
      '#EAEAEA',
    ],
  },
  light: {
    foreground: '#242424',
    background: '#FFFFFF',
    selection: 'rgba(0,123,255,.35)',
    fallback: '#242424',
    ansi: [
      '#000000',
      '#C82829',
      '#718C00',
      '#EAB700',
      '#4271AE',
      '#8959A8',
      '#3E999F',
      '#C7C7C7',
      '#8E908C',
      '#C82829',
      '#718C00',
      '#EAB700',
      '#4271AE',
      '#8959A8',
      '#3E999F',
      '#FFFFFF',
    ],
  },
} as const;

export interface TerminalRun {
  column: number;
  text: string;
  /** Display widths per glyph; combining marks remain attached to their glyph. */
  widths: number[];
  columns: number;
  foreground: string;
  background: string;
  bold: boolean;
  italic: boolean;
  faint: boolean;
  inverse: boolean;
  underline: boolean;
  strike: boolean;
  hidden: boolean;
}
export interface TerminalPosition {
  row: number;
  column: number;
}
export interface TerminalSnapshot {
  cols: number;
  rows: { row: number; wrapped: boolean; runs: TerminalRun[] }[];
  viewport: number;
  scrollback: number;
  totalRows: number;
  title: string;
  cursor: TerminalPosition & {
    visible: boolean;
    shape: 'block' | 'underline' | 'bar';
    blink: boolean;
  };
}

function hex(value: number): string {
  return `#${value.toString(16).padStart(6, '0').toUpperCase()}`;
}
function parseColor(value: string): string | undefined {
  if (/^#[\da-f]{6}$/i.test(value)) return value.toUpperCase();
  const rgb = /^rgb:([\da-f]{1,4})\/([\da-f]{1,4})\/([\da-f]{1,4})$/i.exec(value);
  if (!rgb) return undefined;
  return (
    '#' +
    rgb
      .slice(1)
      .map((channel) =>
        Math.round((parseInt(channel!, 16) * 255) / (16 ** channel!.length - 1))
          .toString(16)
          .padStart(2, '0'),
      )
      .join('')
      .toUpperCase()
  );
}
function queryColor(color: string): string {
  return 'rgb:' + [1, 3, 5].map((offset) => color.slice(offset, offset + 2).repeat(2)).join('/');
}
function faintColor(color: string, theme: TerminalTheme): string {
  const channels = [1, 3, 5].map((offset) => parseInt(color.slice(offset, offset + 2), 16) / 255);
  const min = Math.min(...channels),
    max = Math.max(...channels),
    lightness = (min + max) / 2;
  const target = theme === 'dark' ? lightness / 2 : (1 + lightness) / 2;
  const scale = Math.min(target, 1 - target) / (Math.min(lightness, 1 - lightness) || 1);
  return hex(
    channels.reduce(
      (rgb, channel) =>
        (rgb << 8) |
        Math.round(Math.max(0, Math.min(1, target + (channel - lightness) * scale)) * 255),
      0,
    ),
  );
}

export class TerminalScreen {
  readonly emulator: Terminal;
  private theme: TerminalTheme;
  private custom = new Map<number, string>();
  private title = 'Terminal';
  private cursorVisible = true;
  private cursorShape: 'block' | 'underline' | 'bar' = 'block';
  private cursorBlink = true;
  private pending: Promise<void> = Promise.resolve();

  constructor(
    options: {
      cols?: number;
      rows?: number;
      theme?: TerminalTheme;
      onResponse?: (data: string) => void;
      onClipboard?: (text: string) => void;
    } = {},
  ) {
    this.theme = options.theme ?? 'dark';
    this.emulator = new Terminal({
      cols: options.cols ?? 52,
      rows: options.rows ?? 36,
      scrollback: 10_000,
      allowProposedApi: true,
      logLevel: 'off',
    });
    this.emulator.onData((data) => options.onResponse?.(data));
    this.emulator.onTitleChange((title) => {
      this.title = title.trim() ? title : 'Terminal';
    });
    for (const final of ['h', 'l'])
      this.emulator.parser.registerCsiHandler({ prefix: '?', final }, (params) => {
        if (params.includes(25)) this.cursorVisible = final === 'h';
        return false;
      });
    this.emulator.parser.registerCsiHandler({ intermediates: ' ', final: 'q' }, (params) => {
      const style = typeof params[0] === 'number' ? params[0] : 0;
      this.cursorShape = style >= 5 ? 'bar' : style >= 3 ? 'underline' : 'block';
      this.cursorBlink = style === 0 || style % 2 === 1;
      return false;
    });
    this.emulator.parser.registerEscHandler({ final: 'c' }, () => {
      this.cursorVisible = true;
      this.cursorShape = 'block';
      this.cursorBlink = true;
      this.title = 'Terminal';
      this.custom.clear();
      return false;
    });
    this.emulator.parser.registerOscHandler(4, (data) => {
      const parts = data.split(';');
      for (let i = 0; i + 1 < parts.length; i += 2) {
        const index = Number(parts[i]),
          value = parts[i + 1]!;
        if (!Number.isInteger(index) || index < 0 || index > 255) continue;
        if (value === '?')
          options.onResponse?.(`\x1b]4;${index};${queryColor(this.color(index))}\x1b\\`);
        else {
          const color = parseColor(value);
          if (color) this.custom.set(index, color);
        }
      }
      return true;
    });
    for (const code of [10, 11, 12])
      this.emulator.parser.registerOscHandler(code, (data) => {
        const index = code + 246;
        if (data === '?')
          options.onResponse?.(`\x1b]${code};${queryColor(this.color(index))}\x1b\\`);
        else {
          const color = parseColor(data);
          if (color) this.custom.set(index, color);
        }
        return true;
      });
    this.emulator.parser.registerOscHandler(104, (data) => {
      if (!data) for (let i = 0; i < 256; i++) this.custom.delete(i);
      else for (const index of data.split(';')) this.custom.delete(Number(index));
      return true;
    });
    for (const code of [110, 111, 112])
      this.emulator.parser.registerOscHandler(code, () => {
        this.custom.delete(code + 146);
        return true;
      });
    this.emulator.parser.registerOscHandler(52, (data) => {
      const encoded = data.slice(data.indexOf(';') + 1);
      if (encoded !== '?' && /^[A-Za-z0-9+/]*={0,2}$/.test(encoded))
        options.onClipboard?.(Buffer.from(encoded, 'base64').toString('utf8'));
      return true;
    });
  }

  setTheme(theme: TerminalTheme): void {
    this.theme = theme;
  }
  color(index: number): string {
    const custom = this.custom.get(index);
    if (custom) return custom;
    const palette = terminalPalettes[this.theme];
    if (index === 256 || index === 258) return palette.foreground;
    if (index === 257) return palette.background;
    if (index < 16 && index >= 0) return palette.ansi[index]!;
    if (index < 232 && index >= 16) {
      const n = index - 16,
        levels = [0, 95, 135, 175, 215, 255];
      return hex(
        (levels[Math.floor(n / 36)]! << 16) |
          (levels[Math.floor(n / 6) % 6]! << 8) |
          levels[n % 6]!,
      );
    }
    if (index >= 232 && index < 256) return hex((8 + (index - 232) * 10) * 0x010101);
    return palette.fallback;
  }
  write(data: string | Uint8Array): Promise<void> {
    const { promise, resolve } = Promise.withResolvers<void>();
    this.emulator.write(data, resolve);
    this.pending = promise;
    return this.pending;
  }
  flush(): Promise<void> {
    return this.pending;
  }
  resize(cols: number, rows: number): void {
    this.emulator.resize(cols, rows);
  }
  scrollTo(row: number): void {
    this.emulator.scrollToLine(row);
  }
  dispose(): void {
    this.emulator.dispose();
  }

  snapshot(): TerminalSnapshot {
    const buffer = this.emulator.buffer.active,
      cell = buffer.getNullCell();
    const rows: TerminalSnapshot['rows'] = [];
    for (let y = 0; y < this.emulator.rows; y++) {
      const row = buffer.viewportY + y,
        line = buffer.getLine(row),
        runs: TerminalRun[] = [];
      for (let x = 0; line && x < this.emulator.cols; x++) {
        if (!line.getCell(x, cell) || cell.getWidth() === 0) continue;
        const width = cell.getWidth();
        let foreground = cell.isFgRGB()
          ? hex(cell.getFgColor())
          : this.color(cell.isFgPalette() ? cell.getFgColor() : 256);
        let background = cell.isBgRGB()
          ? hex(cell.getBgColor())
          : this.color(cell.isBgPalette() ? cell.getBgColor() : 257);
        if (cell.isInverse()) [foreground, background] = [background, foreground];
        if (cell.isDim()) foreground = faintColor(foreground, this.theme);
        const style = {
          foreground,
          background,
          bold: !!cell.isBold(),
          italic: !!cell.isItalic(),
          faint: !!cell.isDim(),
          inverse: !!cell.isInverse(),
          underline: !!cell.isUnderline(),
          strike: !!cell.isStrikethrough(),
          hidden: !!cell.isInvisible(),
        };
        const text = style.hidden ? ' '.repeat(width) : cell.getChars() || ' ';
        const last = runs.at(-1);
        if (
          last &&
          last.foreground === style.foreground &&
          last.background === style.background &&
          last.bold === style.bold &&
          last.italic === style.italic &&
          last.faint === style.faint &&
          last.inverse === style.inverse &&
          last.underline === style.underline &&
          last.strike === style.strike &&
          last.hidden === style.hidden
        ) {
          last.text += text;
          last.columns += width;
          last.widths.push(width);
        } else runs.push({ column: x, text, widths: [width], columns: width, ...style });
      }
      rows.push({ row, wrapped: line?.isWrapped ?? false, runs });
    }
    const cursorRow = buffer.baseY + buffer.cursorY - buffer.viewportY;
    return {
      cols: this.emulator.cols,
      rows,
      viewport: buffer.viewportY,
      scrollback: buffer.baseY,
      totalRows: buffer.length,
      title: this.title,
      cursor: {
        row: cursorRow,
        column: Math.min(buffer.cursorX, this.emulator.cols - 1),
        visible: this.cursorVisible && cursorRow >= 0 && cursorRow < this.emulator.rows,
        shape: this.cursorShape,
        blink: this.cursorBlink,
      },
    };
  }

  /** Absolute buffer coordinates; end is exclusive. Wrapped lines join without a newline. */
  selectionText(start: TerminalPosition, end: TerminalPosition): string {
    const buffer = this.emulator.buffer.active;
    if (start.row > end.row || (start.row === end.row && start.column > end.column))
      [start, end] = [end, start];
    if (start.row === end.row && start.column === end.column) return '';
    const first = Math.max(0, start.row),
      last = Math.min(buffer.length - 1, end.row),
      cell = buffer.getNullCell();
    let text = '';
    for (let y = first; y <= last; y++) {
      const line = buffer.getLine(y);
      if (!line) continue;
      if (y > first && !line.isWrapped) text += '\n';
      let part = '';
      let from = y === start.row ? Math.max(0, start.column) : 0;
      // A pointer can land on the spacer half of a wide glyph.
      while (from > 0 && line.getCell(from)?.getWidth() === 0) from--;
      const to = y === end.row ? Math.min(this.emulator.cols, end.column) : this.emulator.cols;
      for (let x = from; x < to; x++) {
        if (!line.getCell(x, cell) || cell.getWidth() === 0) continue;
        part += cell.isInvisible() ? ' '.repeat(cell.getWidth()) : cell.getChars() || ' ';
      }
      text += buffer.getLine(y + 1)?.isWrapped && y < last ? part : part.trimEnd();
    }
    return text;
  }
}

async function usableShell(path: string | undefined): Promise<boolean> {
  if (!path?.startsWith('/')) return false;
  try {
    await access(path, constants.X_OK);
    return true;
  } catch {
    return false;
  }
}
export async function resolveLoginShell(
  signal?: AbortSignal,
  env: NodeJS.ProcessEnv = process.env,
): Promise<string> {
  signal?.throwIfAborted();
  let accountShell: string | undefined;
  try {
    const proc = Bun.spawn(['getent', 'passwd', String(process.getuid!())], {
      stdout: 'pipe',
      stderr: 'ignore',
      signal,
    });
    const [output, code] = await Promise.all([new Response(proc.stdout).text(), proc.exited]);
    if (code === 0) accountShell = output.trimEnd().split(':')[6];
  } catch (error) {
    signal?.throwIfAborted();
  }
  for (const shell of [accountShell, env.SHELL, '/bin/bash', '/bin/sh']) {
    signal?.throwIfAborted();
    if (await usableShell(shell)) return shell!;
  }
  return '/bin/sh';
}
export interface TerminalSession {
  readonly cwd: string;
  readonly shell: string;
  readonly screen: TerminalScreen;
  /** Resolves only after PTY EOF and queued screen writes have drained. */
  readonly exited: Promise<number>;
  input(data: string): void;
  paste(text: string): void;
  resize(cols: number, rows: number): void;
  close(): Promise<void>;
}
export async function startTerminal(options: {
  cwd: string;
  cols?: number;
  rows?: number;
  theme?: TerminalTheme;
  env?: NodeJS.ProcessEnv;
  signal?: AbortSignal;
  onUpdate?: () => void;
  onClipboard?: (text: string) => void;
}): Promise<TerminalSession> {
  const env = { ...process.env, ...options.env },
    shell = await resolveLoginShell(options.signal, env);
  options.signal?.throwIfAborted();
  let pty: Bun.Terminal | undefined;
  const screen = new TerminalScreen({
    ...options,
    onResponse: (data) => {
      if (pty && !pty.closed) pty.write(data);
    },
  });
  const { promise: eof, resolve: resolveEof } = Promise.withResolvers<void>();
  let dirty = false;
  const extras = [
    `${env.HOME ?? homedir()}/.local/bin`,
    `${env.HOME ?? homedir()}/.bun/bin`,
    `${env.HOME ?? homedir()}/.cargo/bin`,
    '/usr/local/bin',
    '/usr/local/sbin',
    '/usr/bin',
    '/usr/sbin',
    '/bin',
    '/sbin',
  ];
  let proc: Bun.Subprocess;
  try {
    proc = Bun.spawn([shell, '-l'], {
      cwd: options.cwd,
      env: {
        ...env,
        PATH: [...new Set([...(env.PATH ?? '').split(':').filter(Boolean), ...extras])].join(':'),
        TERM: 'xterm-256color',
        COLORTERM: 'truecolor',
      },
      terminal: {
        cols: options.cols ?? 52,
        rows: options.rows ?? 36,
        name: 'xterm-256color',
        data: (_terminal, data) => {
          void screen.write(data).then(() => {
            dirty = true;
          });
        },
        exit: () => resolveEof(),
      },
    });
    pty = proc.terminal;
  } catch (error) {
    pty?.close();
    screen.dispose();
    throw error;
  }
  const timer = setInterval(() => {
    if (dirty) {
      dirty = false;
      options.onUpdate?.();
    }
  }, 24);
  let closing: Promise<void> | undefined;
  const close = (): Promise<void> => {
    if (!closing) {
      options.signal?.removeEventListener('abort', abort);
      clearInterval(timer);
      if (proc.exitCode === null) proc.kill('SIGKILL');
      if (!pty!.closed) pty!.close();
      resolveEof();
      closing = proc.exited.then(async () => {
        await screen.flush();
        screen.dispose();
      });
    }
    return closing;
  };
  const abort = (): void => {
    void close();
  };
  options.signal?.addEventListener('abort', abort, { once: true });
  if (options.signal?.aborted) abort();
  const exited = proc.exited.then(async (code) => {
    await eof;
    await screen.flush();
    clearInterval(timer);
    if (!pty!.closed) pty!.close();
    options.signal?.removeEventListener('abort', abort);
    options.onUpdate?.();
    return code;
  });
  return {
    cwd: options.cwd,
    shell,
    screen,
    exited,
    input(data) {
      if (!pty!.closed && proc.exitCode === null) pty!.write(data);
    },
    paste(text) {
      if (!pty!.closed && proc.exitCode === null) {
        // PTYs use carriage return for Enter, including multiline clipboard text.
        const data = text.replace(/\r?\n/g, '\r');
        pty!.write(
          screen.emulator.modes.bracketedPasteMode
            ? `\x1b[200~${data.replace(/\x1b/g, '')}\x1b[201~`
            : data,
        );
      }
    },
    resize(cols, rows) {
      if (!Number.isInteger(cols) || !Number.isInteger(rows) || cols < 1 || rows < 1)
        throw new RangeError('Terminal dimensions must be positive integers');
      screen.resize(cols, rows);
      if (!pty!.closed) pty!.resize(cols, rows);
      options.onUpdate?.();
    },
    close,
  };
}

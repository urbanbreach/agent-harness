import { expect, test } from 'bun:test';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { startTerminal, TerminalScreen, type TerminalSession } from '../../src/workspace/terminal';

test('screen decodes colors, styled glyphs, palette overrides, title and cursor controls', async () => {
  const responses: string[] = [];
  const screen = new TerminalScreen({
    cols: 40,
    rows: 4,
    onResponse: (data) => responses.push(data),
  });
  try {
    await screen.write(
      '\x1b]2;Build shell\x07\x1b[31mA\x1b[38;5;196mB\x1b[38;2;1;2;3mC\x1b[0;1;3;4;9mD\x1b[0;7mE\x1b[0;2mF\x1b[0;8mG\x1b[0m界e\u0301',
    );
    const snapshot = screen.snapshot(),
      runs = snapshot.rows[0]!.runs;
    expect(snapshot.title).toBe('Build shell');
    expect(runs.find((run) => run.text === 'A')?.foreground).toBe('#CC6666');
    expect(runs.find((run) => run.text === 'B')?.foreground).toBe('#FF0000');
    expect(runs.find((run) => run.text === 'C')?.foreground).toBe('#010203');
    expect(runs.find((run) => run.text === 'D')).toMatchObject({
      bold: true,
      italic: true,
      underline: true,
      strike: true,
    });
    expect(runs.find((run) => run.text === 'E')).toMatchObject({
      foreground: '#151515',
      background: '#E2E2E2',
      inverse: true,
    });
    expect(runs.find((run) => run.text === 'F')).toMatchObject({ faint: true });
    expect(runs.find((run) => run.faint)?.foreground).not.toBe('#E2E2E2');
    expect(runs.find((run) => run.hidden)?.text).toBe(' ');
    const unicode = runs.find((run) => run.text.startsWith('界'))!;
    expect(unicode.widths.slice(0, 2)).toEqual([2, 1]);
    expect(unicode.text).toContain('界e\u0301');
    expect(snapshot.cursor).toMatchObject({ column: 10, row: 0, visible: true, shape: 'block' });
    await screen.write('\x1b[?25l\x1b[4 q\x1b[3;7H');
    expect(screen.snapshot().cursor).toMatchObject({
      visible: false,
      row: 2,
      column: 6,
      shape: 'underline',
      blink: false,
    });
    await screen.write('\x1b[?25h\x1b]4;1;#123456\x07\x1b]4;1;?\x07');
    expect(responses).toContain('\x1b]4;1;rgb:1212/3434/5656\x1b\\');
    screen.setTheme('light');
    expect(screen.color(1)).toBe('#123456');
    expect(screen.color(4)).toBe('#4271AE');
    await screen.write('\x1b]104;1\x07\x1b]10;?\x07\x1b]2;\x07');
    expect(screen.color(1)).toBe('#C82829');
    expect(responses).toContain('\x1b]10;rgb:2424/2424/2424\x1b\\');
    expect(screen.snapshot().title).toBe('Terminal');
  } finally {
    screen.dispose();
  }
});

test('selection joins wrapped rows, preserves combining glyphs, blanks hidden text and includes history', async () => {
  const screen = new TerminalScreen({ cols: 5, rows: 2 });
  try {
    await screen.write('abcdeFG\r\n界e\u0301\x1b[8mX\x1b[0m\r\nlast');
    expect(screen.snapshot().scrollback).toBeGreaterThan(0);
    expect(screen.selectionText({ row: 0, column: 0 }, { row: 3, column: 4 })).toBe(
      'abcdeFG\n界e\u0301\nlast',
    );
    screen.scrollTo(0);
    expect(screen.snapshot().viewport).toBe(0);
    expect(screen.snapshot().cursor.visible).toBe(false);
  } finally {
    screen.dispose();
  }
});

async function eventually(predicate: () => boolean | Promise<boolean>): Promise<void> {
  const deadline = Date.now() + 7_000;
  while (Date.now() < deadline) {
    if (await predicate()) return;
    await Bun.sleep(10);
  }
  throw new Error('Terminal output did not arrive before the deadline');
}

test('login PTY runs in workspace with color environment, accepts input, resizes and drains exit output', async () => {
  const cwd = await mkdtemp(join(tmpdir(), 'harness-terminal-'));
  const controller = new AbortController();
  let session: TerminalSession | undefined;
  try {
    session = await startTerminal({
      cwd,
      cols: 60,
      rows: 12,
      env: { HOME: cwd, ZDOTDIR: cwd, XDG_CONFIG_HOME: cwd },
      signal: controller.signal,
    });
    const terminal = session;
    terminal.input(
      "stty -echo; printf '\\033[2J\\033[H'; printf 'PTY_READY\\n'; pwd; printenv TERM COLORTERM; stty size\n",
    );
    const output = () =>
      terminal.screen.selectionText(
        { row: 0, column: 0 },
        { row: terminal.screen.snapshot().totalRows - 1, column: 60 },
      );
    await eventually(async () => {
      await terminal.screen.flush();
      return /(?:^|\n)PTY_READY\n/.test(output()) && /12\s+60/.test(output());
    });
    expect(output()).toContain(cwd);
    expect(output()).toContain('xterm-256color\ntruecolor');
    terminal.resize(90, 20);
    terminal.input("stty size; printf 'FINAL_OUTPUT\\n'; exit 7\n");
    expect(await terminal.exited).toBe(7);
    expect(terminal.screen.snapshot().cols).toBe(90);
    expect(output()).toMatch(/20\s+90/);
    expect(output()).toContain('FINAL_OUTPUT');
  } finally {
    controller.abort();
    await session?.close();
    await rm(cwd, { recursive: true, force: true });
  }
}, 15_000);

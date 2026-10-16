import { expect, test } from 'bun:test';
import { terminalKey } from '../../src/app/panel/terminal';

test('terminal keys preserve interrupt, application cursor and xterm modifier semantics', () => {
  const cases: {
    key: string;
    keyChar?: string;
    ctrl?: boolean;
    shift?: boolean;
    alt?: boolean;
    cmd?: boolean;
    application?: boolean;
    output: string | null;
  }[] = [
    { key: 'c', ctrl: true, output: '\x03' },
    { key: 'up', application: true, output: '\x1bOA' },
    { key: 'left', application: true, ctrl: true, shift: true, output: '\x1b[1;6D' },
    { key: 'tab', shift: true, output: '\x1b[Z' },
    { key: 'enter', output: '\r' },
    { key: 'escape', output: '\x1b' },
    { key: 'backspace', output: '\x7f' },
    { key: 'delete', alt: true, output: '\x1b[3;3~' },
    { key: 'pagedown', output: '\x1b[6~' },
    { key: 'f1', output: '\x1bOP' },
    { key: 'f12', ctrl: true, output: '\x1b[24;5~' },
    { key: 'x', keyChar: 'X', alt: true, shift: true, output: '\x1bX' },
    { key: 'space', ctrl: true, output: '\0' },
    { key: 'a', cmd: true, output: null },
    { key: 'shift', output: null },
  ];
  for (const entry of cases) {
    expect(
      terminalKey(
        {
          key: entry.key,
          keyChar: entry.keyChar,
          modifiers: {
            ctrl: entry.ctrl ?? false,
            shift: entry.shift ?? false,
            alt: entry.alt ?? false,
            cmd: entry.cmd ?? false,
          },
        },
        entry.application ?? false,
      ),
    ).toBe(entry.output);
  }
});

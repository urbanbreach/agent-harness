import { access, constants, stat } from 'node:fs/promises';
import { dirname, resolve, join } from 'node:path';

async function executable(name: string): Promise<string | undefined> {
  for (const directory of (process.env.PATH ?? '/usr/bin:/bin').split(':')) {
    const path = join(directory || '.', name);
    try {
      await access(path, constants.X_OK);
      return path;
    } catch {
      /* Try the next executable directory. */
    }
  }
  return undefined;
}
async function run(
  command: string[],
  signal?: AbortSignal,
  input?: string,
): Promise<{ code: number; stdout: string; stderr: string }> {
  signal?.throwIfAborted();
  const proc = Bun.spawn(command, {
    stdin: input === undefined ? 'ignore' : new Blob([input]),
    stdout: 'pipe',
    stderr: 'pipe',
    signal,
  });
  const [code, stdout, stderr] = await Promise.all([
    proc.exited,
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
  ]);
  signal?.throwIfAborted();
  return { code, stdout, stderr };
}
async function checked(command: string[], signal?: AbortSignal): Promise<void> {
  const result = await run(command, signal);
  if (result.code !== 0)
    throw new Error(`${command[0]} exited ${result.code}: ${result.stderr.trim()}`);
}

/** Action delivery is available only when notify-send supports both --action and --wait. */
export async function notify(options: {
  title: string;
  body?: string;
  tag?: string;
  onActivate?: () => void | Promise<void>;
  signal?: AbortSignal;
}): Promise<{ activationSupported: boolean; activated: boolean }> {
  options.signal?.throwIfAborted();
  const command = await executable('notify-send');
  if (!command) throw new Error('Desktop notifications require notify-send');
  let activationSupported = false;
  if (options.onActivate) {
    const help = await run([command, '--help'], options.signal);
    activationSupported =
      help.code === 0 && help.stdout.includes('--action') && help.stdout.includes('--wait');
  }
  const args = [command, '--app-name=Harness'];
  if (options.tag) args.push('--hint', `string:x-canonical-private-synchronous:${options.tag}`);
  if (activationSupported) args.push('--action=default=Open', '--wait');
  args.push('--', options.title, options.body ?? '');
  const result = await run(args, options.signal);
  if (result.code !== 0)
    throw new Error(`notify-send exited ${result.code}: ${result.stderr.trim()}`);
  const activated = activationSupported && result.stdout.trim() === 'default';
  if (activated) await options.onActivate?.();
  return { activationSupported, activated };
}

export async function openUrl(url: string, signal?: AbortSignal): Promise<void> {
  const parsed = new URL(url);
  if (['javascript:', 'data:', 'vbscript:'].includes(parsed.protocol))
    throw new Error(`Unsafe external URL scheme: ${parsed.protocol}`);
  await checked(['xdg-open', parsed.href], signal);
}

/** File managers are opened at the parent directory for files, and at the path for directories. */
export async function revealPath(path: string, signal?: AbortSignal): Promise<void> {
  signal?.throwIfAborted();
  const absolute = resolve(path),
    info = await stat(absolute);
  signal?.throwIfAborted();
  await checked(['xdg-open', info.isDirectory() ? absolute : dirname(absolute)], signal);
}

async function clipboard(
  operation: 'read' | 'write',
  text: string | undefined,
  signal?: AbortSignal,
): Promise<string> {
  const candidates =
    operation === 'read'
      ? [
          ['wl-paste', '--no-newline'],
          ['xclip', '-selection', 'clipboard', '-out'],
          ['xsel', '--clipboard', '--output'],
        ]
      : [
          ['wl-copy'],
          ['xclip', '-selection', 'clipboard', '-in'],
          ['xsel', '--clipboard', '--input'],
        ];
  const errors: string[] = [];
  for (const [name, ...args] of candidates) {
    signal?.throwIfAborted();
    const command = await executable(name!);
    if (!command) continue;
    if (operation === 'write') {
      // Clipboard owners may fork and retain pipes after the launcher exits.
      const proc = Bun.spawn([command, ...args], {
        stdin: new Blob([text ?? '']),
        stdout: 'ignore',
        stderr: 'ignore',
        signal,
      });
      const code = await proc.exited;
      signal?.throwIfAborted();
      if (code === 0) return '';
      errors.push(`${name}: exited ${code}`);
      continue;
    }
    const result = await run([command, ...args], signal, text);
    if (result.code === 0) return result.stdout;
    errors.push(`${name}: ${result.stderr.trim()}`);
  }
  throw new Error(
    errors.length
      ? `Clipboard ${operation} failed: ${errors.join('; ')}`
      : 'Clipboard requires wl-clipboard, xclip, or xsel',
  );
}
export async function readClipboard(signal?: AbortSignal): Promise<string> {
  return clipboard('read', undefined, signal);
}
export async function writeClipboard(text: string, signal?: AbortSignal): Promise<void> {
  await clipboard('write', text, signal);
}

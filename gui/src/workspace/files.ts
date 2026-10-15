import { readdir, realpath, readFile, writeFile, lstat } from 'node:fs/promises';
import { watch } from 'node:fs';
import { isAbsolute, relative, resolve, sep, dirname } from 'node:path';

export interface TreeEntry {
  path: string;
  absolutePath: string;
  name: string;
  directory: boolean;
  expanded: boolean;
  depth: number;
}

/** The direct browser intentionally includes ignored files and other dotfiles. */
export async function listTree(
  root: string,
  expanded: ReadonlySet<string> = new Set(),
  signal?: AbortSignal,
): Promise<TreeEntry[]> {
  const canonical = await realpath(root);
  const result: TreeEntry[] = [];
  async function visit(directory: string, depth: number): Promise<void> {
    signal?.throwIfAborted();
    const entries = await readdir(directory, { withFileTypes: true }).catch(() => []);
    entries.sort(
      (a, b) => Number(b.isDirectory()) - Number(a.isDirectory()) || compareNames(a.name, b.name),
    );
    for (const entry of entries) {
      signal?.throwIfAborted();
      if (entry.name === '.git') continue;
      const absolutePath = resolve(directory, entry.name);
      const path = relative(canonical, absolutePath);
      const directoryEntry = entry.isDirectory();
      const isExpanded = directoryEntry && (expanded.has(path) || expanded.has(absolutePath));
      result.push({
        path,
        absolutePath,
        name: entry.name,
        directory: directoryEntry,
        expanded: isExpanded,
        depth,
      });
      if (isExpanded) await visit(absolutePath, depth + 1);
    }
  }
  await visit(canonical, 0);
  return result;
}

function compareNames(a: string, b: string): number {
  const lowerA = a.toLowerCase(),
    lowerB = b.toLowerCase();
  return (lowerA < lowerB ? -1 : lowerA > lowerB ? 1 : 0) || (a < b ? -1 : a > b ? 1 : 0);
}

function contained(root: string, path: string): boolean {
  const rel = relative(root, path);
  return rel === '' || (!isAbsolute(rel) && rel !== '..' && !rel.startsWith(`..${sep}`));
}

/** Resolve both existing targets and new-file parents so symlinks cannot escape the root. */
async function safeFile(root: string, path: string): Promise<string> {
  if (!path || path.includes('\0') || isAbsolute(path) || path.split(/[\\/]/).includes('..')) {
    throw new Error('expected a nonempty relative workspace path without parent traversal');
  }
  const canonical = await realpath(root);
  const target = resolve(canonical, path);
  if (!contained(canonical, target) || target === canonical)
    throw new Error('path is outside the workspace');
  let actual: string;
  try {
    actual = await realpath(target);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
    const link = await lstat(target).catch((error: NodeJS.ErrnoException) => {
      if (error.code !== 'ENOENT') throw error;
      return null;
    });
    if (link?.isSymbolicLink()) throw new Error('unable to resolve workspace symlink');
    actual = resolve(await realpath(dirname(target)), target.slice(dirname(target).length + 1));
  }
  if (!contained(canonical, actual)) throw new Error('path is outside the workspace');
  return actual;
}

export async function readText(root: string, path: string, signal?: AbortSignal): Promise<string> {
  signal?.throwIfAborted();
  const bytes = await readFile(await safeFile(root, path), { signal });
  return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
}

export async function writeText(
  root: string,
  path: string,
  text: string,
  signal?: AbortSignal,
): Promise<void> {
  signal?.throwIfAborted();
  await writeFile(await safeFile(root, path), text, { encoding: 'utf8', signal });
}

export interface FileCandidate {
  path: string;
  directory: boolean;
}
export interface FileIndex {
  root: string;
  entries: FileCandidate[];
  truncated: boolean;
}
const excludedDirectories = new Set([
  'node_modules',
  'target',
  'dist',
  'build',
  'out',
  'vendor',
  '__pycache__',
]);

/** Git owns ignore semantics, including global excludes and tracked ignored files. */
export async function buildFileIndex(
  root: string,
  options: { cap?: number; signal?: AbortSignal } = {},
): Promise<FileIndex> {
  const canonical = await realpath(root);
  const cap = Math.max(0, Math.min(50_000, Math.floor(options.cap ?? 50_000)));
  const signal = options.signal;
  signal?.throwIfAborted();
  const probe = Bun.spawn(['git', '-C', canonical, 'rev-parse', '--show-toplevel'], {
    stdout: 'pipe',
    stderr: 'ignore',
  });
  const repoRoot = (await new Response(probe.stdout).text()).trim();
  const inGit = (await probe.exited) === 0;
  const entries: FileCandidate[] = [];
  const seen = new Set<string>();
  let truncated = false;
  const add = (path: string, directory: boolean): boolean => {
    if (seen.has(path)) return true;
    if (entries.length >= cap) {
      truncated = true;
      return false;
    }
    seen.add(path);
    entries.push({ path, directory });
    return true;
  };
  if (inGit) {
    const child = Bun.spawn(
      ['git', '-C', repoRoot, 'ls-files', '--cached', '--others', '--exclude-standard', '-z'],
      { stdout: 'pipe', stderr: 'pipe' },
    );
    const abort = () => child.kill();
    signal?.addEventListener('abort', abort, { once: true });
    const stderr = new Response(child.stderr).text();
    const reader = child.stdout.getReader();
    const decoder = new TextDecoder();
    let pending = '',
      stopped = false;
    try {
      while (!stopped) {
        signal?.throwIfAborted();
        const { value, done } = await reader.read();
        pending += decoder.decode(value, { stream: !done });
        let end: number;
        while ((end = pending.indexOf('\0')) >= 0) {
          const repoPath = pending.slice(0, end);
          pending = pending.slice(end + 1);
          const absolute = resolve(repoRoot, repoPath);
          if (!contained(canonical, absolute)) continue;
          const path = relative(canonical, absolute);
          const parts = path.split(sep);
          if (parts.length > 8 || parts.includes('.git')) continue;
          for (let i = 1; i < parts.length; i++) {
            if (!add(parts.slice(0, i).join(sep), true)) {
              stopped = true;
              break;
            }
          }
          if (!stopped && !add(path, false)) stopped = true;
          if (stopped) break;
        }
        if (done) break;
      }
      if (stopped) child.kill();
      const code = await child.exited;
      signal?.throwIfAborted();
      const error = await stderr;
      if (code !== 0 && !stopped)
        throw new Error(error.trim() || 'could not index workspace files');
    } finally {
      signal?.removeEventListener('abort', abort);
      await reader.cancel().catch(() => {});
      child.kill();
    }
  } else {
    async function visit(directory: string, depth: number): Promise<void> {
      signal?.throwIfAborted();
      const children = await readdir(directory, { withFileTypes: true }).catch(() => []);
      children.sort((a, b) => compareNames(a.name, b.name));
      for (const child of children) {
        if (truncated) return;
        signal?.throwIfAborted();
        if (
          child.name.startsWith('.') ||
          (child.isDirectory() && excludedDirectories.has(child.name))
        )
          continue;
        const absolute = resolve(directory, child.name);
        if (!child.isDirectory() && !child.isFile()) continue;
        if (!add(relative(canonical, absolute), child.isDirectory())) return;
        if (child.isDirectory() && depth < 7) await visit(absolute, depth + 1);
      }
    }
    await visit(canonical, 0);
  }
  entries.sort((a, b) => compareNames(a.path, b.path));
  return { root: canonical, entries, truncated };
}

/** Filename-oriented subsequence matching; exact and contiguous matches win. */
export function searchFiles(index: FileIndex, query: string, limit = 64): FileCandidate[] {
  const needle = query.trim().toLowerCase();
  const matches: { entry: FileCandidate; score: number }[] = [];
  for (const entry of index.entries) {
    const path = entry.path.toLowerCase();
    let cursor = 0,
      first = -1,
      last = -1;
    for (const char of needle) {
      const next = path.indexOf(char, cursor);
      if (next < 0) {
        cursor = -1;
        break;
      }
      if (first < 0) first = next;
      last = next;
      cursor = next + 1;
    }
    if (cursor < 0) continue;
    const basename = path.slice(path.lastIndexOf(sep) + 1);
    const score =
      (basename === needle ? -10_000 : basename.startsWith(needle) ? -5_000 : 0) +
      (needle && path.includes(needle) ? -1_000 : 0) +
      (last - first) * 4 +
      path.length;
    matches.push({ entry, score });
  }
  matches.sort((a, b) => a.score - b.score || compareNames(a.entry.path, b.entry.path));
  return matches.slice(0, Math.max(0, Math.min(64, Math.floor(limit)))).map(({ entry }) => entry);
}

export function watchWorkspace(
  root: string,
  onRefresh: () => void,
  options: { debounceMs?: number; signal?: AbortSignal; onError?: (error: Error) => void } = {},
): () => void {
  options.signal?.throwIfAborted();
  let timer: NodeJS.Timeout | undefined;
  let closed = false;
  const watcher = watch(root, { recursive: true }, (_event, filename) => {
    if (filename?.toString().split(/[\\/]/).includes('.git')) return;
    clearTimeout(timer);
    timer = setTimeout(() => {
      if (!closed) onRefresh();
    }, options.debounceMs ?? 100);
  });
  watcher.on('error', (error) => options.onError?.(error));
  const close = () => {
    if (closed) return;
    closed = true;
    clearTimeout(timer);
    watcher.close();
    options.signal?.removeEventListener('abort', close);
  };
  options.signal?.addEventListener('abort', close, { once: true });
  return close;
}

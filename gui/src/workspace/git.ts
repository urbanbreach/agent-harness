import { copyFile, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

export interface GitOptions {
  signal?: AbortSignal;
  env?: Record<string, string | undefined>;
  input?: string | Uint8Array;
  allowFailure?: boolean;
}
export interface GitResult {
  stdout: string;
  stderr: string;
  exitCode: number;
}
export class GitError extends Error {
  constructor(readonly result: GitResult) {
    super(
      result.stderr.trim() || result.stdout.trim() || `git exited with status ${result.exitCode}`,
    );
    this.name = 'GitError';
  }
}

export async function gitCommand(
  cwd: string,
  args: string[],
  options: GitOptions = {},
): Promise<GitResult> {
  options.signal?.throwIfAborted();
  const process = Bun.spawn(['git', '--no-pager', ...args], {
    cwd,
    env: { ...Bun.env, GIT_TERMINAL_PROMPT: '0', GIT_OPTIONAL_LOCKS: '0', ...options.env },
    stdin:
      options.input === undefined
        ? 'ignore'
        : new Blob([
            typeof options.input === 'string' ? options.input : new Uint8Array(options.input),
          ]),
    stdout: 'pipe',
    stderr: 'pipe',
  });
  const abort = () => process.kill();
  options.signal?.addEventListener('abort', abort, { once: true });
  try {
    const [stdout, stderr, exitCode] = await Promise.all([
      new Response(process.stdout).text(),
      new Response(process.stderr).text(),
      process.exited,
    ]);
    options.signal?.throwIfAborted();
    const result = { stdout, stderr, exitCode };
    if (exitCode !== 0 && !options.allowFailure) throw new GitError(result);
    return result;
  } finally {
    options.signal?.removeEventListener('abort', abort);
  }
}

async function repositoryRoot(cwd: string, signal?: AbortSignal): Promise<string | null> {
  const result = await gitCommand(cwd, ['rev-parse', '--show-toplevel'], {
    signal,
    allowFailure: true,
  });
  if (result.exitCode === 0) return result.stdout.trim();
  if (result.stderr.includes('not a git repository')) return null;
  throw new GitError(result);
}
async function revision(cwd: string, ref: string, signal?: AbortSignal): Promise<string | null> {
  const result = await gitCommand(cwd, ['rev-parse', '--verify', '--quiet', `${ref}^{tree}`], {
    signal,
    allowFailure: true,
  });
  return result.exitCode === 0 ? result.stdout.trim() : null;
}
async function emptyTree(cwd: string, signal?: AbortSignal): Promise<string> {
  return (
    await gitCommand(cwd, ['hash-object', '-w', '-t', 'tree', '--stdin'], { input: '', signal })
  ).stdout.trim();
}
async function headTree(cwd: string, signal?: AbortSignal): Promise<string> {
  return (await revision(cwd, 'HEAD', signal)) ?? (await emptyTree(cwd, signal));
}

// A private index captures immutable trees without writing the user's index or HEAD.
async function captureTrees(
  cwd: string,
  current: boolean,
  signal?: AbortSignal,
): Promise<{ index: string; current: string }> {
  const directory = await mkdtemp(join(tmpdir(), 'harness-git-'));
  try {
    const indexPath = (
      await gitCommand(cwd, ['rev-parse', '--path-format=absolute', '--git-path', 'index'], {
        signal,
      })
    ).stdout.trim();
    const privateIndex = join(directory, 'index');
    const env = { GIT_INDEX_FILE: privateIndex };
    try {
      await copyFile(indexPath, privateIndex);
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
      await gitCommand(cwd, ['read-tree', '--empty'], { env, signal });
    }
    const index = (await gitCommand(cwd, ['write-tree'], { env, signal })).stdout.trim();
    if (!current) return { index, current: index };
    await gitCommand(cwd, ['add', '--all', '--', '.'], { env, signal });
    const tree = (await gitCommand(cwd, ['write-tree'], { env, signal })).stdout.trim();
    return { index, current: tree };
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

export interface DiffFile {
  path: string;
  additions: number;
  deletions: number;
  binary: boolean;
}
function parseSummary(summary: string): DiffFile[] {
  const fields = summary.split('\0');
  const files: DiffFile[] = [];
  for (let i = 0; i < fields.length; i++) {
    const field = fields[i];
    if (!field) continue;
    const first = field.indexOf('\t');
    const second = field.indexOf('\t', first + 1);
    const added = field.slice(0, first);
    const removed = field.slice(first + 1, second);
    let path = field.slice(second + 1);
    if (!path) {
      i++;
      path = fields[++i] ?? '';
    }
    files.push({
      path,
      additions: Number(added) || 0,
      deletions: Number(removed) || 0,
      binary: added === '-',
    });
  }
  return files;
}
function totals(files: DiffFile[]): { additions: number; deletions: number } {
  return files.reduce(
    (sum, file) => ({
      additions: sum.additions + file.additions,
      deletions: sum.deletions + file.deletions,
    }),
    { additions: 0, deletions: 0 },
  );
}
async function summaryBetween(cwd: string, base: string, target: string, signal?: AbortSignal) {
  const summary = (
    await gitCommand(
      cwd,
      ['diff', '--no-ext-diff', '--no-renames', '--numstat', '-z', base, target, '--'],
      { signal },
    )
  ).stdout;
  const files = parseSummary(summary);
  return { summary, files, ...totals(files) };
}
async function defaultBranch(cwd: string, signal?: AbortSignal): Promise<string | null> {
  const remote = await gitCommand(
    cwd,
    ['symbolic-ref', '--quiet', '--short', 'refs/remotes/origin/HEAD'],
    { signal, allowFailure: true },
  );
  if (remote.exitCode === 0) return remote.stdout.trim().replace(/^origin\//, '');
  for (const name of ['main', 'master'])
    if (await revision(cwd, `refs/heads/${name}`, signal)) return name;
  return null;
}
export interface BranchSnapshot {
  root: string;
  current: string;
  detached: boolean;
  defaultBranch: string | null;
  branches: { name: string; checkedOutElsewhere: boolean }[];
  additions: number;
  deletions: number;
}
export async function inspectBranches(
  cwd: string,
  signal?: AbortSignal,
): Promise<BranchSnapshot | null> {
  const root = await repositoryRoot(cwd, signal);
  if (!root) return null;
  const symbolic = await gitCommand(root, ['symbolic-ref', '--quiet', '--short', 'HEAD'], {
    signal,
    allowFailure: true,
  });
  const detached = symbolic.exitCode !== 0;
  const current = detached
    ? (await gitCommand(root, ['rev-parse', '--short', 'HEAD'], { signal })).stdout.trim()
    : symbolic.stdout.trim();
  const refs = await gitCommand(
    root,
    ['for-each-ref', '--format=%(refname:short)%00%(worktreepath)', 'refs/heads/'],
    { signal },
  );
  const branches = refs.stdout
    .trimEnd()
    .split('\n')
    .filter(Boolean)
    .map((line) => {
      const [name = '', worktree = ''] = line.split('\0');
      return { name, checkedOutElsewhere: Boolean(worktree) && name !== current };
    });
  const trees = await captureTrees(root, true, signal);
  const counts = await summaryBetween(root, await headTree(root, signal), trees.current, signal);
  return {
    root,
    current,
    detached,
    defaultBranch: await defaultBranch(root, signal),
    branches,
    additions: counts.additions,
    deletions: counts.deletions,
  };
}
export async function switchBranch(
  cwd: string,
  name: string,
  create = false,
  signal?: AbortSignal,
): Promise<BranchSnapshot> {
  name = name.trim();
  if (!name) throw new Error('enter a branch name');
  await gitCommand(cwd, ['check-ref-format', '--branch', name], { signal });
  await gitCommand(cwd, ['switch', ...(create ? ['-c'] : []), name], { signal });
  const snapshot = await inspectBranches(cwd, signal);
  if (!snapshot) throw new Error('the workspace is not a Git repository');
  return snapshot;
}

export interface CommitState {
  branch: string;
  detached: boolean;
  additions: number;
  deletions: number;
  stagedAdditions: number;
  stagedDeletions: number;
  hasStaged: boolean;
  hasUnstaged: boolean;
  canPush: boolean;
}
async function pushRemote(
  cwd: string,
  branch: string,
  signal?: AbortSignal,
): Promise<string | null> {
  const configured = await gitCommand(cwd, ['config', '--get', `branch.${branch}.remote`], {
    signal,
    allowFailure: true,
  });
  if (configured.exitCode === 0 && configured.stdout.trim()) return configured.stdout.trim();
  const remotes = (await gitCommand(cwd, ['remote'], { signal })).stdout
    .trim()
    .split('\n')
    .filter(Boolean);
  return remotes.includes('origin') ? 'origin' : (remotes[0] ?? null);
}
export async function commitState(cwd: string, signal?: AbortSignal): Promise<CommitState> {
  const root = await repositoryRoot(cwd, signal);
  if (!root) throw new Error('the workspace is not a Git repository');
  const symbolic = await gitCommand(root, ['symbolic-ref', '--quiet', '--short', 'HEAD'], {
    signal,
    allowFailure: true,
  });
  const detached = symbolic.exitCode !== 0;
  const branch = detached ? 'HEAD' : symbolic.stdout.trim();
  const trees = await captureTrees(root, true, signal);
  const head = await headTree(root, signal);
  const staged = await summaryBetween(root, head, trees.index, signal);
  // Commit dialog totals describe tracked changes; untracked presence still enables staging.
  const tracked = (
    await gitCommand(root, ['diff', '--no-ext-diff', '--numstat', '-z', head, '--'], { signal })
  ).stdout;
  const trackedTotals = totals(parseSummary(tracked));
  return {
    branch,
    detached,
    ...trackedTotals,
    stagedAdditions: staged.additions,
    stagedDeletions: staged.deletions,
    hasStaged: staged.files.length > 0,
    hasUnstaged: trees.index !== trees.current,
    canPush:
      !detached &&
      (await revision(root, 'HEAD', signal)) !== null &&
      (await pushRemote(root, branch, signal)) !== null,
  };
}
export function normalizeCommitSubject(message: string): string {
  const lines = message
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line && !/^```/.test(line));
  return (lines.at(-1) ?? '')
    .replace(/^(?:(?:commit(?: message)?|subject)\s*:\s*|[-*]\s+)/i, '')
    .replace(/^[\s'"`]+|[\s'"`]+$/g, '')
    .replace(/\.+$/, '')
    .trim()
    .slice(0, 200);
}
export async function push(cwd: string, signal?: AbortSignal): Promise<void> {
  const branchResult = await gitCommand(cwd, ['symbolic-ref', '--quiet', '--short', 'HEAD'], {
    signal,
    allowFailure: true,
  });
  if (branchResult.exitCode !== 0) throw new Error('cannot push a detached HEAD');
  const branch = branchResult.stdout.trim();
  const remote = await pushRemote(cwd, branch, signal);
  if (!remote) throw new Error('no Git remote is configured for this branch');
  const upstream = await gitCommand(cwd, ['rev-parse', '--verify', '--quiet', '@{upstream}'], {
    signal,
    allowFailure: true,
  });
  await gitCommand(
    cwd,
    upstream.exitCode === 0 ? ['push'] : ['push', '--set-upstream', remote, branch],
    { signal },
  );
}
export interface CommitResult {
  commit: string;
  subject: string;
  pushed: boolean;
  pushError?: string;
}
export async function commit(
  cwd: string,
  options: { message: string; stageAll?: boolean; push?: boolean; signal?: AbortSignal },
): Promise<CommitResult> {
  const subject = normalizeCommitSubject(options.message);
  if (!subject) throw new Error('enter a commit message');
  const { signal } = options;
  if (options.stageAll) await gitCommand(cwd, ['add', '--all', '--', ':/'], { signal });
  const state = await commitState(cwd, signal);
  if (!state.hasStaged) throw new Error('there are no staged changes to commit');
  await gitCommand(cwd, ['commit', '-m', subject], { signal });
  const hash = (await gitCommand(cwd, ['rev-parse', 'HEAD'], { signal })).stdout.trim();
  if (options.push) {
    try {
      await push(cwd, signal);
    } catch (error) {
      return {
        commit: hash,
        subject,
        pushed: false,
        pushError: error instanceof Error ? error.message : String(error),
      };
    }
  }
  return { commit: hash, subject, pushed: Boolean(options.push) };
}

export type ReviewSource =
  | 'uncommitted'
  | 'unstaged'
  | 'staged'
  | 'committed'
  | 'branch'
  | { kind: 'lastTurn'; session: string; turn: number };
export interface ReviewSnapshot {
  source: ReviewSource;
  root: string;
  baseRevision: string;
  targetRevision: string;
  summary: string;
  files: DiffFile[];
  additions: number;
  deletions: number;
  patch: string;
  truncated: boolean;
  completeContext: boolean;
}
async function mergeBase(cwd: string, signal?: AbortSignal): Promise<string> {
  const head = await headTree(cwd, signal);
  const current = (
    await gitCommand(cwd, ['symbolic-ref', '--quiet', '--short', 'HEAD'], {
      signal,
      allowFailure: true,
    })
  ).stdout.trim();
  const preferred = await defaultBranch(cwd, signal);
  for (const name of [...new Set([preferred, 'main', 'master'])]) {
    if (!name || name === current) continue;
    for (const ref of [`refs/heads/${name}`, `refs/remotes/origin/${name}`]) {
      const result = await gitCommand(cwd, ['merge-base', 'HEAD', ref], {
        signal,
        allowFailure: true,
      });
      if (result.exitCode === 0) return result.stdout.trim();
    }
  }
  return head;
}
async function patchSnapshot(
  snapshot: Omit<ReviewSnapshot, 'patch' | 'truncated' | 'completeContext'>,
  context: number,
  signal?: AbortSignal,
): Promise<ReviewSnapshot> {
  const patch = (
    await gitCommand(
      snapshot.root,
      [
        'diff',
        '--no-ext-diff',
        '--no-textconv',
        '--no-renames',
        `--unified=${context}`,
        snapshot.baseRevision,
        snapshot.targetRevision,
        '--',
      ],
      { signal },
    )
  ).stdout;
  let end = 0;
  let lines = 0;
  while (lines < 50_000 && end < patch.length) {
    const next = patch.indexOf('\n', end);
    end = next < 0 ? patch.length : next + 1;
    lines++;
  }
  const truncated = end < patch.length;
  return {
    ...snapshot,
    patch: truncated ? patch.slice(0, end) : patch,
    truncated,
    completeContext: context === 2_147_483_647 && !truncated,
  };
}
export async function collectReviewDiff(
  cwd: string,
  source: ReviewSource,
  signal?: AbortSignal,
): Promise<ReviewSnapshot> {
  const root = await repositoryRoot(cwd, signal);
  if (!root) throw new Error('the workspace is not a Git repository');
  let baseRevision: string;
  let targetRevision: string;
  if (typeof source === 'object') {
    if (!Number.isSafeInteger(source.turn) || source.turn < 0)
      throw new Error('invalid turn number');
    if (source.turn === 0)
      throw new Error('the first checkpoint is a baseline, not a completed turn');
    const prefix = `refs/harness-gui/${source.session}/${source.turn}`;
    await gitCommand(root, ['check-ref-format', `${prefix}-start`], { signal });
    const start = await revision(root, `${prefix}-start`, signal);
    if (!start) throw new Error("the turn's starting checkpoint is unavailable");
    const end = await revision(root, `${prefix}-end`, signal);
    if (!end) throw new Error("the turn's ending checkpoint is unavailable");
    baseRevision = start;
    targetRevision = end;
  } else {
    const head = await headTree(root, signal);
    baseRevision =
      source === 'committed' || source === 'branch' ? await mergeBase(root, signal) : head;
    targetRevision = head;
    if (source !== 'committed') {
      const trees = await captureTrees(root, source !== 'staged', signal);
      if (source === 'unstaged') baseRevision = trees.index;
      targetRevision = source === 'staged' ? trees.index : trees.current;
    }
  }
  return patchSnapshot(
    {
      source,
      root,
      baseRevision,
      targetRevision,
      ...(await summaryBetween(root, baseRevision, targetRevision, signal)),
    },
    3,
    signal,
  );
}
export async function expandReviewContext(
  snapshot: ReviewSnapshot,
  context = 100,
  signal?: AbortSignal,
): Promise<ReviewSnapshot> {
  if (!Number.isSafeInteger(context) || context < 0)
    throw new Error('context must be a nonnegative integer');
  return patchSnapshot(snapshot, Math.min(context, 2_147_483_647), signal);
}

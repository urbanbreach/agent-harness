import { copyFile, lstat, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { capturePaths, gitCommand, repositoryRoot } from './git';

const namespace = 'refs/harness-gui/';
const requiresGit = 'checkpoints require a Git repository';

export interface CheckpointFile {
  path: string;
  status: 'added' | 'modified' | 'deleted';
  additions?: number;
  deletions?: number;
}

export interface Checkpoint {
  turn: number;
  ref: string;
  state: 'ready' | 'unavailable';
  files: CheckpointFile[];
  additions: number;
  deletions: number;
  createdAt: number;
}

function validateSession(session: string): void {
  if (!session || /[^A-Za-z0-9_-]/.test(session)) throw new Error('invalid checkpoint session');
}

function validateTurn(turn: number): void {
  if (!Number.isSafeInteger(turn) || turn < 0) throw new Error('invalid checkpoint turn');
}

function validateRef(ref: string): void {
  if (!/^refs\/harness-gui\/[A-Za-z0-9_-]+\/[A-Za-z0-9_-]+$/.test(ref) || /\s/.test(ref)) {
    throw new Error('invalid checkpoint reference');
  }
}

export function checkpointRef(session: string, turn: number, phase: 'start' | 'end'): string {
  validateSession(session);
  validateTurn(turn);
  if (phase !== 'start' && phase !== 'end') throw new Error('invalid checkpoint phase');
  return `${namespace}${session}/${turn}-${phase}`;
}

async function resolveCommit(
  cwd: string,
  ref: string,
  signal?: AbortSignal,
): Promise<string | null> {
  const result = await gitCommand(cwd, ['rev-parse', '--verify', '--quiet', `${ref}^{commit}`], {
    signal,
    allowFailure: true,
  });
  if (result.exitCode === 0) return result.stdout.trim();
  if (result.exitCode === 1) return null;
  throw new Error(result.stderr.trim() || `could not resolve ${ref}`);
}

async function temporaryIndex<T>(fn: (env: Record<string, string>) => Promise<T>): Promise<T> {
  const directory = await mkdtemp(join(tmpdir(), 'harness-checkpoint-'));
  try {
    return await fn({ GIT_INDEX_FILE: join(directory, 'index') });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

async function capture(
  cwd: string,
  ref: string,
  immutable: boolean,
  signal?: AbortSignal,
): Promise<string> {
  signal?.throwIfAborted();
  if (immutable) {
    const existing = await resolveCommit(cwd, ref, signal);
    if (existing) return existing;
  }
  return temporaryIndex(async (env) => {
    const head = await resolveCommit(cwd, 'HEAD', signal);
    await gitCommand(cwd, ['read-tree', head ?? '--empty'], { signal, env });
    await gitCommand(cwd, ['add', '-A', '--', ...(await capturePaths(cwd, signal))], {
      signal,
      env,
    });
    const tree = (await gitCommand(cwd, ['write-tree'], { signal, env })).stdout.trim();
    const commit = (
      await gitCommand(
        cwd,
        ['commit-tree', tree, ...(head ? ['-p', head] : []), '-m', `Workspace checkpoint ${ref}`],
        {
          signal,
          env: {
            ...env,
            GIT_AUTHOR_NAME: 'Harness GUI',
            GIT_AUTHOR_EMAIL: 'checkpoint@localhost',
            GIT_COMMITTER_NAME: 'Harness GUI',
            GIT_COMMITTER_EMAIL: 'checkpoint@localhost',
          },
        },
      )
    ).stdout.trim();
    const updated = await gitCommand(
      cwd,
      ['update-ref', ref, commit, ...(immutable ? ['0'.repeat(commit.length)] : [])],
      {
        signal,
        env,
        allowFailure: immutable,
      },
    );
    if (updated.exitCode !== 0) {
      const existing = await resolveCommit(cwd, ref, signal);
      if (existing) return existing;
      throw new Error(updated.stderr.trim() || 'could not save checkpoint');
    }
    return commit;
  });
}

export async function captureTurnStart(
  cwd: string,
  session: string,
  turn: number,
  signal?: AbortSignal,
): Promise<string | null> {
  const ref = checkpointRef(session, turn, 'start');
  const root = await repositoryRoot(cwd, signal);
  if (!root) return null;
  await capture(root, ref, true, signal);
  return ref;
}

export async function summarizeCheckpointChanges(
  cwd: string,
  fromRef: string,
  toRef: string,
  signal?: AbortSignal,
): Promise<CheckpointFile[]> {
  validateRef(fromRef);
  validateRef(toRef);
  const args = ['diff', '--no-ext-diff', '--no-renames'];
  const stats = (
    await gitCommand(cwd, [...args, '--numstat', '-z', fromRef, toRef, '--'], { signal })
  ).stdout;
  const statuses = (
    await gitCommand(cwd, [...args, '--name-status', '-z', fromRef, toRef, '--'], { signal })
  ).stdout.split('\0');
  const byPath = new Map<string, CheckpointFile['status']>();
  for (let i = 0; i + 1 < statuses.length; i += 2) {
    byPath.set(
      statuses[i + 1]!,
      statuses[i] === 'A' ? 'added' : statuses[i] === 'D' ? 'deleted' : 'modified',
    );
  }
  return stats
    .split('\0')
    .filter(Boolean)
    .map((entry) => {
      const first = entry.indexOf('\t');
      const second = entry.indexOf('\t', first + 1);
      const additions = entry.slice(0, first);
      const deletions = entry.slice(first + 1, second);
      const path = entry.slice(second + 1);
      return {
        path,
        status: byPath.get(path) ?? 'modified',
        ...(additions === '-' ? {} : { additions: Number(additions) }),
        ...(deletions === '-' ? {} : { deletions: Number(deletions) }),
      };
    });
}

export async function captureTurnEnd(
  cwd: string,
  session: string,
  turn: number,
  signal?: AbortSignal,
): Promise<Checkpoint> {
  const ref = checkpointRef(session, turn, 'end');
  const root = await repositoryRoot(cwd, signal);
  const checkpoint: Checkpoint = {
    turn,
    ref,
    state: root ? 'ready' : 'unavailable',
    files: [],
    additions: 0,
    deletions: 0,
    createdAt: Date.now(),
  };
  if (!root) return checkpoint;
  await capture(root, ref, true, signal);
  const startRef = checkpointRef(session, turn, 'start');
  if (await resolveCommit(root, startRef, signal)) {
    checkpoint.files = await summarizeCheckpointChanges(root, startRef, ref, signal);
    for (const file of checkpoint.files) {
      checkpoint.additions += file.additions ?? 0;
      checkpoint.deletions += file.deletions ?? 0;
    }
  }
  return checkpoint;
}

export async function listCheckpointRefs(
  cwd: string,
  session: string,
  signal?: AbortSignal,
): Promise<string[]> {
  validateSession(session);
  const root = await repositoryRoot(cwd, signal);
  if (!root) return [];
  return (
    await gitCommand(root, ['for-each-ref', '--format=%(refname)', `${namespace}${session}/`], {
      signal,
    })
  ).stdout
    .split('\n')
    .filter(Boolean);
}

async function restoreFiles(
  cwd: string,
  current: string,
  target: string,
  signal?: AbortSignal,
): Promise<void> {
  await temporaryIndex(async (env) => {
    await gitCommand(cwd, ['read-tree', current], { signal, env });
    // The disposable index includes current nonignored files, so Git also removes newly created files.
    await gitCommand(cwd, ['read-tree', '--reset', '-u', target], { signal, env });
  });
  const head = await resolveCommit(cwd, 'HEAD', signal);
  if (head) await gitCommand(cwd, ['read-tree', '--reset', head], { signal });
}

export async function restoreNamedSnapshot(
  cwd: string,
  ref: string,
  signal?: AbortSignal,
): Promise<{ safetyRef: string }> {
  validateRef(ref);
  const root = await repositoryRoot(cwd, signal);
  if (!root) throw new Error(requiresGit);
  const target = await resolveCommit(root, ref, signal);
  if (!target) throw new Error(`checkpoint is unavailable: ${ref}`);
  const safetyRef = `${ref.slice(0, ref.lastIndexOf('/'))}/safety-${crypto.randomUUID()}`;
  const indexPath = (
    await gitCommand(root, ['rev-parse', '--git-path', 'index'], { signal })
  ).stdout.trim();
  const index = await temporaryIndex(async (env) => {
    // write-tree may create an index even for an unborn repository. Work from an exact copy.
    await copyFile(resolve(root, indexPath), env.GIT_INDEX_FILE!).catch(
      (error: NodeJS.ErrnoException) => {
        if (error.code !== 'ENOENT') throw error;
      },
    );
    return gitCommand(root, ['write-tree'], { signal, env, allowFailure: true });
  });
  if (index.exitCode !== 0)
    throw new Error(
      'Cannot restore checkpoint: unable to save staged state; resolve unmerged index entries first',
    );
  // Pin the real index separately; the working-tree snapshot cannot preserve partial staging.
  await gitCommand(root, ['update-ref', `${safetyRef}-index`, index.stdout.trim()], { signal });
  const current = await capture(root, safetyRef, false, signal);
  const currentPaths = new Set(
    (await gitCommand(root, ['ls-tree', '-r', '--name-only', '-z', current], { signal })).stdout
      .split('\0')
      .filter(Boolean),
  );
  const targetPaths = (
    await gitCommand(root, ['ls-tree', '-r', '--name-only', '-z', target], { signal })
  ).stdout
    .split('\0')
    .filter(Boolean);
  for (const path of targetPaths) {
    for (
      let candidate = path;
      candidate;
      candidate = candidate.includes('/') ? candidate.slice(0, candidate.lastIndexOf('/')) : ''
    ) {
      if (currentPaths.has(candidate)) continue;
      const info = await lstat(resolve(root, candidate)).catch((error: NodeJS.ErrnoException) => {
        if (error.code !== 'ENOENT' && error.code !== 'ENOTDIR') throw error;
        return null;
      });
      if (info && (!info.isDirectory() || candidate === path))
        throw new Error(
          `Cannot restore checkpoint: ${candidate} is not protected by the safety snapshot`,
        );
    }
  }
  try {
    await restoreFiles(root, current, target, signal);
  } catch (error) {
    try {
      // Recovery must finish even if the caller cancelled the destructive operation.
      await restoreFiles(root, target, current);
    } catch (recoveryError) {
      throw new AggregateError(
        [error, recoveryError],
        `checkpoint restore and recovery failed; recovery snapshot: ${safetyRef}`,
      );
    }
    throw new Error(`checkpoint restore failed; recovery snapshot: ${safetyRef}`, { cause: error });
  }
  return { safetyRef };
}

export async function deleteSnapshot(
  cwd: string,
  ref: string,
  signal?: AbortSignal,
): Promise<void> {
  validateRef(ref);
  const root = await repositoryRoot(cwd, signal);
  if (root) await gitCommand(root, ['update-ref', '-d', ref], { signal });
}

export async function deleteSessionSnapshots(
  cwd: string,
  session: string,
  signal?: AbortSignal,
): Promise<void> {
  for (const ref of await listCheckpointRefs(cwd, session, signal))
    await deleteSnapshot(cwd, ref, signal);
}

export async function copySessionSnapshots(
  cwd: string,
  source: string,
  target: string,
  throughTurn: number,
  signal?: AbortSignal,
): Promise<void> {
  validateSession(source);
  validateSession(target);
  validateTurn(throughTurn);
  if (source === target) throw new Error('checkpoint fork requires a different session');
  const refs = await listCheckpointRefs(cwd, source, signal);
  const commands: string[] = [];
  for (const ref of refs) {
    const match = /\/(\d+)-(start|end)$/.exec(ref);
    if (!match || Number(match[1]) > throughTurn) continue;
    const commit = await resolveCommit(cwd, ref, signal);
    if (!commit) throw new Error(`checkpoint is unavailable: ${ref}`);
    commands.push(
      `create ${checkpointRef(target, Number(match[1]), match[2] as 'start' | 'end')} ${commit}`,
    );
  }
  if (commands.length)
    await gitCommand(cwd, ['update-ref', '--stdin'], { signal, input: `${commands.join('\n')}\n` });
}

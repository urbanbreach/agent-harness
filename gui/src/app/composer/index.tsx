// Placeholder until the composer slice lands; keeps the shell contract compiling.
export type ComposerTarget =
  | { kind: 'task'; sessionId: string; cwd: string }
  | { kind: 'new'; projectId: string | null; cwd: string | null };

/** Approval/question cards, queued follow-ups and the composer card. */
export function Composer(_props: { target: ComposerTarget }) {
  return null;
}

/** The workspace row below the composer (project, local/worktree, branch, usage). */
export function WorkspaceFooter(_props: { target: ComposerTarget }) {
  return null;
}

// Work that must finish before a prompt is sent, such as the per-turn checkpoint.
export type BeforeSubmit = (context: { sessionId: string; cwd: string }) => Promise<void>;

const hooks = new Set<BeforeSubmit>();

export function registerBeforeSubmit(hook: BeforeSubmit): () => void {
  hooks.add(hook);
  return () => hooks.delete(hook);
}

/** Runs every hook; a failing hook is reported but never blocks the prompt. */
export async function runBeforeSubmit(
  context: { sessionId: string; cwd: string },
  onError: (error: unknown) => void,
): Promise<void> {
  await Promise.all(
    [...hooks].map((hook) =>
      hook(context).catch((error: unknown) => {
        onError(error);
      }),
    ),
  );
}

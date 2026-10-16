import type { Json } from '../../backend/protocol';
import { basename } from 'node:path';
import { pathToFileURL } from 'node:url';
import type { AppStore, Draft, Selection } from '../../store/store';
import type { FollowUp } from './Queue';
import { fileTags } from './completion';
import { recordPromptDisplay } from './prompt-display';
export function canSteerDraft(draft: Draft): boolean {
  return (
    !draft.fileTags?.length &&
    !draft.agentTags?.length &&
    !draft.resourceTags?.length &&
    !/(^|\s)@/.test(draft.text) &&
    !(draft.attachments ?? []).some((item) => item.path && !item.mime.startsWith('image/'))
  );
}

export function prepareDraft(draft: Draft, cwd: string): Draft {
  const files = (draft.attachments ?? []).filter(
    (item) => item.path && !item.mime.startsWith('image/'),
  );
  let text = draft.text.trim();
  const tags = fileTags(text, cwd);
  for (const attachment of files) {
    if (text) text += ' ';
    const start = text.length;
    const value = `@${attachment.path}`;
    text += value;
    tags.push({
      path: attachment.path!,
      filename: attachment.name ?? basename(attachment.path!),
      url: pathToFileURL(attachment.path!).href,
      mime: attachment.directory ? 'application/x-directory' : attachment.mime,
      source: { start, end: text.length, value },
    });
  }
  return {
    ...draft,
    text,
    agentTags: [...text.matchAll(/(^|\s)(@agent:\S+)/g)].flatMap((match) => {
      const tag = draft.agentTags?.find((entry) => entry.source.value === match[2]);
      const start = match.index! + match[1]!.length;
      return tag
        ? [{ ...tag, source: { start, end: start + match[2]!.length, value: match[2]! } }]
        : [];
    }),
    resourceTags: [...text.matchAll(/(^|\s)(@resource:\S+)/g)].flatMap((match) => {
      const tag = draft.resourceTags?.find((entry) => entry.source.value === match[2]);
      const start = match.index! + match[1]!.length;
      return tag
        ? [{ ...tag, source: { start, end: start + match[2]!.length, value: match[2]! } }]
        : [];
    }),
    fileTags: [
      ...(draft.fileTags ?? []).filter((tag) => !tags.some((item) => item.path === tag.path)),
      ...tags,
    ],
    attachments: (draft.attachments ?? []).filter(
      (item) => item.mime.startsWith('image/') && item.dataBase64,
    ),
  };
}
export function queuedDrafts(store: AppStore): Record<string, FollowUp[]> {
  return (store.getSnapshot().preferences.composerQueues ?? {}) as unknown as Record<
    string,
    FollowUp[]
  >;
}
export function setQueuedDrafts(store: AppStore, key: string, items: FollowUp[]) {
  store.setPreferences({
    composerQueues: { ...queuedDrafts(store), [key]: items } as unknown as Json,
  });
}
const initialized = new WeakSet<AppStore>();
/** Delivery follows every loaded session, not only the composer currently on screen. */
export function startQueueDelivery(store: AppStore) {
  if (initialized.has(store)) return;
  initialized.add(store);
  const delivering = new Set<string>();
  const failed = new Set<string>();
  const awaitingAdmission = new Map<string, number>();
  function deliver() {
    const state = store.getSnapshot();
    if (state.backend.state !== 'connected') return;
    for (const [key, items] of Object.entries(queuedDrafts(store))) {
      const first = items[0];
      if (!first || delivering.has(key) || failed.has(first.id)) continue;
      let target: Selection;
      try {
        target = JSON.parse(key) as Selection;
      } catch {
        continue;
      }
      if (target.kind !== 'session') continue;
      const view = state.views[target.sessionId];
      const beforeAdmission = awaitingAdmission.get(key);
      if (beforeAdmission !== undefined) {
        if (!view || view.events.length <= beforeAdmission) continue;
        awaitingAdmission.delete(key);
      }
      if (
        !view ||
        view.status === 'working' ||
        view.status === 'waiting' ||
        view.status === 'background' ||
        view.activeProviderRequests.size ||
        view.permissions.some((permission) => !permission.decision)
      )
        continue;
      const cwd = Object.entries(state.sessions).find(([, rows]) =>
        rows.some((row) => row.runId === target.sessionId),
      )?.[0];
      if (!cwd) continue;
      delivering.add(key);
      awaitingAdmission.set(key, view.events.length);
      const text = first.draft.text.trim();
      const operation = text.startsWith('!')
        ? store.sendIntent(target.sessionId, { kind: 'runShell', command: text.slice(1).trim() })
        : store.submit(target, {
            ...prepareDraft(first.draft, cwd),
            mode: 'submit',
            consumeDraft: false,
            runBeforeSubmit: first.draft.invocation
              ? ({ sessionId }) =>
                  recordPromptDisplay(
                    sessionId,
                    prepareDraft(first.draft, cwd).text,
                    first.draft.invocation!,
                  )
              : undefined,
          });
      void operation
        .then(() => {
          setQueuedDrafts(
            store,
            key,
            (queuedDrafts(store)[key] ?? []).filter((item) => item.id !== first.id),
          );
        })
        .catch(() => {
          awaitingAdmission.delete(key);
          failed.add(first.id);
        })
        .finally(() => {
          delivering.delete(key);
          deliver();
        });
    }
  }
  store.subscribe(deliver);
  deliver();
}

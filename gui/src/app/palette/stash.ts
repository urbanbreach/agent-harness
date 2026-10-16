import { getAppStore } from '../../store';
import type { Draft } from '../../store/store';

export function restoreStash(index: number) {
  const store = getAppStore();
  const state = store.getSnapshot();
  const stashes = (state.preferences.stashedDrafts ?? []) as unknown as Draft[];
  const draft = stashes[index];
  if (!draft) return;
  const current = store.getDraft(state.selection);
  const remaining = stashes.filter((_, at) => at !== index);
  if (current?.text || current?.attachments?.length) remaining.unshift(current);
  store.setPreferences({ stashedDrafts: remaining as never });
  store.setDraft(state.selection, draft);
}

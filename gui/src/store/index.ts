import { useSyncExternalStore } from 'react';
import { AppStore, draftKey, type Selection, type StoreState } from './store';
export { AppStore } from './store';
export type { StoreState, Project, Selection, Draft, DraftAttachment, SubmitDraft } from './store';
export { draftKey } from './store';
let activeStore: AppStore | undefined;
export function setAppStore(store: AppStore) {
  activeStore = store;
}
export function getAppStore(): AppStore {
  if (!activeStore) throw new Error('Application store has not been initialized');
  return activeStore;
}
export function useStore<T>(select: (state: StoreState) => T): T {
  const store = getAppStore();
  return useSyncExternalStore(
    store.subscribe,
    () => select(store.getSnapshot()),
    () => select(store.getSnapshot()),
  );
}
export function useProjects() {
  return useStore((state) => state.projects);
}
export function useSelectedProject() {
  return useStore((state) =>
    state.projects.find((project) => project.id === state.selectedProject),
  );
}
export function useSessionView(id: string | undefined) {
  return useStore((state) => (id ? state.views[id] : undefined));
}
export function usePreferences() {
  return useStore((state) => state.preferences);
}
export function useModels(cwd: string) {
  return useStore((state) => state.models[cwd]);
}
export function useCommands(cwd: string) {
  return useStore((state) => state.commands[cwd]);
}
export function useFiles(cwd: string) {
  return useStore((state) => state.files[cwd]);
}
export function useDraft(target: Selection) {
  return useStore((state) => state.drafts[draftKey(target)]);
}
export function useSessionStatus(id: string | undefined) {
  return useStore(() => (id ? getAppStore().sessionStatus(id) : 'idle'));
}

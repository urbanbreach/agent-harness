import { useSyncExternalStore } from 'react';
import { createBackend } from '../backend/fixture';
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
export async function initializeStore(cwd?: string) {
  const store = new AppStore(await createBackend());
  setAppStore(store);
  await store.initialize(cwd);
  return store;
}
export function useStore<T>(select: (state: StoreState) => T): T {
  const store = getAppStore();
  return useSyncExternalStore(
    store.subscribe,
    () => select(store.getSnapshot()),
    () => select(store.getSnapshot()),
  );
}
const emptySessions: StoreState['sessions'][string] = [];
export function useProjects() {
  return useStore((state) => state.projects);
}
export function useSessions(cwd: string) {
  return useStore((state) => state.sessions[cwd] || emptySessions);
}
export function useSelectedTask() {
  return useStore((state) => state.selectedTask);
}
export function useSelectedProject() {
  return useStore((state) =>
    state.projects.find((project) => project.id === state.selectedProject),
  );
}
export function useSessionView(id: string | undefined) {
  return useStore((state) => (id ? state.views[id] : undefined));
}
export function useBackendStatus() {
  return useStore((state) => state.backend);
}
export function usePreferences() {
  return useStore((state) => state.preferences);
}
export function useLayout() {
  return useStore((state) => state.layout);
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
export function useSelection() {
  return useStore((state) => state.provisionalSelection ?? state.selection);
}
export function useDraft(target: Selection) {
  return useStore((state) => state.drafts[draftKey(target)]);
}
export function useSessionStatus(id: string | undefined) {
  return useStore(() => (id ? getAppStore().sessionStatus(id) : 'idle'));
}
export function usePinnedSessions(cwd: string) {
  return useStore((state) => state.pins[cwd] ?? emptyPins);
}
const emptyPins: string[] = [];
export function useProjectModel(projectId: string | undefined) {
  return useStore((state) => (projectId ? state.projectModels[projectId] : undefined));
}

import { useSyncExternalStore } from 'react';
import { createBackend } from '../backend/fixture';
import { AppStore, type StoreState } from './store';
export { AppStore } from './store';
export type { StoreState, Project } from './store';
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

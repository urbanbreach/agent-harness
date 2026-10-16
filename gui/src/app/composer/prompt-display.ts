import { createHash } from 'node:crypto';
import { useEffect, useSyncExternalStore } from 'react';
import { getAppStore } from '../../store';

let displays: Record<string, string> = {};
let loaded: Promise<void> | undefined;
const listeners = new Set<() => void>();
function key(sessionId: string, text: string): string {
  return `${sessionId}:${createHash('sha256').update(text).digest('hex')}`;
}
function notify() {
  listeners.forEach((listener) => listener());
}
async function load() {
  loaded ??= getAppStore()
    .backend.call('state.read', { name: 'prompt-display' })
    .then((value) => {
      if (value && typeof value === 'object' && !Array.isArray(value)) {
        displays = Object.fromEntries(
          Object.entries(value).filter(
            (entry): entry is [string, string] => typeof entry[1] === 'string',
          ),
        );
        notify();
      }
    });
  await loaded;
}
export async function recordPromptDisplay(sessionId: string, text: string, invocation: string) {
  await load();
  displays = { ...displays, [key(sessionId, text)]: invocation };
  await getAppStore().backend.call('state.write', { name: 'prompt-display', value: displays });
  notify();
}
export function displayTextFor(sessionId: string, text: string): string {
  return displays[key(sessionId, text)] ?? text;
}
export function usePromptDisplay(sessionId: string, text: string): string {
  useEffect(() => {
    void load().catch(() => {});
  }, []);
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    () => displayTextFor(sessionId, text),
  );
}

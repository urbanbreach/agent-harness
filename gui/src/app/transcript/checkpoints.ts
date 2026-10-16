import { t } from '../../i18n';
import { useEffect, useRef, useSyncExternalStore } from 'react';
import type { SessionView } from '../../model/session';
import { getAppStore, type AppStore } from '../../store';
import type { Json } from '../../backend/protocol';
import {
  captureTurnStart,
  captureTurnEnd,
  checkpointRef,
  type Checkpoint,
} from '../../workspace/checkpoints';
import { registerBeforeSubmit } from '../turn-hooks';

export interface TurnCheckpoint extends Checkpoint {
  startRef: string;
  endRef: string;
  cwd: string;
}
interface PendingCheckpoint {
  checkpoint: TurnCheckpoint;
  knownTurns: string[];
}
interface SavedCheckpoints {
  nextIndex: number;
  turns: Record<string, TurnCheckpoint>;
  pending: PendingCheckpoint[];
}
interface SessionCheckpoints {
  data: SavedCheckpoints;
  loaded: Promise<void>;
  operations: Promise<void>;
  listeners: Set<() => void>;
}
const emptyMapping: Record<string, TurnCheckpoint> = Object.freeze({});
const registries = new WeakMap<
  AppStore,
  {
    sessions: Map<string, SessionCheckpoints>;
    document: Promise<Record<string, SavedCheckpoints>>;
    writes: Promise<void>;
  }
>();

function immutable(checkpoint: TurnCheckpoint): TurnCheckpoint {
  checkpoint.files.forEach(Object.freeze);
  Object.freeze(checkpoint.files);
  return Object.freeze(checkpoint);
}
function registry(store = getAppStore()) {
  let result = registries.get(store);
  if (!result) {
    const sessions = new Map<string, SessionCheckpoints>();
    result = {
      sessions,
      document: store.backend
        .call('state.read', { name: 'turn-checkpoints' })
        .then((value) => (value ?? {}) as unknown as Record<string, SavedCheckpoints>),
      writes: Promise.resolve(),
    };
    registries.set(store, result);
    // Captured turns can settle while another task owns the transcript viewport.
    store.subscribe(() => {
      for (const [id, state] of sessions) {
        const view = store.getSnapshot().views[id];
        if (
          view &&
          (state.data.pending.length ||
            Object.values(state.data.turns).some((c) => c.ref === c.startRef))
        ) {
          void reconcile(id, view, store).catch(console.error);
        }
      }
    });
  }
  return result;
}
function session(sessionId: string, store = getAppStore()): SessionCheckpoints {
  const registryState = registry(store);
  let state = registryState.sessions.get(sessionId);
  if (!state) {
    state = {
      data: { nextIndex: 1, turns: emptyMapping, pending: [] },
      loaded: Promise.resolve(),
      operations: Promise.resolve(),
      listeners: new Set(),
    };
    registryState.sessions.set(sessionId, state);
    const target = state;
    state.loaded = registryState.document.then((document) => {
      const saved = document[sessionId];
      if (saved) {
        Object.values(saved.turns).forEach(immutable);
        saved.pending.forEach((pending) => immutable(pending.checkpoint));
        target.data = {
          ...saved,
          nextIndex: Math.max(1, saved.nextIndex),
          turns: Object.freeze(saved.turns),
        };
      }
      target.listeners.forEach((listener) => listener());
    });
  }
  return state;
}
function enqueue(
  sessionId: string,
  operation: (state: SessionCheckpoints) => Promise<void>,
  store = getAppStore(),
) {
  const state = session(sessionId, store);
  const next = state.operations.then(async () => {
    await state.loaded;
    await operation(state);
  });
  state.operations = next.catch(() => {});
  return next;
}
async function save(sessionId: string, state: SessionCheckpoints, store = getAppStore()) {
  state.listeners.forEach((listener) => listener());
  const target = registry(store);
  const data = state.data;
  const operation = target.writes.then(async () => {
    const document = { ...(await target.document), [sessionId]: data };
    await store.backend.call('state.write', {
      name: 'turn-checkpoints',
      value: document as unknown as Json,
    });
    target.document = Promise.resolve(document);
  });
  target.writes = operation.catch(() => {});
  await operation;
}
function reconcile(sessionId: string, view: SessionView, store = getAppStore()) {
  return enqueue(
    sessionId,
    async (state) => {
      let changed = false;
      const turns = { ...state.data.turns };
      const pending = [...state.data.pending];
      for (const turn of view.turns) {
        if (!turns[turn.id]) {
          const index = pending.findIndex((item) => !item.knownTurns.includes(turn.id));
          if (index >= 0) {
            turns[turn.id] = pending.splice(index, 1)[0]!.checkpoint;
            changed = true;
          }
        }
        const checkpoint = turns[turn.id];
        if (
          checkpoint &&
          checkpoint.ref === checkpoint.startRef &&
          ['completed', 'failed', 'interrupted'].includes(turn.status)
        ) {
          const end = await captureTurnEnd(checkpoint.cwd, sessionId, checkpoint.turn);
          turns[turn.id] = immutable({
            ...end,
            cwd: checkpoint.cwd,
            startRef: checkpoint.startRef,
            endRef: end.ref,
          });
          changed = true;
        }
      }
      if (changed) {
        state.data = { ...state.data, turns: Object.freeze(turns), pending };
        await save(sessionId, state, store);
      }
    },
    store,
  );
}
export async function prepareTurnCheckpoint(
  context: { sessionId: string; cwd: string },
  store = getAppStore(),
) {
  const view = store.getSnapshot().views[context.sessionId];
  if (!view) return;
  await reconcile(context.sessionId, view, store);
  await enqueue(
    context.sessionId,
    async (current) => {
      const knownTurns = view.turns.map((item) => item.id);
      if (
        current.data.pending.some(
          (pending) =>
            pending.knownTurns.length === knownTurns.length &&
            pending.knownTurns.every((id, index) => id === knownTurns[index]),
        )
      )
        return;
      const turn = Math.max(1, current.data.nextIndex, view.turns.length + 1);
      const startRef = await captureTurnStart(context.cwd, context.sessionId, turn);
      if (!startRef) return;
      const checkpoint = immutable({
        turn,
        ref: startRef,
        state: 'unavailable',
        files: [],
        additions: 0,
        deletions: 0,
        createdAt: Date.now(),
        startRef,
        endRef: checkpointRef(context.sessionId, turn, 'end'),
        cwd: context.cwd,
      });
      current.data = {
        ...current.data,
        nextIndex: turn + 1,
        pending: [...current.data.pending, { checkpoint, knownTurns }],
      };
      await save(context.sessionId, current, store);
    },
    store,
  );
}

/** Includes start-only checkpoints so rewind becomes available before a diff is ready. */
export function useTurnCheckpoints(
  sessionId: string,
  view: SessionView,
): Record<string, TurnCheckpoint> {
  const latest = useRef(view);
  latest.current = view;
  const state = session(sessionId);
  const mapping = useSyncExternalStore(
    (listener) => {
      state.listeners.add(listener);
      return () => {
        state.listeners.delete(listener);
      };
    },
    () => state.data.turns,
    () => state.data.turns,
  );
  useEffect(() => {
    void reconcile(sessionId, latest.current).catch(console.error);
    return registerBeforeSubmit(async (context) => {
      if (context.sessionId !== sessionId) return;
      await prepareTurnCheckpoint(context);
    });
  }, [sessionId, state]);
  useEffect(() => {
    void reconcile(sessionId, view).catch(console.error);
  }, [sessionId, view.turns]);
  return mapping;
}

export async function getTurnCheckpoint(
  sessionId: string,
  turnId: string,
): Promise<TurnCheckpoint | undefined> {
  const state = session(sessionId);
  await state.loaded;
  await state.operations;
  return state.data.turns[turnId];
}

/** Call after the corresponding Git snapshots have been copied to the fork. */
export async function copyTurnCheckpoints(
  sourceSession: string,
  targetSession: string,
  throughTurnId: string,
): Promise<void> {
  if (sourceSession === targetSession) throw new Error(t('transcript.fork_same_session'));
  const source = session(sourceSession);
  await source.loaded;
  await source.operations;
  const through = source.data.turns[throughTurnId];
  if (!through) throw new Error(t('transcript.fork_no_checkpoint'));
  const copied: Record<string, TurnCheckpoint> = {};
  for (const [turnId, checkpoint] of Object.entries(source.data.turns)) {
    if (checkpoint.turn > through.turn) continue;
    const startRef = checkpointRef(targetSession, checkpoint.turn, 'start');
    const endRef = checkpointRef(targetSession, checkpoint.turn, 'end');
    copied[turnId] = immutable({
      ...checkpoint,
      startRef,
      endRef,
      ref: checkpoint.ref === checkpoint.startRef ? startRef : endRef,
    });
  }
  await enqueue(targetSession, async (target) => {
    target.data = {
      ...target.data,
      turns: Object.freeze({ ...copied, ...target.data.turns }),
      nextIndex: Math.max(target.data.nextIndex, through.turn + 1),
    };
    await save(targetSession, target);
  });
}

export function useTurnCheckpoint(sessionId: string, turnId: string): TurnCheckpoint | undefined {
  const state = session(sessionId);
  return useSyncExternalStore(
    (listener) => {
      state.listeners.add(listener);
      return () => {
        state.listeners.delete(listener);
      };
    },
    () => state.data.turns[turnId],
    () => state.data.turns[turnId],
  );
}

import type { Backend } from '../backend/client';
import type { Json } from '../backend/protocol';
import type { Persist } from '../state/persist';

/** One backend document preserves arbitrary UI keys without leaking them into filenames. */
export function createBackendPersist(backend: Backend): Persist {
  let document: Promise<Record<string, Json>> | undefined;
  let writes = Promise.resolve();
  const load = () =>
    (document ??= backend
      .call('state.read', { name: 'ui' })
      .then((value) => (value ?? {}) as Record<string, Json>));
  return {
    async read<T>(key: string) {
      await writes;
      return structuredClone((await load())[key]) as T | undefined;
    },
    async write<T>(key: string, value: T) {
      const operation = writes.then(async () => {
        const previous = await load();
        const serialized = JSON.stringify(value);
        if (serialized === JSON.stringify(previous[key])) return;
        const next = { ...previous };
        if (serialized === undefined) delete next[key];
        else next[key] = JSON.parse(serialized) as Json;
        await backend.call('state.write', { name: 'ui', value: next });
        document = Promise.resolve(next);
      });
      writes = operation.catch(() => {});
      return operation;
    },
  };
}

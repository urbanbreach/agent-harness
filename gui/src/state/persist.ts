export interface Persist {
  read<T>(key: string): Promise<T | undefined>;
  write<T>(key: string, value: T): Promise<void>;
}

export function createMemoryPersist(): Persist {
  const values = new Map<string, unknown>();
  return {
    async read<T>(key: string) {
      return values.get(key) as T | undefined;
    },
    async write<T>(key: string, value: T) {
      values.set(key, structuredClone(value));
    },
  };
}

export const memoryPersist = createMemoryPersist();

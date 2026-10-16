import type { Method, Methods, Notification, Response, RpcError } from './protocol';

export type BackendStatus = {
  state: 'connecting' | 'connected' | 'failed' | 'closed';
  error?: string;
  reconnectable: boolean;
};
export interface Backend {
  call<M extends Method>(method: M, params: Methods[M]['params']): Promise<Methods[M]['result']>;
  subscribe(listener: (notification: Notification) => void): () => void;
  subscribeStatus(listener: () => void): () => void;
  getStatus(): BackendStatus;
  getStderr(): string;
  reconnect(): Promise<void>;
  close(): void | Promise<void>;
}
export class BackendError extends Error {
  constructor(
    public readonly code: RpcError['code'],
    message: string,
  ) {
    super(message);
    this.name = 'BackendError';
  }
}

/** Incremental UTF-8 decoder: chunks need not end at a character or line boundary. */
export class JsonLines {
  private decoder = new TextDecoder('utf-8', { fatal: true });
  private buffer = '';
  constructor(private receive: (message: unknown) => void) {}
  push(chunk: Uint8Array) {
    this.consume(this.decoder.decode(chunk, { stream: true }));
  }
  end() {
    this.consume(this.decoder.decode());
    if (this.buffer.trim()) throw new Error('Backend ended with an incomplete JSON line');
  }
  private consume(text: string) {
    this.buffer += text;
    let end: number;
    while ((end = this.buffer.indexOf('\n')) >= 0) {
      const line = this.buffer.slice(0, end);
      this.buffer = this.buffer.slice(end + 1);
      if (line.trim()) this.receive(JSON.parse(line));
    }
  }
}

export class BackendClient implements Backend {
  private process?: Bun.Subprocess<'pipe', 'pipe', 'pipe'>;
  private generation = 0;
  private closing?: Promise<void>;
  private nextId = 1;
  private pending = new Map<
    number,
    { resolve: (value: unknown) => void; reject: (error: Error) => void }
  >();
  private listeners = new Set<(message: Notification) => void>();
  private statusListeners = new Set<() => void>();
  private status: BackendStatus = { state: 'connecting', reconnectable: false };
  private stderr = '';
  constructor(
    private readonly command = [
      process.env.HARNESS_BIN || 'harness',
      'gui-server',
      ...(process.env.HARNESS_GUI_MOCK === '1' ? ['--mock'] : []),
      ...(process.env.HARNESS_GUI_MOCK_SCRIPT
        ? ['--mock-script', process.env.HARNESS_GUI_MOCK_SCRIPT]
        : []),
    ],
  ) {
    this.start();
  }
  getStatus = () => this.status;
  getStderr = () => this.stderr;
  subscribe = (listener: (notification: Notification) => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  subscribeStatus = (listener: () => void) => {
    this.statusListeners.add(listener);
    return () => {
      this.statusListeners.delete(listener);
    };
  };
  private setStatus(status: BackendStatus) {
    this.status = status;
    for (const listener of this.statusListeners) listener();
  }
  private fail(error: Error) {
    for (const pending of this.pending.values()) pending.reject(error);
    this.pending.clear();
    this.setStatus({ state: 'failed', error: error.message, reconnectable: true });
  }
  private start() {
    const generation = ++this.generation;
    this.stderr = '';
    this.setStatus({ state: 'connecting', reconnectable: false });
    try {
      const child = Bun.spawn(this.command, {
        env: { ...process.env },
        stdin: 'pipe',
        stdout: 'pipe',
        stderr: 'pipe',
      });
      this.process = child;
      this.setStatus({ state: 'connected', reconnectable: false });
      const active = () => generation === this.generation;
      const lines = new JsonLines((message) => {
        if (active()) this.receive(message);
      });
      void (async () => {
        try {
          for await (const chunk of child.stdout) lines.push(chunk);
          lines.end();
        } catch (error) {
          if (active()) {
            child.kill();
            this.fail(asError(error));
          }
        }
      })();
      void (async () => {
        const decoder = new TextDecoder();
        try {
          for await (const chunk of child.stderr)
            if (active())
              this.stderr = (this.stderr + decoder.decode(chunk, { stream: true })).slice(-65536);
        } catch (error) {
          if (active()) this.stderr += `\n${asError(error).message}`;
        }
      })();
      void child.exited.then(
        (code) => {
          if (active() && this.status.state !== 'failed')
            this.fail(
              new Error(`Backend exited (${code})${this.stderr ? `: ${this.stderr.trim()}` : ''}`),
            );
        },
        (error) => {
          if (active()) this.fail(asError(error));
        },
      );
    } catch (error) {
      this.fail(asError(error));
    }
  }
  private receive(message: unknown) {
    if (!message || typeof message !== 'object') throw new Error('Invalid backend message');
    if ('id' in message) {
      const response = message as Response;
      if (!Number.isSafeInteger(response.id)) throw new Error('Invalid backend response id');
      if (!('result' in response) && !('error' in response))
        throw new Error('Backend response has no result or error');
      if (
        'error' in response &&
        (!response.error ||
          typeof response.error.message !== 'string' ||
          !['invalid_params', 'not_found', 'failed'].includes(response.error.code))
      )
        throw new Error('Invalid backend error');
      const pending = this.pending.get(response.id);
      if (!pending) return;
      this.pending.delete(response.id);
      if ('error' in response)
        pending.reject(new BackendError(response.error.code, response.error.message));
      else if ('result' in response) pending.resolve(response.result);
    } else if ('method' in message && 'params' in message) {
      const notification = message as Notification;
      if (notification.method !== 'session.update' && notification.method !== 'auth.update')
        throw new Error('Unknown backend notification');
      for (const listener of this.listeners) listener(notification);
    } else throw new Error('Invalid backend message envelope');
  }
  call<M extends Method>(method: M, params: Methods[M]['params']): Promise<Methods[M]['result']> {
    if (this.status.state !== 'connected' || !this.process)
      return Promise.reject(new Error(this.status.error || 'Backend is not connected'));
    const id = this.nextId++;
    const { promise, resolve, reject } = Promise.withResolvers<Methods[M]['result']>();
    this.pending.set(id, { resolve: (value) => resolve(value as Methods[M]['result']), reject });
    try {
      this.process.stdin.write(JSON.stringify({ id, method, params }) + '\n');
      this.process.stdin.flush();
    } catch (error) {
      this.pending.delete(id);
      reject(asError(error));
      this.fail(asError(error));
    }
    return promise;
  }
  async reconnect() {
    await this.close();
    this.start();
  }
  close(): Promise<void> {
    if (this.closing) return this.closing;
    ++this.generation;
    const child = this.process;
    for (const pending of this.pending.values())
      pending.reject(new Error('Backend connection closed'));
    this.pending.clear();
    this.setStatus({ state: 'closed', reconnectable: true });
    this.closing = (async () => {
      if (!child) return;
      let timer: Timer | undefined;
      try {
        child.stdin.end();
        timer = setTimeout(() => child.kill('SIGKILL'), 1000);
        await child.exited;
      } finally {
        clearTimeout(timer);
        if (this.process === child) this.process = undefined;
      }
    })().finally(() => {
      this.closing = undefined;
    });
    return this.closing;
  }
}
function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

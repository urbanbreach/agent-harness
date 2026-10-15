import { BackendClient, BackendError, type Backend, type BackendStatus } from './client';
import type {
  EventEnvelopeV1,
  Json,
  Method,
  Methods,
  Notification,
  SessionRow,
  Update,
} from './protocol';

/** Scenario JSON: projects [{id,name,cwd}], sessions [{row:SessionRow,events:EventEnvelopeV1[]}],
 * optional updates [{sessionId,delayMs,update:Update}] scheduled after continue/create.
 * Times, ids and event bodies are authored synthetic data; each instance starts from a deep copy. */
export type Scenario = {
  projects: { id: string; name: string; cwd: string }[];
  sessions: { row: SessionRow; events: EventEnvelopeV1[] }[];
  updates?: { sessionId: string; delayMs: number; update: Update }[];
};
export class FixtureBackend implements Backend {
  private scenario: Scenario;
  private documents = new Map<string, Json>();
  private listeners = new Set<(notification: Notification) => void>();
  private statusListeners = new Set<() => void>();
  private timers = new Map<string, ReturnType<typeof setTimeout>[]>();
  private pins = new Map<string, string[]>();
  private status: BackendStatus = { state: 'connected', reconnectable: false };
  private serial = 0;
  constructor(scenario: Scenario) {
    this.scenario = structuredClone(scenario);
    this.documents.set('projects', structuredClone(scenario.projects));
  }
  getStatus = () => this.status;
  getStderr = () => '';
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
  private emit(sessionId: string, update: Update) {
    const session = this.scenario.sessions.find((s) => s.row.runId === sessionId);
    if (
      session &&
      update.kind === 'event' &&
      update.delivery === 'durable' &&
      !session.events.some((e) => e.seq === update.event.seq)
    )
      session.events.push(structuredClone(update.event));
    if (session && update.kind === 'exited') session.row.live = false;
    for (const listener of this.listeners)
      listener(structuredClone({ method: 'session.update', params: { sessionId, update } }));
  }
  private start(sessionId: string) {
    if (this.timers.has(sessionId)) return;
    this.timers.set(
      sessionId,
      (this.scenario.updates || [])
        .filter((u) => u.sessionId === sessionId)
        .map((u) => setTimeout(() => this.emit(sessionId, u.update), u.delayMs)),
    );
  }
  async call<M extends Method>(
    method: M,
    params: Methods[M]['params'],
  ): Promise<Methods[M]['result']> {
    if (this.status.state !== 'connected') throw new Error('Fixture backend is closed');
    const result = await this.dispatch(method, params);
    return structuredClone(result) as Methods[M]['result'];
  }
  private async dispatch(method: Method, params: unknown): Promise<unknown> {
    const p = params as Record<string, any>;
    const find = () => {
      const session = this.scenario.sessions.find(
        (s) => s.row.runDir === p.runDir || s.row.runId === p.sessionId,
      );
      if (!session) throw new BackendError('not_found', 'Fixture session not found');
      return session;
    };
    switch (method) {
      case 'app.initialize':
        return {
          version: 'fixture',
          home: '/fixture/home',
          guiStateDir: '/fixture/home/gui',
          defaultCwd: this.scenario.projects[0]?.cwd || '/fixture',
          mock: true,
        };
      case 'state.read':
        return this.documents.get(p.name) ?? null;
      case 'state.write':
        this.documents.set(p.name, structuredClone(p.value));
        return null;
      case 'sessions.list':
        return this.scenario.sessions
          .filter((s) => s.row.cwd === p.cwd)
          .map((s) => s.row)
          .sort((a, b) => (b.createdAt || '').localeCompare(a.createdAt || ''));
      case 'sessions.events':
        return find().events;
      case 'sessions.rename':
        find().row.catalog.run_name = p.title;
        return null;
      case 'sessions.delete': {
        const session = find();
        this.stop(session.row.runId);
        this.scenario.sessions.splice(this.scenario.sessions.indexOf(session), 1);
        return { trashedTo: `/fixture/trash/${session.row.runId}` };
      }
      case 'sessions.pins':
        return this.pins.get(p.cwd) || [];
      case 'sessions.setPinned': {
        const ids = (this.pins.get(p.cwd) || []).filter((id) => id !== p.runId);
        if (p.pinned) ids.push(p.runId);
        this.pins.set(p.cwd, ids);
        return null;
      }
      case 'sessions.search':
        return this.scenario.sessions
          .filter((s) => p.cwds.includes(s.row.cwd))
          .flatMap((s) =>
            s.events.flatMap((e) => {
              const payload = e.payload as { event_type: string; data: Record<string, unknown> };
              const text = payload.data.text ?? payload.data.delta;
              return typeof text === 'string' && text.toLowerCase().includes(p.query.toLowerCase())
                ? [
                    {
                      runDir: s.row.runDir,
                      role: payload.event_type === 'user_message_submitted' ? 'user' : 'assistant',
                      snippet: text,
                    },
                  ]
                : [];
            }),
          )
          .slice(0, p.limit);
      case 'session.create': {
        const runId = `fixture-created-${++this.serial}`;
        const row: SessionRow = {
          runId,
          runDir: `/fixture/sessions/${runId}`,
          cwd: p.cwd,
          live: true,
          createdAt: '2026-01-01T00:00:00Z',
          catalog: {
            run_id: runId,
            run_name: 'New task',
            status: 'running',
            last_updated_at: null,
            workspace_root: p.cwd,
            profile_preset: null,
            provider_model: null,
            mode_source: 'scenario_fixture',
            is_resumable: true,
            resume_disabled_reason: null,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: null,
          },
        };
        this.scenario.sessions.push({ row, events: [] });
        this.start(runId);
        return { sessionId: runId, runDir: row.runDir, cwd: row.cwd };
      }
      case 'session.continue': {
        const { row } = find();
        row.live = true;
        this.start(row.runId);
        return { sessionId: row.runId, runDir: row.runDir, cwd: row.cwd };
      }
      case 'session.close': {
        const { row } = find();
        this.stop(row.runId);
        row.live = false;
        this.emit(row.runId, { kind: 'exited' });
        return null;
      }
      case 'session.intent': {
        const { row } = find();
        if (!row.live) throw new BackendError('failed', 'Fixture session is not live');
        if (p.intent.kind === 'updateTitle') row.catalog.run_name = p.intent.title;
        this.emit(row.runId, { kind: 'status', text: `Fixture accepted ${p.intent.kind}` });
        return null;
      }
      case 'models.catalog':
        return { current: null, models: [], profiles: [] };
      case 'commands.list':
      case 'files.index':
      case 'providers.list':
      case 'settings.list':
        return [];
      default:
        throw new BackendError('failed', `${method} is not configured in this fixture`);
    }
  }
  private stop(id: string) {
    for (const timer of this.timers.get(id) || []) clearTimeout(timer);
    this.timers.delete(id);
  }
  close() {
    for (const id of this.timers.keys()) this.stop(id);
    this.status = { state: 'closed', reconnectable: true };
    for (const listener of this.statusListeners) listener();
  }
  async reconnect() {
    this.status = { state: 'connected', reconnectable: false };
    for (const listener of this.statusListeners) listener();
  }
}
export async function createBackend(): Promise<Backend> {
  const name = process.env.HARNESS_GUI_FIXTURE;
  if (!name) return new BackendClient();
  if (!/^[a-z0-9-]+$/.test(name)) throw new Error('Invalid fixture scenario name');
  const scenario = (await Bun.file(
    new URL(`../../test/fixtures/scenarios/${name}.json`, import.meta.url),
  ).json()) as Scenario;
  return new FixtureBackend(scenario);
}

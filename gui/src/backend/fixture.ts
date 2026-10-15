import { FixtureExtensions, type ExtensionFixtures } from './fixture-extensions';
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
  extensions?: ExtensionFixtures;
  state?: Record<string, Json>;
};
export class FixtureBackend implements Backend {
  private scenario: Scenario;
  private documents = new Map<string, Json>();
  private listeners = new Set<(notification: Notification) => void>();
  private statusListeners = new Set<() => void>();
  private timers = new Map<string, ReturnType<typeof setTimeout>[]>();
  private pins = new Map<string, string[]>();
  private accessModes = new Map<string, Methods['session.info']['result']['accessMode']>();
  private status: BackendStatus = { state: 'connected', reconnectable: false };
  private serial = 0;
  private extensions: FixtureExtensions;
  constructor(scenario: Scenario) {
    this.scenario = structuredClone(scenario);
    this.extensions = new FixtureExtensions(scenario.extensions);
    this.documents.set('projects', structuredClone(scenario.projects));
    for (const [name, value] of Object.entries(scenario.state ?? {})) {
      this.documents.set(name, structuredClone(value));
    }
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
    const override = this.extensions.values[method];
    if (override !== undefined && !['skills.list', 'memory.list', 'toggles.get'].includes(method)) {
      return structuredClone(override);
    }
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
      case 'sessions.tree': {
        const rows = this.scenario.sessions
          .filter((s) => s.row.cwd === p.cwd)
          .map((s) => ({
            ...s.row.catalog,
            run_dir: s.row.runDir,
            depth: s.row.catalog.parent_session_id ? 1 : 0,
          }));
        return { session_count: rows.length, harness_lineage: rows, root: null, filter: null };
      }
      case 'sessions.importForeign': {
        const handle = (await this.dispatch('session.create', {
          cwd: p.cwd,
        })) as Methods['session.create']['result'];
        const session = this.scenario.sessions.find((s) => s.row.runId === handle.sessionId)!;
        session.row.live = false;
        session.row.catalog.mode_source = 'replay_only';
        session.row.catalog.is_resumable = false;
        session.row.catalog.resume_disabled_reason = 'Imported visible history is replay-only';
        session.row.catalog.status = 'finished';
        session.row.catalog.run_name = 'Imported conversation';
        session.events.push({
          schema_version: 1,
          event_id: `${handle.sessionId}-1`,
          seq: 1,
          run_id: handle.sessionId,
          mono_ms: 0,
          actor: { kind: 'user' },
          payload: {
            event_type: 'user_message_submitted',
            data: { request_id: 'imported-prompt-1', text: 'Review this imported conversation.' },
          },
        });
        return {
          run_id: handle.sessionId,
          run_dir: handle.runDir,
          event_count: session.events.length,
          source_path: p.from,
          format: 'codex_visible_jsonl_v1',
          mode_source: 'replay_only',
        };
      }
      case 'session.info': {
        const { row } = find();
        return {
          sessionId: row.runId,
          accessMode: this.accessModes.get(row.runId) ?? 'supervised',
          runDir: row.runDir,
          cwd: row.cwd,
          profile: row.catalog.profile_preset ?? 'default',
          model: row.catalog.provider_model,
          provider: 'fixture',
          selectionStatus: 'launch_snapshot',
          mcp: [],
          lsp: [],
          integrationStatusReason: 'Fixture has no running integrations.',
          sandbox: { status: 'unknown', enforced: null, reason: 'No sandbox probe in fixture.' },
          goals: {
            supported: false,
            reason: 'Coordinator has no persistent goal intent.',
            guidance: 'Use an ordinary prompt to describe the objective.',
          },
        };
      }
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
        this.accessModes.set(
          runId,
          p.accessMode ??
            this.extensions.values['models.catalog']?.defaultAccessMode ??
            'supervised',
        );
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
        if (p.intent.kind === 'setAccessMode') this.accessModes.set(row.runId, p.intent.mode);
        if (p.intent.kind === 'setYolo')
          this.accessModes.set(row.runId, p.intent.enabled ? 'full' : 'supervised');
        if (p.intent.kind === 'clone' || p.intent.kind === 'fork') {
          const source = find();
          const runId = `fixture-fork-${++this.serial}`;
          const clone = structuredClone(source);
          clone.row = {
            ...clone.row,
            runId,
            runDir: `/fixture/sessions/${runId}`,
            live: false,
            catalog: { ...clone.row.catalog, run_id: runId, parent_session_id: row.runId },
          };
          let draft = '';
          if (p.intent.kind === 'fork' && p.intent.promptRequestId) {
            const index = clone.events.findIndex(
              (e) =>
                e.payload.event_type === 'user_message_submitted' &&
                e.payload.data.request_id === p.intent.promptRequestId,
            );
            if (index < 0) throw new BackendError('not_found', 'Prompt not found');
            const prompt = clone.events[index]!;
            if (prompt.payload.event_type === 'user_message_submitted')
              draft = prompt.payload.data.text;
            clone.events = clone.events.slice(0, index);
          }
          this.scenario.sessions.push(clone);
          this.emit(row.runId, { kind: 'forked', runId, runDir: clone.row.runDir, draft });
          return null;
        }
        this.emit(row.runId, { kind: 'status', text: `Fixture accepted ${p.intent.kind}` });
        return null;
      }
      case 'auth.start': {
        const attempt = ++this.serial;
        const timer = setTimeout(() => {
          for (const listener of this.listeners) {
            listener({
              method: 'auth.update',
              params: {
                attempt,
                kind: 'result',
                success: true,
                message: 'Fixture authentication completed. No real credentials were stored.',
              },
            });
          }
        }, 100);
        this.timers.set(`auth-${attempt}`, [timer]);
        return { attempt };
      }
      case 'auth.input':
      case 'auth.logout':
        return null;
      case 'settings.write':
      case 'settings.reset': {
        const settings = this.extensions.values['settings.list'] ?? [];
        const setting = settings.find((s) => s.id === p.id);
        if (!setting) throw new BackendError('not_found', 'Fixture setting not found');
        setting.value =
          method === 'settings.reset'
            ? structuredClone(setting.defaultValue)
            : structuredClone(p.value);
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
        return this.extensions.dispatch(method, p);
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

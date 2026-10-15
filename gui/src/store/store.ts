import type { Backend, BackendStatus } from '../backend/client';
import type {
  Command,
  FileEntry,
  Initialize,
  Intent,
  Json,
  Methods,
  ModelCatalog,
  SessionHandle,
  SessionRow,
  Update,
} from '../backend/protocol';
import { applyUpdate, emptySessionView, projectEvents, type SessionView } from '../model/session';

export type Project = { id: string; name: string; cwd: string };
export type StoreState = {
  backend: BackendStatus;
  initialized?: Initialize;
  error?: string;
  projects: Project[];
  sessions: Record<string, SessionRow[]>;
  selectedProject?: string;
  selectedTask?: string;
  views: Record<string, SessionView>;
  live: Record<string, SessionHandle>;
  models: Record<string, ModelCatalog>;
  commands: Record<string, Command[]>;
  files: Record<string, FileEntry[]>;
  preferences: Record<string, Json>;
  layout: Record<string, Json>;
};
export class AppStore {
  private state: StoreState;
  private listeners = new Set<() => void>();
  private unsubscribers: (() => void)[];
  private loading = new Map<string, Promise<void>>();
  private loadingUpdates = new Map<string, Update[]>();
  private loaded = new Set<string>();
  private saved = new Map<string, string>();
  private pendingWrites = new Map<string, Json>();
  private writeTimer?: Timer;
  private writing?: Promise<void>;
  constructor(readonly backend: Backend) {
    this.state = {
      backend: backend.getStatus(),
      projects: [],
      sessions: {},
      views: {},
      live: {},
      models: {},
      commands: {},
      files: {},
      preferences: {},
      layout: {},
    };
    this.unsubscribers = [
      backend.subscribeStatus(() => {
        const status = backend.getStatus();
        this.patch({ backend: status, ...(status.state !== 'connected' ? { live: {} } : {}) });
      }),
      backend.subscribe((notification) => {
        if (notification.method !== 'session.update') return;
        const { sessionId, update } = notification.params;
        if (!this.loaded.has(sessionId)) {
          const buffered = this.loadingUpdates.get(sessionId) || [];
          buffered.push(update);
          this.loadingUpdates.set(sessionId, buffered);
        }
        const previous =
          this.state.views[sessionId] || emptySessionView(this.row(sessionId)?.catalog);
        const views = { ...this.state.views, [sessionId]: applyUpdate(previous, update) };
        let live = this.state.live;
        if (update.kind === 'exited') {
          live = { ...live };
          delete live[sessionId];
        }
        let sessions = this.state.sessions;
        if (update.kind === 'exited')
          sessions = Object.fromEntries(
            Object.entries(sessions).map(([cwd, rows]) => [
              cwd,
              rows.map((row) => (row.runId === sessionId ? { ...row, live: false } : row)),
            ]),
          );
        if (update.kind === 'event' && update.delivery === 'durable') {
          const payload = update.event.payload;
          const catalog =
            payload.event_type === 'session_title_updated'
              ? { run_name: payload.data.title }
              : payload.event_type === 'run_finished'
                ? { status: 'finished' as const }
                : payload.event_type === 'run_failed'
                  ? { status: 'failed' as const }
                  : undefined;
          if (catalog)
            sessions = Object.fromEntries(
              Object.entries(sessions).map(([cwd, rows]) => [
                cwd,
                rows.map((row) =>
                  row.runId === sessionId
                    ? { ...row, catalog: { ...row.catalog, ...catalog } }
                    : row,
                ),
              ]),
            );
        }
        this.patch({ views, live, sessions });
      }),
    ];
  }
  getSnapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private patch(patch: Partial<StoreState>) {
    this.state = { ...this.state, ...patch };
    for (const listener of this.listeners) listener();
  }
  private row(id: string) {
    return Object.values(this.state.sessions)
      .flat()
      .find((row) => row.runId === id);
  }
  async initialize(cwd?: string) {
    try {
      const [initialized, projects, preferences, layout] = await Promise.all([
        this.backend.call('app.initialize', { cwd }),
        this.backend.call('state.read', { name: 'projects' }),
        this.backend.call('state.read', { name: 'preferences' }),
        this.backend.call('state.read', { name: 'layout' }),
      ]);
      const projectList = (projects ?? []) as Project[];
      const prefs = (preferences ?? {}) as Record<string, Json>;
      const ui = (layout ?? {}) as Record<string, Json>;
      for (const [name, value] of [
        ['projects', projectList],
        ['preferences', prefs],
        ['layout', ui],
      ] as const)
        this.saved.set(name, JSON.stringify(value));
      this.patch({
        initialized,
        projects: projectList,
        preferences: prefs,
        layout: ui,
        selectedProject:
          typeof ui.selectedProject === 'string' ? ui.selectedProject : projectList[0]?.id,
        selectedTask: typeof ui.selectedTask === 'string' ? ui.selectedTask : undefined,
        error: undefined,
      });
      await Promise.all(projectList.map((project) => this.loadSessions(project.cwd)));
    } catch (error) {
      this.patch({ error: error instanceof Error ? error.message : String(error) });
      throw error;
    }
  }
  setProjects(projects: Project[]) {
    this.patch({ projects });
    this.persist('projects', projects);
  }
  selectProject(id: string | undefined) {
    this.patch({ selectedProject: id, selectedTask: undefined });
    this.setLayout({ selectedProject: id ?? null, selectedTask: null });
  }
  async selectTask(id: string | undefined) {
    this.patch({ selectedTask: id });
    this.setLayout({ selectedTask: id ?? null });
    if (id) await this.loadSession(id);
  }
  setPreferences(patch: Record<string, Json>) {
    const preferences = { ...this.state.preferences, ...patch };
    this.patch({ preferences });
    this.persist('preferences', preferences);
  }
  setLayout(patch: Record<string, Json>) {
    const layout = { ...this.state.layout, ...patch };
    this.patch({ layout });
    this.persist('layout', layout);
  }
  async loadSessions(cwd: string) {
    const rows = await this.backend.call('sessions.list', { cwd });
    this.patch({ sessions: { ...this.state.sessions, [cwd]: rows } });
    return rows;
  }
  async loadSession(id: string): Promise<void> {
    const loading = this.loading.get(id);
    if (loading) return loading;
    if (this.loaded.has(id)) return;
    const row = this.row(id);
    if (!row) throw new Error(`Unknown session ${id}`);
    if (!this.loadingUpdates.has(id)) this.loadingUpdates.set(id, []);
    const promise = (async () => {
      try {
        const events = await this.backend.call('sessions.events', { runDir: row.runDir });
        let view = events.length ? projectEvents(events) : emptySessionView(row.catalog);
        for (const update of this.loadingUpdates.get(id) || []) view = applyUpdate(view, update);
        this.loaded.add(id);
        this.loadingUpdates.delete(id);
        this.patch({ views: { ...this.state.views, [id]: view } });
      } finally {
        this.loading.delete(id);
      }
    })();
    this.loading.set(id, promise);
    return promise;
  }
  async createSession(params: Methods['session.create']['params']) {
    const handle = await this.backend.call('session.create', params);
    this.patch({
      live: { ...this.state.live, [handle.sessionId]: handle },
      selectedTask: handle.sessionId,
    });
    this.setLayout({ selectedTask: handle.sessionId });
    await this.loadSessions(handle.cwd);
    await this.loadSession(handle.sessionId);
    return handle;
  }
  async continueSession(id: string) {
    if (this.state.live[id]) return this.state.live[id]!;
    const row = this.row(id);
    if (!row) throw new Error(`Unknown session ${id}`);
    await this.loadSession(id);
    const handle = await this.backend.call('session.continue', { runDir: row.runDir });
    this.patch({ live: { ...this.state.live, [id]: handle } });
    await this.loadSessions(handle.cwd);
    return handle;
  }
  async closeSession(id: string) {
    await this.backend.call('session.close', { sessionId: id });
    const live = { ...this.state.live };
    delete live[id];
    this.patch({ live });
    const row = this.row(id);
    if (row) await this.loadSessions(row.cwd);
  }
  async sendIntent(id: string, intent: Intent) {
    if (!this.state.live[id]) await this.continueSession(id);
    await this.backend.call('session.intent', { sessionId: id, intent });
  }
  async loadModels(cwd: string) {
    if (this.state.models[cwd]) return this.state.models[cwd]!;
    const value = await this.backend.call('models.catalog', { cwd });
    this.patch({ models: { ...this.state.models, [cwd]: value } });
    return value;
  }
  async loadCommands(cwd: string) {
    if (this.state.commands[cwd]) return this.state.commands[cwd]!;
    const value = await this.backend.call('commands.list', { cwd });
    this.patch({ commands: { ...this.state.commands, [cwd]: value } });
    return value;
  }
  async loadFiles(cwd: string) {
    if (this.state.files[cwd]) return this.state.files[cwd]!;
    const value = await this.backend.call('files.index', { cwd });
    this.patch({ files: { ...this.state.files, [cwd]: value } });
    return value;
  }
  invalidate(cwd: string) {
    const models = { ...this.state.models },
      commands = { ...this.state.commands },
      files = { ...this.state.files };
    delete models[cwd];
    delete commands[cwd];
    delete files[cwd];
    this.patch({ models, commands, files });
  }
  private persist(name: string, value: unknown) {
    const json = JSON.stringify(value);
    if (json === this.saved.get(name)) {
      this.pendingWrites.delete(name);
      return;
    }
    this.pendingWrites.set(name, JSON.parse(json) as Json);
    clearTimeout(this.writeTimer);
    this.writeTimer = setTimeout(() => {
      void this.flushPersistence().catch((error) => this.patch({ error: String(error) }));
    }, 150);
  }
  async flushPersistence(): Promise<void> {
    clearTimeout(this.writeTimer);
    if (this.writing) await this.writing;
    this.writing = (async () => {
      for (const [name, value] of this.pendingWrites) {
        this.pendingWrites.delete(name);
        try {
          await this.backend.call('state.write', { name, value });
          this.saved.set(name, JSON.stringify(value));
        } catch (error) {
          if (!this.pendingWrites.has(name)) this.pendingWrites.set(name, value);
          throw error;
        }
      }
    })();
    try {
      await this.writing;
    } finally {
      this.writing = undefined;
    }
  }
  async dispose() {
    try {
      await this.flushPersistence();
    } finally {
      for (const unsubscribe of this.unsubscribers) unsubscribe();
      this.backend.close();
    }
  }
}

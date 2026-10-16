import type { Backend, BackendStatus } from '../backend/client';
import { runBeforeSubmit as runSubmitHooks } from '../app/turn-hooks';
import type {
  AccessMode,
  Attachment,
  Command,
  FileEntry,
  Initialize,
  Intent,
  Json,
  Methods,
  ModelCatalog,
  ModelSelection,
  SessionHandle,
  SessionRow,
  Update,
} from '../backend/protocol';
import {
  applyUpdate,
  emptySessionView,
  projectEvents,
  type RunStatus,
  type SessionView,
} from '../model/session';

export type Project = { id: string; name: string; cwd: string };
export type Selection =
  | { kind: 'project'; projectId: string }
  | { kind: 'session'; sessionId: string; projectId: string }
  | { kind: 'provisional'; projectId?: string };
export type DraftAttachment = Attachment & {
  name?: string;
  path?: string;
  directory?: boolean;
  digest?: string;
};
export type Draft = {
  text: string;
  attachments?: DraftAttachment[];
  fileTags?: Extract<Intent, { kind: 'submit' | 'followUp' }>['fileTags'];
  agentTags?: Extract<Intent, { kind: 'submit' | 'followUp' }>['agentTags'];
  resourceTags?: Extract<Intent, { kind: 'submit' | 'followUp' }>['resourceTags'];
  selection?: ModelSelection;
  accessMode?: AccessMode;
  worktree?: { name?: string; baseBranch?: string };
};
export type SubmitDraft = Draft & {
  mode?: 'submit' | 'followUp' | 'steer';
  consumeDraft?: boolean;
  runBeforeSubmit?: (context: { sessionId: string; cwd: string }) => void | Promise<void>;
};
export function draftKey(target: Selection): string {
  return target.kind === 'session'
    ? `session:${target.sessionId}`
    : `project:${target.projectId ?? ''}`;
}
export type StoreState = {
  backend: BackendStatus;
  initialized?: Initialize;
  error?: string;
  projects: Project[];
  sessions: Record<string, SessionRow[]>;
  selectedProject?: string;
  selectedTask?: string;
  selection: Selection;
  provisionalSelection?: Selection;
  drafts: Record<string, Draft>;
  pins: Record<string, string[]>;
  projectModels: Record<string, ModelSelection>;
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
  private draftTimer?: Timer;
  private draftGenerations = new Map<string, number>();
  private selectionGeneration = 0;
  private submissions = new Map<string, Promise<string>>();
  private removed = new Set<string>();
  private draftSaving?: Promise<void>;
  constructor(readonly backend: Backend) {
    this.state = {
      backend: backend.getStatus(),
      projects: [],
      sessions: {},
      selection: { kind: 'provisional' },
      drafts: {},
      pins: {},
      projectModels: {},
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
        if (this.removed.has(sessionId)) return;
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
      const [initialized, projects, preferences, layout, drafts] = await Promise.all([
        this.backend.call('app.initialize', { cwd }),
        this.backend.call('state.read', { name: 'projects' }),
        this.backend.call('state.read', { name: 'preferences' }),
        this.backend.call('state.read', { name: 'layout' }),
        this.backend.call('state.read', { name: 'drafts' }),
      ]);
      const projectList = (projects ?? []) as Project[];
      const prefs = (preferences ?? {}) as Record<string, Json>;
      const ui = (layout ?? {}) as Record<string, Json>;
      const restoredDrafts = structuredClone((drafts ?? {}) as Record<string, Draft>);
      await Promise.all(
        Object.values(restoredDrafts).map(async (draft) => {
          if (!draft.attachments) return;
          draft.attachments = await Promise.all(
            draft.attachments.map(async (attachment) => {
              if (!attachment.digest || attachment.dataBase64) return attachment;
              const blob = await this.backend.call('blobs.get', { digest: attachment.digest });
              if (!blob) throw new Error(`Missing draft attachment ${attachment.digest}`);
              return { ...attachment, ...blob };
            }),
          );
        }),
      );
      for (const [name, value] of [
        ['projects', projectList],
        ['preferences', prefs],
        ['layout', ui],
        ['drafts', drafts ?? {}],
      ] as const)
        this.saved.set(name, JSON.stringify(value));
      this.patch({
        initialized,
        projects: projectList,
        preferences: prefs,
        layout: ui,
        drafts: restoredDrafts,
        projectModels: (prefs.projectModels ?? {}) as Record<string, ModelSelection>,
        selectedProject: typeof ui.selectedProject === 'string' ? ui.selectedProject : undefined,
        error: undefined,
      });
      await Promise.all(projectList.map((project) => this.loadSessions(project.cwd)));
      const projectId =
        typeof ui.selectedProject === 'string' &&
        projectList.some((p) => p.id === ui.selectedProject)
          ? ui.selectedProject
          : undefined;
      this.commitSelection(
        projectId ? { kind: 'project', projectId } : { kind: 'provisional' },
        false,
      );
      if (typeof ui.selectedTask === 'string' && this.row(ui.selectedTask))
        await this.selectTask(ui.selectedTask);
    } catch (error) {
      this.patch({ error: error instanceof Error ? error.message : String(error) });
      throw error;
    }
  }
  setProjects(projects: Project[]) {
    this.patch({ projects });
    this.persist('projects', projects);
  }
  reportError(error: string) {
    this.patch({ error });
  }
  async flush() {
    await this.flushDrafts();
  }
  addProject(project: Project) {
    const existing = this.state.projects.find(
      (value) => value.id === project.id || value.cwd === project.cwd,
    );
    if (!existing) this.setProjects([...this.state.projects, project]);
    this.selectProject(existing?.id ?? project.id);
  }
  newTask(projectId?: string) {
    this.selectProject(projectId);
  }
  selectProjectless() {
    this.selectProject(undefined);
    this.setLayout({ projectless: true });
  }
  private commitSelection(selection: Selection, persist = true) {
    this.patch({
      selection,
      provisionalSelection: undefined,
      selectedProject: selection.projectId,
      selectedTask: selection.kind === 'session' ? selection.sessionId : undefined,
    });
    if (persist)
      this.setLayout({
        selectedProject: selection.projectId ?? null,
        projectless: selection.projectId
          ? (this.state.projects
              .find((project) => project.id === selection.projectId)
              ?.cwd.startsWith(`${this.state.initialized?.home}/scratch/`) ?? false)
          : this.state.layout.projectless === true,
        selectedTask: selection.kind === 'session' ? selection.sessionId : null,
      });
  }
  selectProject(id: string | undefined) {
    this.selectionGeneration++;
    void this.flushDrafts().catch((error) => this.patch({ error: String(error) }));
    this.commitSelection(id ? { kind: 'project', projectId: id } : { kind: 'provisional' });
  }
  async selectTask(id: string | undefined) {
    if (!id) {
      this.selectProject(this.state.selectedProject);
      return;
    }
    const row = this.row(id);
    if (!row) throw new Error(`Unknown session ${id}`);
    const projectId = this.state.projects.find((project) => project.cwd === row.cwd)?.id ?? row.cwd;
    const target: Selection = { kind: 'session', sessionId: id, projectId };
    const generation = ++this.selectionGeneration;
    this.patch({ provisionalSelection: target });
    try {
      await this.flushDrafts();
      await this.loadSession(id);
      if (generation === this.selectionGeneration) this.commitSelection(target);
    } catch (error) {
      if (generation === this.selectionGeneration)
        this.patch({
          provisionalSelection: undefined,
          error: `Could not open that session: ${String(error)}`,
        });
      throw error;
    }
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
  setProjectModel(projectId: string, selection: ModelSelection) {
    const projectModels = { ...this.state.projectModels, [projectId]: selection };
    this.patch({ projectModels });
    this.setPreferences({ projectModels: JSON.parse(JSON.stringify(projectModels)) as Json });
  }
  getDraft(target: Selection): Draft | undefined {
    return this.state.drafts[draftKey(target)];
  }
  setDraft(target: Selection, draft: Draft) {
    const key = draftKey(target);
    this.draftGenerations.set(key, (this.draftGenerations.get(key) ?? 0) + 1);
    this.patch({ drafts: { ...this.state.drafts, [key]: structuredClone(draft) } });
    this.scheduleDraftSave();
  }
  consumeDraft(target: Selection): Draft | undefined {
    const key = draftKey(target);
    const draft = this.state.drafts[key];
    this.draftGenerations.set(key, (this.draftGenerations.get(key) ?? 0) + 1);
    const drafts = { ...this.state.drafts };
    delete drafts[key];
    this.patch({ drafts });
    this.scheduleDraftSave();
    return draft;
  }
  moveDraft(from: Selection, to: Selection): boolean {
    if (draftKey(from) === draftKey(to)) return false;
    const draft = this.getDraft(from);
    const destination = this.getDraft(to);
    if (!draft || destination?.text.trim() || destination?.attachments?.length) return false;
    this.setDraft(to, draft);
    this.consumeDraft(from);
    return true;
  }
  private scheduleDraftSave() {
    clearTimeout(this.draftTimer);
    this.draftTimer = setTimeout(() => {
      void this.flushDrafts().catch((error) => this.patch({ error: String(error) }));
    }, 250);
  }
  async flushDrafts() {
    clearTimeout(this.draftTimer);
    while (this.draftSaving) await this.draftSaving;
    this.draftSaving = this.saveDrafts();
    try {
      await this.draftSaving;
    } finally {
      this.draftSaving = undefined;
    }
  }
  private async storeAttachment(attachment: DraftAttachment): Promise<DraftAttachment> {
    if (!attachment.mime.toLowerCase().startsWith('image/') || attachment.directory)
      return attachment;
    if (!attachment.dataBase64 && attachment.digest) {
      const blob = await this.backend.call('blobs.get', { digest: attachment.digest });
      if (!blob) throw new Error(`Missing attachment ${attachment.digest}`);
      return { ...attachment, ...blob };
    }
    if (attachment.digest || !attachment.dataBase64) return attachment;
    const { digest } = await this.backend.call('blobs.put', {
      mime: attachment.mime,
      dataBase64: attachment.dataBase64,
    });
    return { ...attachment, digest };
  }
  private async saveDrafts() {
    for (;;) {
      const snapshot = this.state.drafts;
      const prepared = Object.fromEntries(
        await Promise.all(
          Object.entries(snapshot).map(
            async ([key, draft]) =>
              [
                key,
                {
                  ...draft,
                  attachments: draft.attachments
                    ? await Promise.all(
                        draft.attachments.map((attachment) => this.storeAttachment(attachment)),
                      )
                    : undefined,
                },
              ] as const,
          ),
        ),
      );
      if (this.state.drafts !== snapshot) continue;
      this.patch({ drafts: prepared });
      const serialized = Object.fromEntries(
        Object.entries(prepared).map(([key, draft]) => [
          key,
          {
            ...draft,
            attachments: draft.attachments?.map((attachment) => {
              if (!attachment.digest) return attachment;
              const { dataBase64: _bytes, ...metadata } = attachment;
              return metadata;
            }),
          },
        ]),
      );
      this.persist('drafts', serialized);
      await this.flushPersistence();
      if (this.state.drafts === prepared) return;
    }
  }
  sessionStatus(id: string): RunStatus {
    const view = this.state.views[id];
    if (view) return view.status;
    const row = this.row(id);
    if (row?.catalog.status === 'failed') return 'failed';
    return row?.live && row.catalog.status === 'running' ? 'working' : 'idle';
  }
  async renameTask(id: string, title: string) {
    const row = this.row(id);
    if (!row) throw new Error(`Unknown session ${id}`);
    await this.backend.call('sessions.rename', { runDir: row.runDir, title });
    const view = this.state.views[id];
    if (view) this.patch({ views: { ...this.state.views, [id]: { ...view, title } } });
    await this.loadSessions(row.cwd);
  }
  async pinTask(id: string, pinned: boolean) {
    const row = this.row(id);
    if (!row) throw new Error(`Unknown session ${id}`);
    await this.backend.call('sessions.setPinned', { cwd: row.cwd, runId: id, pinned });
    const pins = this.state.pins[row.cwd] ?? [];
    this.patch({
      pins: {
        ...this.state.pins,
        [row.cwd]: pinned
          ? [...pins.filter((value) => value !== id), id]
          : pins.filter((value) => value !== id),
      },
    });
  }
  async removeTask(id: string) {
    const row = this.row(id);
    if (!row) throw new Error(`Unknown session ${id}`);
    if (this.state.live[id] || row.live) await this.closeSession(id);
    if (this.state.pins[row.cwd]?.includes(id)) await this.pinTask(id, false);
    await this.backend.call('sessions.delete', { runDir: row.runDir });
    const selected =
      this.state.selectedTask === id ||
      (this.state.provisionalSelection?.kind === 'session' &&
        this.state.provisionalSelection.sessionId === id);
    this.removed.add(id);
    if (selected) this.selectionGeneration++;
    const projectId = this.state.projects.find((project) => project.cwd === row.cwd)?.id ?? row.cwd;
    this.consumeDraft({ kind: 'session', sessionId: id, projectId });
    this.loaded.delete(id);
    this.loadingUpdates.delete(id);
    const views = { ...this.state.views },
      live = { ...this.state.live };
    delete views[id];
    delete live[id];
    this.patch({ views, live });
    const rows = await this.loadSessions(row.cwd);
    if (selected) {
      const next = [...rows].sort((a, b) =>
        (b.catalog.last_updated_at ?? b.createdAt ?? '').localeCompare(
          a.catalog.last_updated_at ?? a.createdAt ?? '',
        ),
      )[0];
      if (next) await this.selectTask(next.runId);
      else this.selectProject(projectId);
    }
    await this.flushDrafts();
  }
  submit(target: Selection, draft: SubmitDraft): Promise<string> {
    const key = draftKey(target);
    const pending = this.submissions.get(key);
    if (pending) return pending;
    const operation = this.submitDraft(target, draft).finally(() => this.submissions.delete(key));
    this.submissions.set(key, operation);
    return operation;
  }
  private async submitDraft(target: Selection, draft: SubmitDraft): Promise<string> {
    if (!draft.text.trim() && !draft.attachments?.length)
      throw new Error('Cannot submit an empty draft');
    const key = draftKey(target);
    if (
      target.kind !== 'session' &&
      target.projectId &&
      !this.state.projects.some((project) => project.id === target.projectId)
    )
      throw new Error(`Unknown project ${target.projectId}`);
    const { runBeforeSubmit, mode, consumeDraft = true, ...submittedDraft } = draft;
    const previous = consumeDraft ? (this.consumeDraft(target) ?? submittedDraft) : submittedDraft;
    const generation = this.draftGenerations.get(key);
    let sessionId = target.kind === 'session' ? target.sessionId : undefined;
    try {
      await this.flushDrafts();
      let project = this.state.projects.find((value) => value.id === target.projectId);
      if (!sessionId) {
        if (!project) {
          const scratch = await this.backend.call('projects.scratch', { prompt: draft.text });
          project = {
            id: scratch.cwd,
            cwd: scratch.cwd,
            name: scratch.cwd.split('/').pop() || 'Scratch',
          };
          this.setProjects([...this.state.projects, project]);
        }
        const selection = draft.selection ?? this.state.projectModels[project.id];
        const handle = await this.createSession({
          cwd: project.cwd,
          selection,
          accessMode: draft.accessMode,
          worktree: draft.worktree,
        });
        sessionId = handle.sessionId;
        if (selection) this.setProjectModel(project.id, selection);
      } else {
        await this.continueSession(sessionId);
        if (draft.selection) {
          await this.sendIntent(sessionId, { kind: 'switchModel', selection: draft.selection });
          if (target.projectId) this.setProjectModel(target.projectId, draft.selection);
        }
        if (draft.accessMode)
          await this.sendIntent(sessionId, { kind: 'setAccessMode', mode: draft.accessMode });
      }
      const cwd = this.state.live[sessionId]?.cwd ?? this.row(sessionId)?.cwd;
      if (!cwd) throw new Error(`Unknown workspace for session ${sessionId}`);
      await runSubmitHooks({ sessionId, cwd }, (error) => this.reportError(String(error)));
      await runBeforeSubmit?.({ sessionId, cwd });
      const status = this.sessionStatus(sessionId);
      const kind =
        mode ??
        (status === 'background'
          ? 'steer'
          : status === 'working' || status === 'waiting'
            ? 'followUp'
            : 'submit');
      const storedAttachments = draft.attachments
        ? await Promise.all(draft.attachments.map((attachment) => this.storeAttachment(attachment)))
        : undefined;
      const attachments = storedAttachments
        ?.filter((attachment) => !attachment.directory)
        .map(({ mime, dataBase64 }) => ({ mime, dataBase64 }));
      const intent: Intent =
        kind === 'steer'
          ? { kind, text: draft.text, attachments }
          : {
              kind,
              text: draft.text,
              attachments,
              fileTags: draft.fileTags,
              agentTags: draft.agentTags,
              resourceTags: draft.resourceTags,
            };
      await this.sendIntent(sessionId, intent);
      return sessionId;
    } catch (error) {
      if (consumeDraft && this.draftGenerations.get(key) === generation) {
        const destination: Selection =
          sessionId && target.kind !== 'session'
            ? {
                kind: 'session',
                sessionId,
                projectId: this.state.selectedProject ?? target.projectId ?? '',
              }
            : target;
        if (draftKey(destination) === key || !this.getDraft(destination))
          this.setDraft(destination, previous);
      }
      this.patch({ error: error instanceof Error ? error.message : String(error) });
      throw error;
    }
  }
  async loadSessions(cwd: string) {
    const [rows, pins] = await Promise.all([
      this.backend.call('sessions.list', { cwd }),
      this.backend.call('sessions.pins', { cwd }),
    ]);
    this.patch({
      sessions: { ...this.state.sessions, [cwd]: rows },
      pins: { ...this.state.pins, [cwd]: pins },
    });
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
        if (this.removed.has(id)) return;
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
      views: { ...this.state.views, [handle.sessionId]: emptySessionView() },
    });
    const projectId =
      this.state.projects.find((project) => project.cwd === handle.cwd)?.id ?? handle.cwd;
    this.selectionGeneration++;
    this.commitSelection({ kind: 'session', sessionId: handle.sessionId, projectId });
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
  async loadModels(cwd: string, refresh = false) {
    if (!refresh && this.state.models[cwd]) return this.state.models[cwd]!;
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
    if (!this.writing && json === this.saved.get(name)) {
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
    while (this.writing) await this.writing;
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
      await this.flushDrafts();
    } finally {
      for (const unsubscribe of this.unsubscribers) unsubscribe();
      this.backend.close();
    }
  }
}

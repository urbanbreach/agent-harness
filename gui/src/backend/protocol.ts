import type {
  Skill,
  UsageHistory,
  SubscriptionUsage,
  MemoryEntry,
  MemoryList,
  PlanEntry,
  ForeignDiscovery,
  ForeignImport,
  SessionTree,
} from './extensions';
export type {
  Skill,
  SkillInstall,
  UsageAggregate,
  UsageHistory,
  SubscriptionUsage,
  MemoryEntry,
  MemoryList,
  PlanEntry,
  ForeignCandidate,
  ForeignDiscovery,
  ForeignImport,
  SessionTree,
} from './extensions';
import type {
  ToggleState,
  TrustState,
  DoctorReport,
  ConfigSources,
  SessionInfo,
} from './extensions';
export type {
  ToggleState,
  TrustState,
  DoctorReport,
  ConfigSources,
  SessionInfo,
} from './extensions';
import type { EventEnvelopeV1, LiveEventEnvelope } from '../model/harness-events';
export type { EventEnvelopeV1, LiveEventEnvelope } from '../model/harness-events';

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type SessionCatalogEntry = {
  run_id: string;
  run_name: string | null;
  status: 'running' | 'finished' | 'failed' | null;
  last_updated_at: string | null;
  workspace_root: string | null;
  profile_preset: string | null;
  provider_model: string | null;
  mode_source:
    | 'interactive_live'
    | 'interactive_mock'
    | 'prompt'
    | 'scenario_fixture'
    | 'replay_only'
    | 'unknown';
  is_resumable: boolean;
  resume_disabled_reason: string | null;
  artifact_count: number;
  child_session_count: number;
  parent_session_id: string | null;
};
export type SessionRow = {
  runId: string;
  runDir: string;
  cwd: string;
  projectCwd?: string;
  branch?: string | null;
  worktree?: boolean;
  catalog: SessionCatalogEntry;
  createdAt?: string;
  live: boolean;
};
export type ModelSelection = { profile: string; model?: string; variant?: string };
export type AccessMode = 'supervised' | 'autoEdits' | 'auto' | 'full';
export type FileTagSource = { start: number; end: number; value: string };
export type SelectedFileTag = {
  path: string;
  filename: string;
  url: string;
  mime: string;
  source: FileTagSource;
  line_range?: { start: number; end?: number };
};
export type SelectedAgentTag = { name: string; source: FileTagSource };
export type SelectedResourceTag = {
  name: string;
  uri: string;
  mime: string;
  source: FileTagSource;
  description?: string;
};
export type Attachment = { mime: string; dataBase64: string };
export type Intent =
  | {
      kind: 'submit' | 'followUp';
      text: string;
      fileTags?: SelectedFileTag[];
      agentTags?: SelectedAgentTag[];
      resourceTags?: SelectedResourceTag[];
      attachments?: Attachment[];
    }
  | { kind: 'steer'; text: string; attachments?: Attachment[] }
  | { kind: 'queued'; id: number; prompt: Intent }
  | { kind: 'withdrawQueued' | 'withdrawAccepted' | 'backgroundForeground' | 'clone' }
  | { kind: 'interrupt'; taskIds: string[]; reason: 'user' | 'replace' | 'task' | 'sendNow' }
  | {
      kind: 'resolvePermission';
      permissionId: string;
      decision: 'allow' | 'deny';
      reason?: string;
      grantScope?: 'once' | 'session';
    }
  | { kind: 'setYolo'; enabled: boolean }
  | { kind: 'setAccessMode'; mode: AccessMode }
  | { kind: 'switchModel'; selection: ModelSelection }
  | { kind: 'compact'; instructions?: string }
  | { kind: 'handoff'; focus?: string }
  | { kind: 'shake'; args?: string }
  | { kind: 'cancelCompaction'; agentId: string }
  | { kind: 'demoteChild'; handleId: string }
  | { kind: 'cancelSubagent'; sessionId: string }
  | { kind: 'inspectCommand'; taskId?: string }
  | { kind: 'loadRewindPoints'; generation: number; cancelTaskIds: string[] }
  | { kind: 'rewind'; generation: number; requestId: string }
  | { kind: 'revertWorkspace'; snapshotRequestId: string }
  | { kind: 'updateTitle'; title: string }
  | { kind: 'fork'; promptRequestId?: string }
  | { kind: 'runShell'; command: string };
export type RewindPoint = { seq: number; request_id: string; text: string };
export type Update =
  | { kind: 'event'; delivery: 'durable'; event: EventEnvelopeV1 }
  | { kind: 'event'; delivery: 'live'; event: LiveEventEnvelope }
  | { kind: 'status'; text: string }
  | { kind: 'notice'; level: 'info' | 'error'; message: string }
  | { kind: 'modelNotice'; message: string }
  | { kind: 'yolo'; enabled: boolean }
  | { kind: 'yoloFailed' }
  | {
      kind: 'compaction';
      command: 'compact' | 'handoff' | 'shake' | 'idle';
      outcome: 'done' | 'warning' | 'cancelled' | 'failed';
      message?: string;
      tokensAfter?: number;
      savedPath?: string;
    }
  | { kind: 'commandOutput'; snapshot: unknown }
  | { kind: 'rewindPoints'; generation: number; points?: RewindPoint[]; error?: string }
  | { kind: 'rewindComplete'; generation: number; point?: RewindPoint; error?: string }
  | { kind: 'restoreQueued'; texts: string[] }
  | { kind: 'queuedReceipt'; id: number; error?: string }
  | { kind: 'forked'; runId: string; runDir: string; draft: string }
  | { kind: 'exited'; error?: string };
export type Initialize = {
  version: string;
  home: string;
  guiStateDir: string;
  defaultCwd: string;
  mock: boolean;
};
export type SessionHandle = { sessionId: string; runDir: string; cwd: string };
export type ModelEntry = {
  profile: string;
  provider: string;
  providerLabel?: string;
  model: string;
  modelLabel?: string;
  variant?: string;
  variantLabel?: string;
  label: string;
  reasoningEffort?: string;
  contextWindow?: number;
  description?: string;
};
export type ModelCatalog = {
  current: ModelEntry | null;
  models: ModelEntry[];
  profiles: string[];
  defaultAccessMode?: AccessMode;
};
export type Command = {
  name: string;
  description: string;
  argumentHint?: string;
  scope: 'builtin' | 'project' | 'user';
  template?: string;
};
export type Worktree = {
  path: string;
  branch: string | null;
  head: string | null;
  bare: boolean;
  detached: boolean;
  harness_managed: boolean;
  slug: string | null;
};
export type WorktreeList = {
  repository_root: string;
  managed_count: number;
  worktrees: Worktree[];
};
export type IndexRebuild = {
  entry_count: number;
  journals_scanned: number;
  journals_opened: number;
  index_path: string;
  skipped_changing_journals: number;
};
export type CrashScan = {
  scan_root: string;
  summary: {
    scanned: number;
    previous_crash: number;
    clean: number;
    stale_writer_lock: number;
    recovery_marker: number;
  };
  reports: {
    run_dir: string;
    previous_crash_detected: boolean;
    stale_writer_lock: boolean;
    recovery_marker_present: boolean;
    events_log_present: boolean;
    recovery_message: string | null;
    recovery_action: 'resume_with_prompt' | 'reopen_session' | 'open_recovers' | null;
    notes: string[];
  }[];
};
export type ConfigExplanation = {
  schema_version: string;
  path: string;
  redacted: true;
  found: boolean;
  effective: Json;
  source_path: string | null;
  primary_path: string | null;
  layers: { path: string; defines_path: boolean; value: Json }[];
  source_note: string;
};
export type FileEntry = { path: string; dir: boolean };
export type Provider = {
  id: string;
  label: string;
  description: string;
  methods: { id: 'api-key' | 'browser' | 'device'; label: string }[];
  connected: boolean;
};
export type Setting = {
  id: string;
  surface: string;
  label: string;
  description: string;
  kind: string;
  value: Json;
  defaultValue: Json;
  editable: boolean;
  choices?: string[];
};
type Rpc<P, R> = { params: P; result: R };
export type Methods = {
  'app.initialize': Rpc<{ cwd?: string }, Initialize>;
  'blobs.put': Rpc<{ mime: string; dataBase64: string }, { digest: string }>;
  'blobs.get': Rpc<{ digest: string }, { mime: string; dataBase64: string } | null>;
  'projects.scratch': Rpc<{ prompt: string }, { cwd: string }>;
  'llm.complete': Rpc<
    { cwd: string; prompt: string; system?: string; model?: string },
    { text: string; model: string; provider: string }
  >;
  'skills.list': Rpc<{ cwd?: string }, { skills: Skill[]; errors: string[] }>;
  'skills.read': Rpc<{ cwd?: string; id: string }, { id: string; body: string }>;
  'skills.setEnabled': Rpc<
    { cwd?: string; id: string; enabled: boolean },
    { id: string; enabled: boolean; changed: number }
  >;
  'skills.trash': Rpc<{ cwd?: string; id: string }, { id: string; trashed: number }>;
  'usage.history': Rpc<{ cwd?: string; from: string; to: string }, UsageHistory>;
  'usage.subscription': Rpc<{ cwd?: string; provider: string }, SubscriptionUsage>;
  'memory.list': Rpc<{ cwd: string }, MemoryList>;
  'memory.search': Rpc<{ cwd: string; query: string }, MemoryList>;
  'memory.get': Rpc<{ cwd: string; key: string }, MemoryEntry>;
  'memory.delete': Rpc<{ cwd: string; key: string }, { deleted: boolean }>;
  'plans.list': Rpc<{ cwd: string }, { plans: PlanEntry[] }>;
  'plans.read': Rpc<{ cwd: string; path: string }, { path: string; content: string }>;
  'history.prompts': Rpc<{ cwd: string }, { prompts: string[] }>;
  'sessions.discoverForeign': Rpc<{ cwd: string; from: string }, ForeignDiscovery>;
  'sessions.importForeign': Rpc<{ cwd: string; from: string }, ForeignImport>;
  'sessions.exportMarkdown': Rpc<{ runDir: string; dest: string }, { dest: string; bytes: number }>;
  'sessions.trace': Rpc<
    { runDir: string; dest: string },
    { session_id: string; status: 'exported'; local_path: string; bytes: number }
  >;
  'sessions.tree': Rpc<{ cwd: string }, SessionTree>;
  'toggles.get': Rpc<{ cwd: string }, ToggleState>;
  'toggles.set': Rpc<{ cwd: string; id: string; enabled: boolean }, ToggleState>;
  'trust.check': Rpc<{ cwd: string }, TrustState>;
  'trust.set': Rpc<{ cwd: string; trusted: boolean }, never>;
  'app.doctor': Rpc<{ cwd: string }, DoctorReport>;
  'app.configSources': Rpc<{ cwd: string }, ConfigSources>;
  'app.releaseNotes': Rpc<Record<string, never>, { version: string; markdown: string }>;
  'session.info': Rpc<{ sessionId: string }, SessionInfo>;
  'state.read': Rpc<{ name: string }, Json>;
  'state.write': Rpc<{ name: string; value: Json }, null>;
  'sessions.list': Rpc<{ cwd: string }, SessionRow[]>;
  'sessions.events': Rpc<{ runDir: string }, EventEnvelopeV1[]>;
  'sessions.search': Rpc<
    { cwds: string[]; query: string; limit: number },
    { runDir: string; role: 'user' | 'assistant'; snippet: string }[]
  >;
  'sessions.rename': Rpc<{ runDir: string; title: string }, null>;
  'sessions.delete': Rpc<{ runDir: string }, { trashedTo: string }>;
  'sessions.pins': Rpc<{ cwd: string }, string[]>;
  'sessions.setPinned': Rpc<{ cwd: string; runId: string; pinned: boolean }, null>;
  'session.create': Rpc<
    {
      cwd: string;
      selection?: ModelSelection;
      accessMode?: AccessMode;
      worktree?: { name?: string; baseBranch?: string };
    },
    SessionHandle
  >;
  'session.continue': Rpc<{ runDir: string }, SessionHandle>;
  'session.close': Rpc<{ sessionId: string }, null>;
  'session.intent': Rpc<{ sessionId: string; intent: Intent }, null>;
  'models.catalog': Rpc<{ cwd: string }, ModelCatalog>;
  'commands.list': Rpc<{ cwd: string }, Command[]>;
  'commands.expand': Rpc<{ cwd: string; name: string; args: string }, { text: string }>;
  'mcp.resources': Rpc<
    { cwd: string },
    { name: string; uri: string; mime: string; description: string | null }[]
  >;
  'agents.list': Rpc<{ cwd: string }, { name: string }[]>;
  'worktrees.list': Rpc<{ cwd: string; all?: boolean }, WorktreeList>;
  'worktrees.remove': Rpc<
    { cwd: string; slug: string; force?: boolean; keepBranch?: boolean },
    { removed: true; slug: string; path: string; branch: string | null; delete_branch: boolean }
  >;
  'diagnostics.rebuildSessionIndex': Rpc<{ cwd: string }, IndexRebuild>;
  'diagnostics.crashScan': Rpc<{ cwd: string }, CrashScan>;
  'diagnostics.refreshModelCatalog': Rpc<{ cwd: string }, { message: string }>;
  'providers.protocols': Rpc<
    { cwd: string },
    { protocol: string; support: string; apis: string[] }[]
  >;
  'config.validate': Rpc<{ cwd: string }, { valid: true; message: string }>;
  'config.explain': Rpc<{ cwd: string; path: string }, ConfigExplanation>;
  'auth.command': Rpc<{ cwd: string; args: string[] }, { attempt: number }>;
  'files.index': Rpc<{ cwd: string; limit?: number }, FileEntry[]>;
  'providers.list': Rpc<Record<string, never>, Provider[]>;
  'auth.start': Rpc<{ args: string[]; stdin?: string }, { attempt: number }>;
  'auth.input': Rpc<{ attempt: number; line: string | null }, null>;
  'auth.logout': Rpc<{ provider: string }, null>;
  'settings.list': Rpc<{ cwd: string }, Setting[]>;
  'settings.write': Rpc<{ cwd: string; id: string; value: Json }, null>;
  'settings.reset': Rpc<{ cwd: string; id: string }, null>;
};
export type Method = keyof Methods;
export type Request<M extends Method = Method> = {
  id: number;
  method: M;
  params: Methods[M]['params'];
};
export type RpcError = { code: 'invalid_params' | 'not_found' | 'failed'; message: string };
export type Response = { id: number; result: unknown } | { id: number; error: RpcError };
export type Notification =
  | { method: 'session.update'; params: { sessionId: string; update: Update } }
  | {
      method: 'auth.update';
      params: {
        attempt: number;
        kind: 'notice' | 'result';
        level?: 'info' | 'error';
        success?: boolean;
        message: string;
      };
    };

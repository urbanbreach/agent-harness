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
export type ModelCatalog = { current: ModelEntry | null; models: ModelEntry[]; profiles: string[] };
export type Command = {
  name: string;
  description: string;
  argumentHint?: string;
  scope: 'builtin' | 'project' | 'user';
  template?: string;
};
export type FileEntry = { path: string; dir: boolean };
export type Provider = {
  id: string;
  label: string;
  description: string;
  methods: string[];
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
      worktree?: { name?: string };
    },
    SessionHandle
  >;
  'session.continue': Rpc<{ runDir: string }, SessionHandle>;
  'session.close': Rpc<{ sessionId: string }, null>;
  'session.intent': Rpc<{ sessionId: string; intent: Intent }, null>;
  'models.catalog': Rpc<{ cwd: string }, ModelCatalog>;
  'commands.list': Rpc<{ cwd: string }, Command[]>;
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

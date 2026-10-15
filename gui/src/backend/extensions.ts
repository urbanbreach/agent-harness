import type { AccessMode, SessionCatalogEntry } from './protocol';

export type SkillInstall = {
  directory: string;
  file: string;
  root: string;
  scope: 'user' | 'project';
  source: 'Harness' | 'Shared';
  enabled: boolean;
};
export type Skill = {
  id: string;
  name: string;
  description: string;
  scope: 'user' | 'project';
  project: string | null;
  enabled: boolean;
  body: string;
  allowedTools: string[];
  primaryDirectory: string;
  primaryFile: string;
  installs: SkillInstall[];
  supportingFiles: number;
  sizeBytes: number;
  updatedAt: number | null;
};
export type UsageAggregate = {
  key: string;
  provider?: string;
  model?: string;
  project?: string;
  records: number;
  sessions: number;
  inputTokens: number;
  cachedInputTokens: number;
  cacheWriteTokens: number;
  outputTokens: number;
  reasoningTokens: number;
  totalTokens: number;
  costUsd: number | null;
  knownCostUsd: number;
  unpricedRecords: number;
  lastActive: string | null;
  providers: UsageAggregate[];
  models: UsageAggregate[];
};
export type UsageHistory = {
  from: string;
  to: string;
  timeZone: 'UTC';
  totals: UsageAggregate;
  byCategory: UsageAggregate[];
  byModel: UsageAggregate[];
  byProvider: UsageAggregate[];
  byProject: UsageAggregate[];
  byDay: UsageAggregate[];
  byMonth: UsageAggregate[];
  quality: { providerReportedRecords: number; modelPricedRecords: number; unpricedRecords: number };
  scan: { scanned: number; skipped: number; records: number; elapsedMs: number };
  errors: string[];
  ratesAvailable: boolean;
  cacheSavingsUsd: number | null;
};
export type SubscriptionUsage = {
  provider: string;
  status: 'available' | 'unconfigured' | 'error';
  plan: string | null;
  windows: { label: string; usedPercent: number; resetsAt: string | null }[];
  message?: string;
};
export type MemoryEntry = {
  key: string;
  value: string;
  updated_at_unix_ms: number;
  scope: 'global' | 'workspace' | 'session';
};
export type MemoryList = { store_path: string; entries: MemoryEntry[] };
export type PlanEntry = {
  path: string;
  slug: string;
  exists: boolean;
  is_active: boolean;
  byte_len: number | null;
};
export type ForeignKind = 'codex' | 'claude' | 'open_code' | 'unknown';
export type ForeignCandidate =
  | { status: 'discoverable'; kind: ForeignKind; path: string; marker: string; importable: boolean }
  | { status: 'corrupt'; kind: ForeignKind; path: string; reason: string; importable: false }
  | { status: 'rejected'; path: string; reason: string; importable: false };
export type ForeignDiscovery = {
  scan_root: string;
  count: number;
  candidates: ForeignCandidate[];
  summary: {
    discoverable: number;
    importable: number;
    discoverable_not_importable: number;
    corrupt: number;
    rejected: number;
    total: number;
  };
};
export type ForeignImport = {
  run_id: string;
  run_dir: string;
  event_count: number;
  source_path: string;
  format: string;
  mode_source: SessionCatalogEntry['mode_source'];
};
export type SessionTree = {
  session_count: number;
  harness_lineage: (SessionCatalogEntry & { run_dir: string; depth: number })[];
  root: null;
  filter: null;
};
export type ToggleState = {
  entries: {
    id: string;
    kind: 'profile' | 'tool' | 'hook' | 'mcp';
    name: string;
    agent?: string;
    enabled: boolean;
  }[];
  appliesTo: 'newSessions';
  persisted: true;
};
export type TrustState = { supported: false; trusted: null; status: 'unknown'; reason: string };
export type DoctorReport = {
  noNetworkProbes: true;
  providerExecutionProof: false;
  scope: 'local_configuration';
  checks: { name: string; status: string; message?: string; unknown?: number }[];
  trust: { supported: false; status: 'unknown' };
};
export type ConfigSources = {
  primaryPath: string | null;
  layers: { path: string; kind: string }[];
  searched: string[];
  runtimeOverrides: string | null;
  scope: string;
};
export type SessionInfo = {
  sessionId: string;
  accessMode: AccessMode;
  runDir: string;
  cwd: string;
  profile: string;
  model: string | null;
  provider: string | null;
  selectionStatus: 'launch_snapshot';
  mcp: { name: string; enabled: boolean; status: string }[];
  lsp: { name: string; status: string }[];
  integrationStatusReason: string;
  sandbox: { status: 'unknown'; enforced: null; reason: string };
  goals: { supported: false; reason: string; guidance: string };
};

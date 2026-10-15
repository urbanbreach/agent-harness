// JSON emitted by harness-core's event schema and its referenced serde types.
// Defaults affect Rust deserialization, not field presence in serialized output.
// Rust integers serialize as JSON numbers; consumers must not assume u64 precision.
export type JsonValue =
  null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };
export type RunId = string;
export type RunName = string;
export type SessionId = string;
export type EntryId = string;
export type RequestId = string;
export type ProviderRequestId = string;
export type TaskId = string;
export type ToolCallId = string;
export type SubagentId = string;
export type InjectedSubagentDepth = number;

export type ActorKind = 'supervisor' | 'worker' | 'user' | 'system';
export interface EventActor {
  kind: ActorKind;
  agent_id?: string;
}
export interface EventEnvelopeV1 {
  schema_version: number;
  event_id: string;
  seq: number;
  run_id: RunId;
  mono_ms: number;
  ts?: string;
  actor: EventActor;
  correlation_id?: string;
  causation_id?: string;
  stream_key?: string;
  payload: EventV1;
}
export interface LiveEventEnvelope {
  event_id: string;
  run_id: RunId;
  mono_ms: number;
  ts: string | null;
  actor: EventActor;
  correlation_id: string | null;
  causation_id: string | null;
  stream_key: string | null;
  payload: LiveEventV1;
}
export type RuntimeEvent =
  { delivery: 'durable'; event: EventEnvelopeV1 } | { delivery: 'live'; event: LiveEventEnvelope };

export interface RunStartedEvent {
  run_name: RunName;
  workspace_root: string;
}
export interface RunFinishedEvent {
  summary: string;
}
export interface RunFailedEvent {
  error: string;
}
export interface SessionTitleUpdatedEvent {
  title: string;
}
export interface UserMessageSubmittedEvent {
  request_id: RequestId;
  text: string;
}
export interface AgentSpawnedEvent {
  agent_id: string;
  profile: string;
  parent_agent_id: string | null;
}
export interface AgentStoppedEvent {
  agent_id: string;
  reason: string;
}
export type TaskScheduleState = 'queued' | 'started';
export type TaskTerminalScope = 'agent_turn' | 'tool_call';
export interface TaskLineageMetadata {
  parent_tool_call_id: string | null;
  parent_task_id: string | null;
  parent_request_id: string | null;
  parent_session_id: string | null;
  child_session_id: string | null;
  child_request_id: string | null;
  child_provider_id: string | null;
  child_model_id: string | null;
}
export interface TaskScheduleMetadata {
  lineage: TaskLineageMetadata | null;
}
export interface TaskScheduledEvent {
  task_id: TaskId;
  state: TaskScheduleState;
  queue_key: string | null;
  metadata: TaskScheduleMetadata | null;
}
export interface ExecutionTimingMetadata {
  started_mono_ms: number | null;
  finished_mono_ms: number | null;
  elapsed_ms: number | null;
}
export interface TaskCompletionMetadata {
  lineage: TaskLineageMetadata | null;
  task_scope: TaskTerminalScope | null;
  timing: ExecutionTimingMetadata | null;
  hook_executions: HookExecutionMetadata[];
}
export interface TaskCancelledEvent {
  failure: boolean;
  task_id: TaskId;
  reason: string;
  task_scope: TaskTerminalScope | null;
}
export interface TaskCompletedEvent {
  task_id: TaskId;
  result_summary: string;
  result_digest: string;
  metadata: TaskCompletionMetadata | null;
}
export interface TaskResultLateEvent {
  task_id: TaskId;
  result_digest: string;
}
export interface StaleDetectedEvent {
  task_id: TaskId;
  stale_for_ms: number;
}
export type BackgroundTaskNotificationStatus = 'completed' | 'cancelled' | 'failed' | 'timed_out';
export interface BackgroundTaskNotificationEvent {
  parent_session_id: SessionId;
  parent_agent_id: string | null;
  child_session_id: SessionId;
  child_request_id: string;
  task_id: TaskId;
  description: string;
  status: BackgroundTaskNotificationStatus;
  summary: string;
  terminal_event_id: string;
  terminal_task_id: string;
  delivered_turn_request_id: string | null;
}

export interface CompletionUsage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  context_tokens: number | null;
  reasoning_tokens: number | null;
}
export type ProviderErrorCategory =
  | 'missing_credentials'
  | 'invalid_credentials'
  | 'rate_limited'
  | 'context_window_exceeded'
  | 'output_limit_exceeded'
  | 'unsupported_tool_call'
  | 'malformed_stream'
  | 'transport_failure'
  | 'other';
export type ProviderRetryFailure =
  | { http_status: number }
  | 'connection'
  | 'server'
  | 'authentication'
  | 'serialization'
  | 'idle_timeout'
  | 'empty_response'
  | 'truncated'
  | 'rate_limited'
  | 'other';
export interface ProviderRequestRetryMetadata {
  attempt: number;
  max_attempts: number;
  delay_ms: number | null;
  category: ProviderErrorCategory | null;
  failure?: ProviderRetryFailure;
}
export interface ProviderRequestCost {
  system_tokens: number;
  tools_tokens: number;
  history_tokens: number;
  attachments_tokens: number;
  framing_tokens: number;
  pending_prompt_tokens: number;
}
export type ProviderOutputCapDisposition =
  | { emitted: number }
  | { provider_defaulted: number }
  | 'provider_controlled'
  | 'unspecified_unknown_limit';
export type BudgetStatus = 'estimated' | 'conservative_fallback' | 'unknown_limits';
export type RequestBudgetComponents = ProviderRequestCost;
export interface RequestBudget {
  status: BudgetStatus;
  requested_output_tokens: number | null;
  reserved_output_tokens: number | null;
  maximum_input_tokens: number | null;
  safety_margin_tokens: number;
  compaction_threshold_tokens: number | null;
  components: RequestBudgetComponents;
  occupied_input_tokens: number;
  remaining_input_tokens: number | null;
  requires_compaction: boolean | null;
  output_cap_disposition: ProviderOutputCapDisposition;
}
export type RequestBudgetSnapshot = RequestBudget;
export type MaxInputSemantics = 'provider_visible_input_tokens';
export type ModelLimitProvenanceKind =
  | 'explicit_config'
  | 'generated_catalog'
  | 'provider_discovered'
  | 'compatibility_fallback'
  | 'unknown';
export interface ModelLimitProvenance {
  kind: ModelLimitProvenanceKind;
  detail: string;
  source: string | null;
  verified_at: string | null;
}
export interface ResolvedModelLimit {
  tokens: number | null;
  provenance: ModelLimitProvenance;
}
export interface ResolvedModelLimits {
  context_window: ResolvedModelLimit;
  max_input: ResolvedModelLimit;
  max_output: ResolvedModelLimit;
  max_input_semantics: MaxInputSemantics;
}
export interface CanonicalRuntimeSelection {
  profile: string | null;
  provider_id: string;
  model_id: string;
  variant: string | null;
  reasoning_effort: string | null;
  text_verbosity: string | null;
  reasoning_summary: string | null;
  thinking: JsonValue;
  resolved_limits: ResolvedModelLimits;
  profile_tool_shape_digest: string;
}
export interface ProviderRequestStartedMetadata {
  turn_id: string | null;
  provider_call_id: string | null;
  provider_session_id: string | null;
  provider_cache_id: string | null;
  retry: ProviderRequestRetryMetadata | null;
  context_budget: RequestBudgetSnapshot | null;
  runtime_selection: CanonicalRuntimeSelection | null;
}
export interface ProviderAssistantMessageMetadata {
  message_id: string | null;
  text_digest: string | null;
  reasoning_digest: string | null;
}
export interface ProviderStreamThinkingMetadata {
  summary: string | null;
  summary_digest: string | null;
  signature: string | null;
}
export type ProviderThinkingMetadata = ProviderStreamThinkingMetadata;
export interface ProviderNativeCompaction {
  provider_session_id: string;
  boundary_id: string;
  trigger: string;
  pre_tokens: number | null;
  post_tokens: number | null;
}
export interface ProviderRequestFinishedMetadata {
  turn_id: string | null;
  provider_call_id: string | null;
  provider_response_id: string | null;
  provider_session_id: string | null;
  provider_cache_id: string | null;
  provider_stop_reason: string | null;
  cache_read_tokens: number | null;
  cache_write_tokens: number | null;
  cache_write_1h_tokens?: number;
  cost_picodollars?: number;
  assistant_message: ProviderAssistantMessageMetadata | null;
  thinking: ProviderThinkingMetadata | null;
  provider_error_category: ProviderErrorCategory | null;
  provider_error_remediation: string | null;
  account?: string;
  native_compactions?: ProviderNativeCompaction[];
}
export interface ProviderRequestStartedEvent {
  request_id: RequestId;
  provider_id: string;
  model_id: string;
  prompt_summary: string;
  request_digest: string;
  metadata: ProviderRequestStartedMetadata | null;
}
export interface ProviderStreamDeltaEvent {
  request_id: RequestId;
  delta: string;
}
export type ProviderReasoningDeltaEvent = ProviderStreamDeltaEvent;
export interface ProviderRequestFinishedEvent {
  request_id: RequestId;
  finish_reason: string;
  output_digest: string | null;
  usage: CompletionUsage | null;
  metadata: ProviderRequestFinishedMetadata | null;
}
export interface AssistantToolCall {
  tool_call_id: ToolCallId;
  provider_tool_call_id: string | null;
  tool_id: string;
  args_summary: string;
  args_digest: string;
  provider_call_id: string | null;
}
export type AssistantPart =
  | { kind: 'text'; text: string }
  | { kind: 'reasoning'; text: string }
  | ({ kind: 'tool_call' } & AssistantToolCall);
export interface ProviderProvenance {
  provider_id: string;
  model_id: string;
  request_id: ProviderRequestId;
  response_id: string | null;
  stop_reason: string | null;
  usage: CompletionUsage | null;
  runtime_selection: CanonicalRuntimeSelection | null;
}
export interface AssistantMessageFinishedEvent {
  request_id: RequestId;
  tool_call_count: number;
  parts: AssistantPart[];
  provenance: ProviderProvenance | null;
  assistant_message: ProviderAssistantMessageMetadata | null;
}
export interface AttachmentDimensions {
  width: number;
  height: number;
}
export interface AttachmentMetadata {
  id: string;
  mime: string;
  size: number;
  dimensions: AttachmentDimensions | null;
  content_ref: string;
  detail?: string;
}
export interface PromptAttachmentsSubmittedEvent {
  request_id: RequestId;
  attachments: AttachmentMetadata[];
}

export type CompactionMethod = 'remote' | 'archive' | 'handoff' | 'shake' | 'soft';
export type ManualMode = 'soft' | 'remote' | 'archive';
export type ShakeMode = 'elide' | 'images' | 'thinking';
export interface NativePayloadRef {
  path: string;
  digest: string;
  bytes: number;
}
export interface NativeCompactionMeta {
  lane: string;
  provider_id: string | null;
  model_id: string | null;
  api: string | null;
  payload: NativePayloadRef | null;
}
export interface ArchiveFrameRecord {
  path: string;
  digest: string;
  bytes: number;
  width: number;
  height: number;
  chars: number;
  columns: number;
  rows: number;
  shape: string | null;
  detail: string | null;
  variant: string | null;
}
export interface ArchiveRecord {
  text: string;
  text_head: string;
  text_tail: string;
  truncated_chars: number;
  shape: string;
  chars: number;
  frames: ArchiveFrameRecord[];
}
export type PruneKindRecord =
  'superseded' | 'useless' | 'aged' | 'shake' | 'images' | 'thinking' | 'rescue';
export type ContextEdit =
  | {
      kind: 'tool_result';
      seq?: number;
      tool_call_id: string;
      text: string;
      pruned_at_ms: number;
      drop_attachments?: boolean;
    }
  | {
      kind: 'text_range';
      seq: number;
      start: number;
      end: number;
      text: string;
      elided_output_tokens?: number;
    }
  | { kind: 'drop_reasoning'; seq: number; elided_output_tokens?: number }
  | { kind: 'drop_attachments'; seq: number; tool_call_id: string | null };
export interface ContextPrunedEvent {
  agent_id: string;
  kind: PruneKindRecord;
  edits: ContextEdit[];
  tokens_saved: number;
  artifact_path: string | null;
  warning: string | null;
}
export type CompactionPhase =
  | 'preparing'
  | 'summarizing'
  | 'turn_prefix'
  | 'short_summary'
  | 'archiving'
  | 'remote'
  | 'handoff'
  | 'shaking'
  | 'committing'
  | 'done'
  | 'failed'
  | 'cancelled'
  | 'skipped';
export interface FrameThumbnail {
  width: number;
  height: number;
  bits: string;
}
export interface CompactionStage {
  reason: string;
  method: CompactionMethod | null;
  phase: CompactionPhase;
  tokens_before: number | null;
  tokens_after: number | null;
  summarized_tokens: number | null;
  kept_tokens: number | null;
  fixed_tokens: number | null;
  threshold_tokens: number | null;
  window_tokens: number | null;
  first_kept_seq: number | null;
  windows_done: number;
  windows_total: number;
  frames_done: number;
  frames_total: number;
  thumbnails: FrameThumbnail[];
  message: string | null;
}
export interface SessionCompactionEvent {
  agent_id: string;
  summary: string;
  first_kept_event_seq: number;
  first_kept_request_id: string | null;
  first_kept_entry_id: EntryId | null;
  tokens_before: number;
  tokens_after: number | null;
  summary_usage: CompletionUsage | null;
  summary_provider_id: string | null;
  summary_model_id: string | null;
  read_files: string[];
  modified_files: string[];
  task_intent: string | null;
  current_intent: UiIntentReceivedEvent | null;
  trigger_reason: string;
  from_hook: boolean;
  short_summary: string | null;
  method: CompactionMethod | null;
  warning: string | null;
  provider_replay_through_seq: number | null;
  native: NativeCompactionMeta | null;
  archive: ArchiveRecord | null;
  details?: JsonValue;
  preserve_data?: Record<string, JsonValue>;
  supersedes_seq?: number;
  operation_id?: string;
}
export interface BranchSummaryEvent {
  agent_id: string;
  summary: string;
  from_event_seq: number;
  read_files: string[];
  modified_files: string[];
  from_hook: boolean;
  details?: JsonValue;
}
export type CompactionPatch =
  | { kind: 'warning'; text: string }
  | { kind: 'reader_view'; reader_key: string; summary: string; archive: ArchiveRecord | null };
export interface CompactionPatchedEvent {
  agent_id: string;
  compaction_seq: number;
  patch: CompactionPatch;
}
export interface ContextNotesEvent {
  agent_id: string;
  version: number;
  text: string;
}
export interface CompactionRequestedEvent {
  checkpoint_id: string;
  agent_id: string;
  trigger_reason: string;
  through_seq: number;
  through_request_id: string | null;
  provider_id: string | null;
  model_id: string | null;
  tokens_before: number | null;
  tokens_before_estimate: number | null;
  estimate_source: string | null;
}
export interface ProviderCompactionSummarySource {
  strategy: string;
  model_ref: string;
  provider_id: string | null;
  model_id: string | null;
  reasoning_effort: string | null;
  text_verbosity: string | null;
  previous_summary_used: boolean;
  model_backed: boolean;
  deterministic_fallback: boolean;
  summary_contract_version: number | null;
  summary_contract_enforced: boolean | null;
}
export interface CompactionWrittenEvent extends CompactionRequestedEvent {
  artifact_path: string;
  artifact_digest: string | null;
  artifact_bytes: number;
  tokens_after_estimate: number | null;
  summary_tokens_estimate: number | null;
  compacted_turns: number | null;
  reduction_tokens_estimate: number | null;
  reduction_percent_estimate: number | null;
  summary_source: ProviderCompactionSummarySource | null;
  preserved_turns: number;
}
export interface CompactionAppliedEvent {
  checkpoint_id: string;
  agent_id: string;
  through_seq: number;
  through_request_id: string | null;
  tokens_before_estimate: number | null;
  tokens_after_estimate: number | null;
  summary_tokens_estimate: number | null;
  compacted_turns: number | null;
  preserved_turns: number | null;
  reduction_tokens_estimate: number | null;
  reduction_percent_estimate: number | null;
  estimate_source: string | null;
}
export interface CompactionFailedEvent {
  agent_id: string;
  trigger_reason: string;
  reason: string;
  checkpoint_id: string | null;
  through_seq: number | null;
  through_request_id: string | null;
}
export type LifecyclePhase = 'start' | 'end';
export interface CompactionEndResult {
  method: CompactionMethod | null;
  tokens_before: number | null;
  tokens_after: number | null;
  short_summary: string | null;
  warning: string | null;
  first_kept_seq: number | null;
}
export interface CompactionLifecycleEvent {
  agent_id: string;
  operation_id: string;
  phase: LifecyclePhase;
  reason: string;
  action: string;
  will_retry: boolean;
  aborted: boolean;
  skipped: boolean;
  error: string | null;
  result: CompactionEndResult | null;
}
export type NoticeSource = 'compaction' | 'other';
export type NoticeSeverity = 'info' | 'warning' | 'error';
export type SpeculationState = 'idle' | 'armed' | 'running';
export interface CompactionStatusEvent {
  agent_id: string;
  auto: boolean;
  threshold_tokens: number | null;
  speculation_tokens: number | null;
  speculation: SpeculationState;
}

export type HookExecutionStatus = 'succeeded' | 'blocked' | 'failed' | 'skipped' | 'unknown';
export interface HookExecutionMetadata {
  hook_name: string;
  status: HookExecutionStatus;
  hook_event: string | null;
  command_digest: string | null;
  output_digest: string | null;
  output_summary: string | null;
  duration_ms: number | null;
}
export interface EventArtifactRef {
  path: string;
  digest: string | null;
}
export interface ToolCallMetadata {
  useless?: boolean;
  source_artifact?: boolean;
  canonical_tool_id: string | null;
  alias_source_tool_id: string | null;
  lineage: TaskLineageMetadata | null;
  artifact_refs: EventArtifactRef[];
  attachments?: AttachmentMetadata[];
  timing: ExecutionTimingMetadata | null;
  hook_executions: HookExecutionMetadata[];
}
export interface ToolIdentityMetadata {
  canonical_tool_id: string | null;
  alias_source_tool_id: string | null;
}
export interface ResolvedToolIdentity extends ToolIdentityMetadata {
  invoked_tool_id: string | null;
  effective_tool_id: string | null;
}
export type ToolCallStatus = 'succeeded' | 'failed';
export type ToolCallLifecycleState = 'pending' | 'running' | 'completed' | 'error';
export interface ToolCallRequestedEvent {
  tool_call_id: ToolCallId;
  tool_id: string;
  args_summary: string;
  args_digest: string;
  metadata: ToolCallMetadata | null;
}
export interface ToolCallStartedEvent {
  tool_call_id: ToolCallId;
}
export interface ToolCallFinishedEvent {
  tool_call_id: ToolCallId;
  status: ToolCallStatus;
  output_summary: string | null;
  output_digest: string | null;
  output_json: JsonValue;
  metadata: ToolCallMetadata | null;
}
export type PermissionDecision = 'allow' | 'deny';
export type PermissionKind =
  | 'edit_fs'
  | 'shell'
  | 'network'
  | 'question'
  | 'task'
  | 'eval'
  | 'web_fetch'
  | 'web_search'
  | 'code_search'
  | 'lsp'
  | 'read'
  | 'external_directory'
  | 'doom_loop';
export type PermissionGrantScope = 'run' | 'session' | 'workspace';
export interface PermissionToolSelector {
  effective_tool_id: string;
  canonical_tool_id: string | null;
}
export type PermissionGrantMatcher =
  | { selector: 'request_digest'; request_digest: string }
  | {
      selector: 'shell_command';
      command_digest: string;
      request_digest: string;
      always_patterns: string[];
    }
  | { selector: 'workspace_path'; path: string; request_digest: string }
  | { selector: 'external_path'; path_prefix: string; request_digest: string };
export interface PermissionGrantRequest {
  kind: PermissionKind;
  tool: PermissionToolSelector;
  matcher: PermissionGrantMatcher;
}
export interface PermissionGrant extends PermissionGrantRequest {
  grant_id: string;
  permission_id: string;
  scope: PermissionGrantScope;
  expires_at: string | null;
}
export interface PermissionRequestedEvent {
  permission_id: string;
  kind: string;
  tool_call_id: ToolCallId | null;
  summary: string;
  request_digest: string;
  timeout_ms: number;
  default_decision: PermissionDecision;
}
export type PermissionRequestedArgs = PermissionRequestedEvent;
export interface PermissionResolvedEvent {
  permission_id: string;
  decision: PermissionDecision;
  reason: string | null;
}
export interface PermissionGrantRecordedEvent {
  grant: PermissionGrant;
}
export interface YoloModeChangedEvent {
  enabled: boolean;
}
export interface EditProposedEvent {
  edit_id: string;
  path: string;
  summary: string;
  patch_digest: string;
}
export interface EditAppliedEvent {
  edit_id: string;
  path: string;
  new_file_digest: string;
  diff_rel_path: string | null;
  diff_digest: string | null;
}
export interface EditRejectedEvent {
  edit_id: string;
  path: string;
  reason: string;
}
export interface ArtifactWrittenEvent {
  path: string;
  digest: string;
  bytes: number;
  tool_call_id: ToolCallId | null;
  tool_metadata: ToolIdentityMetadata | null;
  metadata: Record<string, string>;
}
export interface PolicyViolationDetectedEvent {
  policy: string;
  detail: string;
}
export interface UiIntentReceivedEvent {
  intent: string;
  params: Record<string, string>;
}
export interface ConversationRewoundEvent {
  target_seq: number;
  request_id: string;
}
export interface WorkspaceSnapshotEvent {
  request_id: RequestId;
  artifact_path: string;
  artifact_digest: string;
  file_count: number;
}
export interface WorkspaceRevertFailure {
  path: string;
  reason: string;
}
export interface WorkspaceRevertedEvent {
  request_id: RequestId;
  snapshot_request_id: string;
  restored_paths: string[];
  removed_paths: string[];
  failed_paths: WorkspaceRevertFailure[];
  conflicts: WorkspaceRevertFailure[];
}

export interface SubagentAncestry {
  spawner: SubagentId | null;
  origin: SubagentId | null;
}
export type LifecycleOrigin = 'stream' | 'reconciliation';
// These two Rust enums deliberately retain serde's default external tags and casing.
export type SubagentExecutionOwner =
  { RootSession: { session_id: string } } | { ChildSession: { child_id: SubagentId } };
export type ResolvedSubagentIsolation = 'SharedWorkspace' | { Worktree: { path: string } };
export interface SubagentNotificationRoute {
  parent_prompt_id: string | null;
  background: boolean;
  await_to_completion: boolean;
  surface_completion: boolean;
}
export interface SubagentDisplayRoute {
  root_session_id: string;
  parent_session_id: string | null;
  child_session_id: string;
}
export interface ResolvedSubagentContext {
  effective_cwd: string;
  policy_roots: string[];
  isolation: ResolvedSubagentIsolation;
}
export type FinalizedStateFidelity = 'exact' | 'semantic' | 'summary_only' | 'redacted';
export type FinalizedStateAvailability = 'available' | 'missing' | 'unsupported';
export interface FinalizedSubagentStateReference {
  payload_version: number;
  sha256: string;
  owner: SubagentId;
  fidelity: FinalizedStateFidelity;
  availability: FinalizedStateAvailability;
}
export type FinalizedStateUnavailable =
  | 'missing'
  | 'active'
  | 'legacy_summary_only'
  | 'policy_modified'
  | 'unsupported_reasoning'
  | 'unsupported_encoding'
  | 'unsupported_version'
  | 'corrupt'
  | 'owner_mismatch'
  | 'incomplete';
export interface FinalizedAgentStateReferenceV1 {
  payload_version: number;
  state: FinalizedSubagentStateReference;
  byte_length: number;
  owner_run_id: string;
  owner_session_id: string;
  attempt_id: string;
  generation: number;
  unavailable: FinalizedStateUnavailable | null;
}
export interface SubagentTerminalAccounting {
  tool_calls: number;
  turns: number;
  duration_ms: number;
  tokens_used: number | null;
  output_tokens_used: number | null;
  total_tokens_used: number | null;
  output_usage_incomplete: boolean;
}
export type SubagentTerminalOutcome =
  | 'completed'
  | 'stationarity_ended'
  | 'max_tokens'
  | 'cancelled'
  | 'max_turns_reached'
  | 'rewound'
  | 'removed_from_queue'
  | 'session_error'
  | 'dropped_result_channel';
export type SubagentCommandRequest =
  | { command: 'explicit_child_kill'; child_id: SubagentId }
  | { command: 'child_session_cancel'; session_id: string; descendants: SubagentId[] }
  | { command: 'parent_prompt_cancel'; prompt_id: string }
  | { command: 'parent_session_stop'; session_id: string }
  | { command: 'waiter_cancel'; waiter_id: string }
  | { command: 'root_shutdown' };
export type FinalizedContextCopy = 'fork' | 'resume' | 'wake';
export interface AgentContextInitializedV1 {
  payload_version: number;
  agent_id: SubagentId;
  mode: FinalizedContextCopy;
  source: FinalizedAgentStateReferenceV1;
}
export interface AgentExecutionContextChangedV1 {
  payload_version: number;
  agent_id: SubagentId;
  context: ResolvedSubagentContext;
  system_prompt?: string;
}
export type SubagentIsolationMode = 'none' | 'worktree';
export interface SubagentTransitionMetadataV1 {
  ancestry: SubagentAncestry;
  execution_owner: SubagentExecutionOwner;
  display_route: SubagentDisplayRoute;
  notification_route: SubagentNotificationRoute;
  injected_depth: InjectedSubagentDepth;
  isolation_requested: SubagentIsolationMode;
  context: ResolvedSubagentContext;
}
export type SubagentTransitionKind = 'spawned' | 'progress' | 'finished' | 'retired' | 'routed';
export interface SubagentTransitionV1 {
  payload_version: number;
  child_id: SubagentId;
  attempt_id: string | null;
  generation: number;
  notification_seq: number | null;
  origin: LifecycleOrigin;
  transition: SubagentTransitionKind;
  metadata: SubagentTransitionMetadataV1;
  outcome: SubagentTerminalOutcome | null;
  accounting: SubagentTerminalAccounting | null;
  finalized_state: FinalizedAgentStateReferenceV1 | null;
}
export interface SubagentCancelIntentV1 {
  payload_version: number;
  command: SubagentCommandRequest;
  targets: SubagentId[];
}
export type PermissionAction = 'allow' | 'ask' | 'deny';
export type PermissionSelector =
  { exact: string } | { prefix: string } | { glob: string } | 'catch_all';
export interface PermissionRule {
  permission: string;
  pattern: PermissionSelector;
  action: PermissionAction;
}
export type PermissionRuleset = PermissionRule[];
export interface NativeSubagentRegistration {
  payload_version: number;
  child_id: string;
  spawner: string;
  root_agent: string;
  parent_tool: string;
  parent_request: string | null;
  subagent_type: string;
  persona: string | null;
  role: string | null;
  fork_context: boolean;
  description: string;
  prompt: string;
  output_schema?: Record<string, JsonValue>;
  background: boolean;
  isolation: SubagentIsolationMode;
  source: string | null;
  model: string;
  system_prompt: string;
  tools: string[];
  permission_rules: PermissionRuleset;
  max_iters: number | null;
  allowed_types: string[] | null;
  model_inherited: boolean;
  messaging_granted: boolean;
}
export type SendSubagentMessageDelivery = 'steer' | 'queue' | 'interject';
export interface NativeMessageReceipt {
  payload_version: number;
  message_id: string;
  sender: string;
  sender_attempt: string | null;
  sender_generation: number;
  recipient: string;
  recipient_attempt: string | null;
  recipient_generation: number;
  delivery: SendSubagentMessageDelivery;
  text: string;
  status: string;
}
export interface NativeSubagentReceipt {
  payload_version: number;
  child_id: string;
  attempt_id: string | null;
  generation: number;
  kind: string;
  waiter_id: string | null;
}
export interface NativeWorkspaceReceipt {
  payload_version: number;
  child_id: string;
  snapshot_ref: string;
  worktree_path: string;
  removed: boolean;
}
export interface SubagentProgressEvent {
  child_id: string;
  attempt_id: string;
  generation: number;
  duration_ms: number;
  turn_count: number;
  tool_call_count: number;
  tokens_used: number | null;
  context_window_tokens: number | null;
  context_usage_pct: number | null;
  tools_used: string[];
  error_count: number;
}
export type RuntimeReminderKind =
  | 'todo_continuation'
  | 'loop_guard'
  | 'stream_guard'
  | 'directory_instructions'
  | 'command_completed'
  | 'output_contract'
  | 'monitor_event'
  | 'compaction_resume'
  | 'length_stop'
  | 'notes_window';
export interface RuntimeReminderEvent {
  request_id: RequestId;
  kind: RuntimeReminderKind;
  text: string;
  source?: string;
}
export type CommandNoticeEvent =
  | { state: 'queued'; task_id: string; status: string; body: string }
  | { state: 'observed'; task_id: string }
  | { state: 'wake_attempted' };
export interface SteeringAcceptedEvent {
  request_id: RequestId;
  turn_id: string;
  text: string;
}

export interface EvalProgressEvent {
  tool_call_id: ToolCallId;
  output: string;
  details: JsonValue;
}
export interface RuntimeWarningEvent {
  message: string;
}
export interface RuntimeNoticeEvent {
  source: NoticeSource;
  severity: NoticeSeverity;
  message: string;
}
export interface MonitorStateEvent {
  task_id: string;
  paused: boolean;
}
export interface CompactionProgressEvent {
  agent_id: string;
  generation: number;
  trigger_reason: string;
  preview: string | null;
  stage?: CompactionStage;
}
export interface ProviderRetryingEvent {
  turn_id: RequestId;
  retry: ProviderRequestRetryMetadata;
}
export interface ProviderTextDeltaEvent {
  request_id: ProviderRequestId;
  delta: string;
}
export interface LiveProviderReasoningDeltaEvent {
  request_id: ProviderRequestId;
  delta: string;
}
export interface ProviderToolInputDeltaEvent {
  request_id: ProviderRequestId;
  tool_call_id: ToolCallId;
  tool_name?: string;
  delta: string;
}

export type LiveEventV1 =
  | { event_type: 'eval_progress'; data: EvalProgressEvent }
  | { event_type: 'subagent_progress'; data: SubagentProgressEvent }
  | { event_type: 'runtime_warning'; data: RuntimeWarningEvent }
  | { event_type: 'runtime_notice'; data: RuntimeNoticeEvent }
  | { event_type: 'compaction_lifecycle'; data: CompactionLifecycleEvent }
  | { event_type: 'compaction_status'; data: CompactionStatusEvent }
  | { event_type: 'monitor_state'; data: MonitorStateEvent }
  | { event_type: 'compaction_progress'; data: CompactionProgressEvent }
  | { event_type: 'provider_retrying'; data: ProviderRetryingEvent }
  | { event_type: 'provider_text_delta'; data: ProviderTextDeltaEvent }
  | { event_type: 'provider_reasoning_delta'; data: LiveProviderReasoningDeltaEvent }
  | { event_type: 'provider_tool_input_delta'; data: ProviderToolInputDeltaEvent };

export type EventV1 =
  | { event_type: 'eval_cell_finished'; data: ToolCallFinishedEvent }
  | { event_type: 'run_started'; data: RunStartedEvent }
  | { event_type: 'run_finished'; data: RunFinishedEvent }
  | { event_type: 'user_message_submitted'; data: UserMessageSubmittedEvent }
  | { event_type: 'session_title_updated'; data: SessionTitleUpdatedEvent }
  | { event_type: 'run_failed'; data: RunFailedEvent }
  | { event_type: 'agent_spawned'; data: AgentSpawnedEvent }
  | { event_type: 'agent_stopped'; data: AgentStoppedEvent }
  | { event_type: 'task_scheduled'; data: TaskScheduledEvent }
  | { event_type: 'task_cancelled'; data: TaskCancelledEvent }
  | { event_type: 'task_completed'; data: TaskCompletedEvent }
  | { event_type: 'task_result_late'; data: TaskResultLateEvent }
  | { event_type: 'background_task_notification'; data: BackgroundTaskNotificationEvent }
  | { event_type: 'stale_detected'; data: StaleDetectedEvent }
  | { event_type: 'prompt_attachments_submitted'; data: PromptAttachmentsSubmittedEvent }
  | { event_type: 'provider_request_started'; data: ProviderRequestStartedEvent }
  | { event_type: 'provider_stream_delta'; data: ProviderStreamDeltaEvent }
  | { event_type: 'provider_reasoning_delta'; data: ProviderReasoningDeltaEvent }
  | { event_type: 'provider_request_finished'; data: ProviderRequestFinishedEvent }
  | { event_type: 'assistant_message_finished'; data: AssistantMessageFinishedEvent }
  | { event_type: 'compaction_requested'; data: CompactionRequestedEvent }
  | { event_type: 'compaction_written'; data: CompactionWrittenEvent }
  | { event_type: 'compaction_applied'; data: CompactionAppliedEvent }
  | { event_type: 'compaction_failed'; data: CompactionFailedEvent }
  | { event_type: 'session_compaction'; data: SessionCompactionEvent }
  | { event_type: 'context_pruned'; data: ContextPrunedEvent }
  | { event_type: 'branch_summary'; data: BranchSummaryEvent }
  | { event_type: 'compaction_patched'; data: CompactionPatchedEvent }
  | { event_type: 'context_notes'; data: ContextNotesEvent }
  | { event_type: 'tool_call_requested'; data: ToolCallRequestedEvent }
  | { event_type: 'tool_call_started'; data: ToolCallStartedEvent }
  | { event_type: 'tool_call_finished'; data: ToolCallFinishedEvent }
  | { event_type: 'permission_requested'; data: PermissionRequestedEvent }
  | { event_type: 'permission_grant_recorded'; data: PermissionGrantRecordedEvent }
  | { event_type: 'permission_resolved'; data: PermissionResolvedEvent }
  | { event_type: 'yolo_mode_changed'; data: YoloModeChangedEvent }
  | { event_type: 'edit_proposed'; data: EditProposedEvent }
  | { event_type: 'edit_applied'; data: EditAppliedEvent }
  | { event_type: 'edit_rejected'; data: EditRejectedEvent }
  | { event_type: 'artifact_written'; data: ArtifactWrittenEvent }
  | { event_type: 'policy_violation_detected'; data: PolicyViolationDetectedEvent }
  | { event_type: 'ui_intent_received'; data: UiIntentReceivedEvent }
  | { event_type: 'conversation_rewound'; data: ConversationRewoundEvent }
  | { event_type: 'workspace_snapshot'; data: WorkspaceSnapshotEvent }
  | { event_type: 'workspace_reverted'; data: WorkspaceRevertedEvent }
  | { event_type: 'subagent_transition'; data: SubagentTransitionV1 }
  | { event_type: 'finalized_agent_state'; data: FinalizedAgentStateReferenceV1 }
  | { event_type: 'agent_context_initialized'; data: AgentContextInitializedV1 }
  | { event_type: 'agent_execution_context_changed'; data: AgentExecutionContextChangedV1 }
  | { event_type: 'subagent_cancel_requested'; data: SubagentCancelIntentV1 }
  | { event_type: 'native_subagent_registered'; data: NativeSubagentRegistration }
  | { event_type: 'native_subagent_message'; data: NativeMessageReceipt }
  | { event_type: 'native_subagent_receipt'; data: NativeSubagentReceipt }
  | { event_type: 'native_subagent_workspace'; data: NativeWorkspaceReceipt }
  | { event_type: 'runtime_reminder'; data: RuntimeReminderEvent }
  | { event_type: 'command_notice'; data: CommandNoticeEvent }
  | { event_type: 'steering_accepted'; data: SteeringAcceptedEvent };

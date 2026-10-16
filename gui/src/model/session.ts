import { t, type TranslationKey, type TranslationVariables } from '../i18n';
import type { Update, SessionCatalogEntry } from '../backend/protocol';
import type {
  EventEnvelopeV1,
  LiveEventEnvelope,
  EventV1,
  AssistantPart,
  AttachmentMetadata,
  ToolCallMetadata,
  CompletionUsage,
  ProviderRequestRetryMetadata,
} from './harness-events';
import { presentTool, type ToolPresentation } from './tools';

type Data<K extends EventV1['event_type']> = Extract<EventV1, { event_type: K }>['data'];
export type RunStatus = 'idle' | 'working' | 'waiting' | 'failed' | 'background';
export type TurnStatus = 'queued' | 'working' | 'completed' | 'failed' | 'interrupted';
interface EntryBase {
  id: string;
  turnId?: string;
  seq?: number;
  startedMs: number;
  timestamp?: string;
}
export interface UserEntry extends EntryBase {
  kind: 'user';
  requestId: string;
  text: string;
  attachments: AttachmentMetadata[];
  fileTags: unknown[];
  agentTags: unknown[];
  resourceTags: unknown[];
  steering: boolean;
  queued?: boolean;
}
export interface TextEntry extends EntryBase {
  kind: 'text' | 'reasoning';
  requestId: string;
  text: string;
  streaming: boolean;
  endedMs?: number;
}
export interface ToolEntry extends EntryBase {
  kind: 'tool';
  toolCallId: string;
  requestId?: string;
}
export interface NoticeEntry extends EntryBase {
  kind: 'notice';
  level: 'info' | 'warning' | 'error';
  message: string;
  localized?: { key: TranslationKey; vars?: TranslationVariables };
  source?: string;
}
export interface CompactionEntry extends EntryBase {
  kind: 'compaction';
  event: EventV1;
  superseded: boolean;
  warning?: string;
  readerViews?: Record<string, unknown>;
}
export type TranscriptEntry = UserEntry | TextEntry | ToolEntry | NoticeEntry | CompactionEntry;
export interface TurnView {
  id: string;
  requestIds: string[];
  userIds: string[];
  status: TurnStatus;
  startedMs: number;
  endedMs?: number;
  durationMs?: number;
  error?: string;
}
export interface ToolActivity {
  id: string;
  turnId: string;
  requestId?: string;
  toolId: string;
  args: unknown;
  argsSummary: string;
  inputDelta?: string;
  status: 'queued' | 'running' | 'waiting' | 'succeeded' | 'failed' | 'cancelled';
  startedMs?: number;
  endedMs?: number;
  durationMs?: number;
  output?: unknown;
  outputSummary?: string;
  metadata?: ToolCallMetadata | null;
  permissions: PermissionView[];
  artifacts: Data<'artifact_written'>[];
  presentation: ToolPresentation;
}
export interface PermissionView extends Data<'permission_requested'> {
  startedMs: number;
  decision?: Data<'permission_resolved'>['decision'];
  reason?: string | null;
  resolvedMs?: number;
}
export interface QuestionRequest {
  permissionId: string;
  toolCallId?: string;
  summary: string;
  questions: unknown[];
}
export interface QueuedInput {
  id: string;
  text: string;
  turnId?: string;
  state: 'queued' | 'steering';
  attachments: AttachmentMetadata[];
}
export interface TaskView {
  id: string;
  state: string;
  background?: boolean;
  startedMs?: number;
  endedMs?: number;
  summary?: string;
  lineage?: unknown;
  scope?: string | null;
  data?: unknown;
}
export interface TodoItem {
  content: string;
  status: 'pending' | 'in_progress' | 'completed' | 'cancelled';
}
export interface SessionView {
  meta?: SessionCatalogEntry;
  runId?: string;
  rootAgentId?: string;
  title: string;
  workspaceRoot?: string;
  status: RunStatus;
  terminal: boolean;
  terminalError?: string;
  entries: TranscriptEntry[];
  turns: TurnView[];
  tools: Record<string, ToolActivity>;
  permissions: PermissionView[];
  questionRequests: QuestionRequest[];
  queued: QueuedInput[];
  tasks: Record<string, TaskView>;
  subagents: Record<string, TaskView>;
  todos: TodoItem[];
  usage?: CompletionUsage;
  usageByRequest: Record<string, CompletionUsage>;
  context?: {
    occupiedTokens: number;
    maximumInputTokens: number | null;
    remainingTokens: number | null;
    windowTokens?: number | null;
    budget: unknown;
  };
  retries: Record<string, { retry: ProviderRequestRetryMetadata; startedMs: number }>;
  compaction: Record<string, { lifecycle?: unknown; status?: unknown; progress?: unknown }>;
  contextPruning: Data<'context_pruned'>[];
  snapshots: Data<'workspace_snapshot'>[];
  workspaceReverts: Data<'workspace_reverted'>[];
  edits: Record<string, unknown>;
  artifacts: Data<'artifact_written'>[];
  yolo: boolean;
  statusText?: string;
  commandOutput?: unknown;
  rewind?: unknown;
  restoredQueued: string[];
  events: EventEnvelopeV1[];
  seenSeq: ReadonlySet<number>;
  providerTurns: Record<string, string>;
  settledRequests: ReadonlySet<string>;
  activeProviderRequests: ReadonlySet<string>;
}

export function emptySessionView(meta?: SessionCatalogEntry): SessionView {
  return {
    meta,
    runId: meta?.run_id,
    workspaceRoot: meta?.workspace_root ?? undefined,
    title: meta?.run_name ?? '',
    status: meta?.status === 'failed' ? 'failed' : 'idle',
    terminal: false,
    entries: [],
    turns: [],
    tools: {},
    permissions: [],
    questionRequests: [],
    queued: [],
    tasks: {},
    subagents: {},
    todos: [],
    usageByRequest: {},
    retries: {},
    compaction: {},
    contextPruning: [],
    snapshots: [],
    workspaceReverts: [],
    edits: {},
    artifacts: [],
    yolo: false,
    restoredQueued: [],
    events: [],
    seenSeq: new Set(),
    providerTurns: {},
    settledRequests: new Set(),
    activeProviderRequests: new Set(),
  };
}

function parse(value: string): unknown {
  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}
function object(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}
function array(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}
function isText(entry: TranscriptEntry): entry is TextEntry {
  return entry.kind === 'text' || entry.kind === 'reasoning';
}
function presentation(tool: Omit<ToolActivity, 'presentation'> | ToolActivity): ToolPresentation {
  return presentTool(tool.toolId, tool.args, tool.output ?? tool.outputSummary, {
    ...tool.metadata,
    status: tool.status,
    output_summary: tool.outputSummary,
  });
}

/** Identical boundary rule to conversation_rewind::active_events: target itself is removed. */
export function activeEvents(events: readonly EventEnvelopeV1[]): EventEnvelopeV1[] {
  let active: EventEnvelopeV1[] = [];
  for (const event of events) {
    if (event.payload.event_type === 'conversation_rewound') {
      let lo = 0,
        hi = active.length;
      while (lo < hi) {
        const mid = (lo + hi) >>> 1;
        if (active[mid]!.seq < event.payload.data.target_seq) lo = mid + 1;
        else hi = mid;
      }
      active = active.slice(0, lo);
    }
    active.push(event);
  }
  return active;
}

export function projectEvents(events: EventEnvelopeV1[]): SessionView {
  const seen = new Set<number>();
  const unique = events.filter((event) => {
    if (seen.has(event.seq)) return false;
    seen.add(event.seq);
    return true;
  });
  return replay(unique, undefined, seen);
}
function replay(
  events: EventEnvelopeV1[],
  meta: SessionCatalogEntry | undefined,
  seen: ReadonlySet<number>,
): SessionView {
  const active = activeEvents(events);
  const withdrawn = new Set(
    active.flatMap((event) =>
      event.payload.event_type === 'task_cancelled' &&
      event.payload.data.reason === 'returned to the editor'
        ? [event.payload.data.task_id]
        : [],
    ),
  );
  let view = { ...emptySessionView(meta), events };
  for (const event of active)
    if (!event.correlation_id || !withdrawn.has(event.correlation_id))
      view = reduceEvent(view, event);
  return { ...view, events, seenSeq: seen };
}

export function applyUpdate(view: SessionView, update: Update): SessionView {
  switch (update.kind) {
    case 'event': {
      if (update.delivery === 'live') return reduceEvent(view, update.event);
      const event = update.event;
      if (view.seenSeq.has(event.seq)) return view;
      const seen = new Set(view.seenSeq);
      seen.add(event.seq);
      const events = [...view.events, event];
      if (
        event.payload.event_type === 'conversation_rewound' ||
        (event.payload.event_type === 'task_cancelled' &&
          event.payload.data.reason === 'returned to the editor')
      )
        return replay(events, view.meta, seen);
      return { ...reduceEvent(view, event), events, seenSeq: seen };
    }
    case 'status':
      return view.statusText === update.text ? view : { ...view, statusText: update.text };
    case 'notice':
      return view.turns.some((turn) => turn.status === 'working' || turn.status === 'queued')
        ? notice(view, `notice:${view.entries.length}`, update.level, update.message, 0)
        : view;
    case 'modelNotice':
      return view.turns.some((turn) => turn.status === 'working' || turn.status === 'queued')
        ? notice(view, `model:${view.entries.length}`, 'info', update.message, 0, 'model')
        : view;
    case 'yolo':
      return { ...view, yolo: update.enabled };
    case 'yoloFailed':
      return notice(
        view,
        `yolo:${view.entries.length}`,
        'error',
        t('transcript.access_failed'),
        0,
        undefined,
        undefined,
        { key: 'transcript.access_failed' },
      );
    case 'compaction':
      return update.message
        ? notice(
            view,
            `compaction:${view.entries.length}`,
            update.outcome === 'failed'
              ? 'error'
              : update.outcome === 'warning'
                ? 'warning'
                : 'info',
            update.message,
            0,
            'compaction',
          )
        : view;
    case 'commandOutput':
      return { ...view, commandOutput: update.snapshot };
    case 'rewindPoints':
    case 'rewindComplete':
      return { ...view, rewind: update };
    case 'restoreQueued':
      return { ...view, restoredQueued: update.texts };
    case 'queuedReceipt':
      return update.error ? notice(view, `queue:${update.id}`, 'error', update.error, 0) : view;
    case 'forked':
      return view;
    case 'exited':
      return settle(
        { ...view, terminal: true, terminalError: update.error },
        update.error ? 'failed' : 'interrupted',
        0,
        update.error,
      );
  }
}
function notice(
  view: SessionView,
  id: string,
  level: NoticeEntry['level'],
  message: string,
  ms: number,
  source?: string,
  seq?: number,
  localized?: NoticeEntry['localized'],
): SessionView {
  return {
    ...view,
    entries: [
      ...view.entries,
      {
        kind: 'notice',
        id,
        level,
        message,
        startedMs: ms,
        source,
        seq,
        ...(localized ? { localized } : {}),
      },
    ],
  };
}
function settle(
  view: SessionView,
  status: TurnStatus,
  ms: number,
  error?: string,
  turnId?: string,
): SessionView {
  const targets = new Set(
    view.turns
      .filter(
        (turn) =>
          (!turnId || turn.id === turnId) &&
          (turn.status === 'working' || turn.status === 'queued'),
      )
      .map((turn) => turn.id),
  );
  if (turnId && !targets.size) return view;
  const stillWorking = view.turns.some(
    (turn) => turn.status === 'working' && !targets.has(turn.id),
  );
  return {
    ...view,
    status: stillWorking ? view.status : status === 'failed' ? 'failed' : 'idle',
    turns: view.turns.map((turn) =>
      targets.has(turn.id)
        ? {
            ...turn,
            status,
            endedMs: ms || turn.startedMs,
            durationMs: Math.max(0, ms - turn.startedMs),
            error,
          }
        : turn,
    ),
    entries: view.entries.map((entry) =>
      isText(entry) && entry.streaming && (!entry.turnId || targets.has(entry.turnId))
        ? { ...entry, streaming: false, endedMs: ms || entry.startedMs }
        : entry,
    ),
    activeProviderRequests: turnId
      ? new Set(
          [...view.activeProviderRequests].filter(
            (id) => !targets.has(view.providerTurns[id] ?? ''),
          ),
        )
      : new Set(),
    permissions: turnId
      ? view.permissions.filter(
          (permission) =>
            !!permission.tool_call_id &&
            !targets.has(view.tools[permission.tool_call_id]?.turnId ?? ''),
        )
      : [],
    questionRequests: turnId
      ? view.questionRequests.filter(
          (question) =>
            !!question.toolCallId && !targets.has(view.tools[question.toolCallId]?.turnId ?? ''),
        )
      : [],
    tools: Object.fromEntries(
      Object.entries(view.tools).map(([id, tool]) => {
        if (!targets.has(tool.turnId) || !['queued', 'running', 'waiting'].includes(tool.status))
          return [id, tool];
        const changed = {
          ...tool,
          status: status === 'failed' ? ('failed' as const) : ('cancelled' as const),
          endedMs: ms,
          durationMs: Math.max(0, ms - (tool.startedMs ?? ms)),
        };
        return [id, { ...changed, presentation: presentation(changed) }];
      }),
    ),
  };
}

function reduceEvent(
  initial: SessionView,
  envelope: EventEnvelopeV1 | LiveEventEnvelope,
): SessionView {
  let view = initial;
  const event = envelope.payload;
  const agentId = 'agent_id' in envelope.actor ? envelope.actor.agent_id : undefined;
  if (
    view.rootAgentId &&
    agentId &&
    agentId !== view.rootAgentId &&
    /^(user_message_|provider_|assistant_message_|tool_call_|task_|eval_|session_title_updated$)/.test(
      event.event_type,
    )
  )
    return view;
  const ms = envelope.mono_ms,
    seq = 'seq' in envelope ? envelope.seq : undefined;
  const base = { id: envelope.event_id, startedMs: ms, timestamp: envelope.ts ?? undefined, seq };
  const correlation = envelope.correlation_id ?? undefined;
  const requestTurn = (requestId?: string): string =>
    (requestId && view.providerTurns[requestId]) ||
    correlation ||
    requestId ||
    view.turns.at(-1)?.id ||
    envelope.event_id;
  const ensureTurn = (id: string, requestId?: string): void => {
    const found = view.turns.find((turn) => turn.id === id);
    if (!found)
      view = {
        ...view,
        turns: [
          ...view.turns,
          {
            id,
            requestIds: requestId ? [requestId] : [],
            userIds: [],
            status: 'working',
            startedMs: ms,
          },
        ],
      };
    else if (requestId && !found.requestIds.includes(requestId))
      view = {
        ...view,
        turns: view.turns.map((turn) =>
          turn === found ? { ...turn, requestIds: [...turn.requestIds, requestId] } : turn,
        ),
      };
  };
  const closeReasoning = (): void => {
    const last = view.entries.at(-1);
    if (last?.kind === 'reasoning' && last.streaming)
      view = {
        ...view,
        entries: view.entries.map((entry) =>
          entry === last ? { ...last, streaming: false, endedMs: ms } : entry,
        ),
      };
  };
  const updateTool = (id: string, change: Partial<ToolActivity>): void => {
    const previous = view.tools[id];
    const turnId = previous?.turnId ?? requestTurn();
    ensureTurn(turnId);
    const tool = {
      id,
      turnId,
      toolId: '',
      args: {},
      argsSummary: '',
      status: 'queued' as const,
      permissions: [],
      artifacts: [],
      ...previous,
      ...change,
    };
    view = {
      ...view,
      tools: { ...view.tools, [id]: { ...tool, presentation: presentation(tool) } },
    };
    if (!previous) {
      closeReasoning();
      view = {
        ...view,
        entries: [
          ...view.entries,
          {
            ...base,
            id: `tool:${id}`,
            kind: 'tool',
            toolCallId: id,
            turnId,
            requestId: tool.requestId,
          },
        ],
      };
    }
  };
  const appendDelta = (requestId: string, delta: string, kind: TextEntry['kind']): void => {
    if (
      view.settledRequests.has(requestId) ||
      requestId.startsWith('eval-completion') ||
      (seq === undefined && !view.activeProviderRequests.has(requestId))
    )
      return;
    const turnId = requestTurn(requestId);
    ensureTurn(turnId, requestId);
    if (kind !== 'reasoning') closeReasoning();
    const last = view.entries.at(-1);
    const entries =
      last && last.kind === kind && isText(last) && last.requestId === requestId && last.streaming
        ? [...view.entries.slice(0, -1), { ...last, text: last.text + delta }]
        : [
            ...view.entries,
            { ...base, kind, turnId, requestId, text: delta, streaming: true } as TextEntry,
          ];
    view = { ...view, entries, status: 'working' };
  };
  switch (event.event_type) {
    case 'run_started':
      return {
        ...view,
        runId: envelope.run_id,
        title: event.data.run_name,
        workspaceRoot: event.data.workspace_root,
        terminal: false,
      };
    case 'session_title_updated':
      return { ...view, title: event.data.title };
    case 'run_finished':
      return settle({ ...view, terminal: true }, 'completed', ms);
    case 'run_failed':
      return settle(
        { ...view, terminal: true, terminalError: event.data.error },
        'failed',
        ms,
        event.data.error,
      );
    case 'user_message_submitted': {
      const data = event.data,
        turnId = correlation ?? data.request_id;
      const steering = turnId !== data.request_id;
      ensureTurn(turnId);
      view = {
        ...view,
        turns: view.turns.map((turn) =>
          turn.id === turnId ? { ...turn, userIds: [...turn.userIds, base.id] } : turn,
        ),
        entries: [
          ...view.entries,
          {
            ...base,
            kind: 'user',
            turnId,
            requestId: data.request_id,
            text: data.text,
            attachments: [],
            fileTags: [],
            agentTags: [],
            resourceTags: [],
            steering,
          },
        ],
        queued: view.queued.filter((item) => item.id !== data.request_id),
        status: 'working',
      };
      return view;
    }
    case 'prompt_attachments_submitted':
      return {
        ...view,
        entries: view.entries.map((entry) =>
          entry.kind === 'user' && entry.requestId === event.data.request_id
            ? { ...entry, attachments: event.data.attachments }
            : entry,
        ),
        queued: view.queued.map((item) =>
          item.id === event.data.request_id
            ? { ...item, attachments: event.data.attachments }
            : item,
        ),
      };
    case 'steering_accepted':
      return {
        ...view,
        queued: [
          ...view.queued.filter((item) => item.id !== event.data.request_id),
          {
            id: event.data.request_id,
            text: event.data.text,
            turnId: event.data.turn_id,
            state: 'steering',
            attachments: [],
          },
        ],
      };
    case 'provider_request_started': {
      if (event.data.request_id.startsWith('eval-completion')) return view;
      const turnId = event.data.metadata?.turn_id ?? correlation ?? event.data.request_id;
      ensureTurn(turnId, event.data.request_id);
      const budget = event.data.metadata?.context_budget;
      return {
        ...view,
        providerTurns: { ...view.providerTurns, [event.data.request_id]: turnId },
        activeProviderRequests: new Set([...view.activeProviderRequests, event.data.request_id]),
        status: 'working',
        ...(budget
          ? {
              context: {
                occupiedTokens: budget.occupied_input_tokens,
                maximumInputTokens: budget.maximum_input_tokens,
                remainingTokens: budget.remaining_input_tokens,
                budget,
                windowTokens:
                  event.data.metadata?.runtime_selection?.resolved_limits.context_window.tokens,
              },
            }
          : {}),
        queued: view.queued.filter((item) => item.id !== turnId),
        retries: Object.fromEntries(Object.entries(view.retries).filter(([id]) => id !== turnId)),
      };
    }
    case 'provider_text_delta':
    case 'provider_stream_delta':
      appendDelta(event.data.request_id, event.data.delta, 'text');
      return view;
    case 'provider_reasoning_delta':
      appendDelta(event.data.request_id, event.data.delta, 'reasoning');
      return view;
    case 'provider_tool_input_delta':
      closeReasoning();
      updateTool(event.data.tool_call_id, {
        toolId: event.data.tool_name ?? view.tools[event.data.tool_call_id]?.toolId ?? '',
        inputDelta: (view.tools[event.data.tool_call_id]?.inputDelta ?? '') + event.data.delta,
        requestId: event.data.request_id,
      });
      return view;
    case 'provider_retrying':
      return {
        ...view,
        retries: {
          ...view.retries,
          [event.data.turn_id]: { retry: event.data.retry, startedMs: ms },
        },
      };
    case 'provider_request_finished': {
      const requestId = event.data.request_id;
      if (requestId.startsWith('eval-completion')) return view;
      closeReasoning();
      const active = new Set(view.activeProviderRequests);
      active.delete(requestId);
      const parked =
        !active.size && Object.values(view.subagents).some((row) => row.background && !row.endedMs);
      return {
        ...view,
        activeProviderRequests: active,
        status: view.permissions.length ? 'waiting' : parked ? 'background' : view.status,
        entries: view.entries.map((entry) =>
          isText(entry) && entry.requestId === requestId && entry.streaming
            ? { ...entry, streaming: false, endedMs: ms }
            : entry,
        ),
        ...(event.data.usage
          ? {
              usage: event.data.usage,
              usageByRequest: { ...view.usageByRequest, [requestId]: event.data.usage },
            }
          : {}),
      };
    }
    case 'assistant_message_finished': {
      const data = event.data,
        turnId = requestTurn(data.request_id);
      ensureTurn(turnId, data.request_id);
      closeReasoning();
      if (data.parts.length) {
        const old = view.entries.filter(
          (entry) => isText(entry) && entry.requestId === data.request_id,
        );
        let insertion = view.entries.findIndex(
          (entry) =>
            (isText(entry) || entry.kind === 'tool') && entry.requestId === data.request_id,
        );
        if (insertion < 0) insertion = view.entries.length;
        const partTools = new Set(
          data.parts.flatMap((part) => (part.kind === 'tool_call' ? [part.tool_call_id] : [])),
        );
        const entries = view.entries.filter(
          (entry) =>
            !(isText(entry) && entry.requestId === data.request_id) &&
            !(entry.kind === 'tool' && partTools.has(entry.toolCallId)),
        );
        const parts: TranscriptEntry[] = [];
        const used = new Set<string>();
        for (const part of data.parts) {
          if (part.kind === 'tool_call') {
            updateTool(part.tool_call_id, {
              toolId: part.tool_id,
              argsSummary: part.args_summary,
              args: parse(part.args_summary),
              requestId: data.request_id,
            });
            parts.push({
              ...base,
              id: `tool:${part.tool_call_id}`,
              kind: 'tool',
              toolCallId: part.tool_call_id,
              turnId,
              requestId: data.request_id,
            });
          } else {
            const prior = old.find((entry) => entry.kind === part.kind && !used.has(entry.id));
            if (prior) used.add(prior.id);
            parts.push({
              ...base,
              id: prior?.id ?? `${base.id}:${parts.length}`,
              kind: part.kind,
              turnId,
              requestId: data.request_id,
              text: part.text,
              streaming: false,
              startedMs: prior?.startedMs ?? ms,
              endedMs: prior && isText(prior) ? (prior.endedMs ?? ms) : ms,
            });
          }
        }
        entries.splice(Math.min(insertion, entries.length), 0, ...parts);
        view = { ...view, entries };
      } else
        view = {
          ...view,
          entries: view.entries.map((entry) =>
            isText(entry) && entry.requestId === data.request_id
              ? { ...entry, streaming: false, endedMs: entry.endedMs ?? ms }
              : entry,
          ),
        };
      return { ...view, settledRequests: new Set([...view.settledRequests, data.request_id]) };
    }
    case 'tool_call_requested':
      closeReasoning();
      updateTool(event.data.tool_call_id, {
        toolId: event.data.tool_id,
        argsSummary: event.data.args_summary,
        args: parse(event.data.args_summary),
        metadata: event.data.metadata,
      });
      return view;
    case 'tool_call_started':
      updateTool(event.data.tool_call_id, { status: 'running', startedMs: ms });
      return { ...view, status: 'working' };
    case 'tool_call_finished':
    case 'eval_cell_finished': {
      const data = event.data,
        previous = view.tools[data.tool_call_id];
      updateTool(data.tool_call_id, {
        status: data.status,
        endedMs: data.metadata?.timing?.finished_mono_ms ?? ms,
        durationMs:
          data.metadata?.timing?.elapsed_ms ?? Math.max(0, ms - (previous?.startedMs ?? ms)),
        output: data.output_json ?? undefined,
        outputSummary: data.output_summary ?? undefined,
        metadata: data.metadata ?? previous?.metadata,
      });
      if (
        data.status === 'succeeded' &&
        /^(todo\.(read|write)|todoread|todowrite)$/.test(view.tools[data.tool_call_id]!.toolId)
      ) {
        view = {
          ...view,
          todos:
            todoItems(data.output_json) ??
            todoItems(parse(data.output_summary ?? '')) ??
            todoItems(previous?.args) ??
            [],
        };
      }
      return view;
    }
    case 'permission_requested': {
      const permission = { ...event.data, startedMs: ms };
      view = {
        ...view,
        permissions: [
          ...view.permissions.filter((item) => item.permission_id !== permission.permission_id),
          permission,
        ],
        status: 'waiting',
      };
      if (permission.tool_call_id)
        updateTool(permission.tool_call_id, {
          status: 'waiting',
          permissions: [...(view.tools[permission.tool_call_id]?.permissions ?? []), permission],
        });
      if (['question', 'ask', 'ask_user'].includes(permission.kind.toLowerCase())) {
        if (permission.tool_call_id && !view.tools[permission.tool_call_id]?.toolId)
          updateTool(permission.tool_call_id, {
            toolId: 'user.question',
            args: parse(permission.summary),
            argsSummary: permission.summary,
          });
        const args = object(view.tools[permission.tool_call_id ?? '']?.args),
          summary = object(parse(permission.summary));
        const questions = array(args.questions ?? summary.questions);
        if (questions.length)
          view = {
            ...view,
            questionRequests: [
              ...view.questionRequests,
              {
                permissionId: permission.permission_id,
                toolCallId: permission.tool_call_id ?? undefined,
                summary: permission.summary,
                questions,
              },
            ],
          };
      }
      return view;
    }
    case 'permission_resolved': {
      const permission = view.permissions.find(
        (item) => item.permission_id === event.data.permission_id,
      );
      view = {
        ...view,
        permissions: view.permissions.filter(
          (item) => item.permission_id !== event.data.permission_id,
        ),
        questionRequests: view.questionRequests.filter(
          (item) => item.permissionId !== event.data.permission_id,
        ),
      };
      if (permission?.tool_call_id)
        updateTool(permission.tool_call_id, {
          status: 'queued',
          permissions: view.tools[permission.tool_call_id]!.permissions.map((item) =>
            item.permission_id === event.data.permission_id
              ? {
                  ...item,
                  decision: event.data.decision,
                  reason: event.data.reason,
                  resolvedMs: ms,
                }
              : item,
          ),
        });
      return { ...view, status: view.permissions.length ? 'waiting' : 'working' };
    }
    case 'task_scheduled': {
      const data = event.data;
      const row = {
        ...view.tasks[data.task_id],
        id: data.task_id,
        state: data.state,
        startedMs: data.state === 'started' ? ms : undefined,
        lineage: data.metadata?.lineage,
      };
      view = { ...view, tasks: { ...view.tasks, [data.task_id]: row } };
      if (data.state === 'queued') {
        const user = view.entries.find(
          (entry) => entry.kind === 'user' && entry.requestId === data.task_id,
        ) as UserEntry | undefined;
        view = {
          ...view,
          turns: view.turns.map((turn) =>
            turn.id === data.task_id ? { ...turn, status: 'queued' } : turn,
          ),
          entries: view.entries.map((entry) =>
            entry === user ? { ...user, queued: true } : entry,
          ),
          ...(user && !view.queued.some((item) => item.id === data.task_id)
            ? {
                queued: [
                  ...view.queued,
                  {
                    id: data.task_id,
                    text: user.text,
                    attachments: user.attachments,
                    state: 'queued' as const,
                  },
                ],
              }
            : {}),
        };
      } else {
        ensureTurn(data.task_id);
        view = {
          ...view,
          turns: view.turns.map((turn) =>
            turn.id === data.task_id ? { ...turn, status: 'working', startedMs: ms } : turn,
          ),
          entries: view.entries.map((entry) =>
            entry.kind === 'user' && entry.requestId === data.task_id && entry.queued
              ? { ...entry, queued: false }
              : entry,
          ),
          queued: view.queued.filter((item) => item.id !== data.task_id),
          status: 'working',
        };
      }
      return view;
    }
    case 'task_completed':
    case 'task_cancelled': {
      const data = event.data,
        completed = event.event_type === 'task_completed';
      const scope = 'metadata' in data ? data.metadata?.task_scope : data.task_scope;
      const failure = 'failure' in data && data.failure;
      const summary = 'result_summary' in data ? data.result_summary : data.reason;
      view = {
        ...view,
        tasks: {
          ...view.tasks,
          [data.task_id]: {
            ...view.tasks[data.task_id],
            id: data.task_id,
            state: completed ? 'completed' : failure ? 'failed' : 'cancelled',
            endedMs: ms,
            summary,
            scope,
          },
        },
        queued: view.queued.filter((item) => item.id !== data.task_id),
      };
      return scope === 'agent_turn' || view.turns.some((turn) => turn.id === data.task_id)
        ? settle(
            view,
            completed ? 'completed' : failure ? 'failed' : 'interrupted',
            ms,
            completed ? undefined : summary,
            data.task_id,
          )
        : view;
    }
    case 'stale_detected':
      return {
        ...view,
        tasks: {
          ...view.tasks,
          [event.data.task_id]: {
            ...view.tasks[event.data.task_id],
            id: event.data.task_id,
            state: 'stale',
            data: event.data,
          },
        },
      };
    case 'task_result_late':
      return notice(
        view,
        base.id,
        'warning',
        t('transcript.late_result', { task: event.data.task_id }),
        ms,
        'task',
        seq,
        { key: 'transcript.late_result', vars: { task: event.data.task_id } },
      );
    case 'agent_spawned':
      return {
        ...view,
        rootAgentId: event.data.parent_agent_id ? view.rootAgentId : event.data.agent_id,
        subagents: {
          ...view.subagents,
          [event.data.agent_id]: {
            id: event.data.agent_id,
            state: 'running',
            startedMs: ms,
            summary: event.data.profile,
            lineage: event.data.parent_agent_id,
          },
        },
      };
    case 'agent_stopped':
      return {
        ...view,
        subagents: {
          ...view.subagents,
          [event.data.agent_id]: {
            ...view.subagents[event.data.agent_id],
            id: event.data.agent_id,
            state: 'stopped',
            endedMs: ms,
            summary: event.data.reason,
          },
        },
      };
    case 'background_task_notification':
      return {
        ...view,
        tasks: {
          ...view.tasks,
          [event.data.task_id]: {
            ...view.tasks[event.data.task_id],
            id: event.data.task_id,
            state: event.data.status,
            endedMs: ms,
            summary: event.data.summary,
            lineage: event.data,
          },
        },
      };
    case 'eval_progress':
      updateTool(event.data.tool_call_id, {
        outputSummary: event.data.output,
        output: event.data.details,
      });
      return view;
    case 'runtime_warning':
      return notice(view, base.id, 'warning', event.data.message, ms, 'runtime', seq);
    case 'runtime_notice':
      return notice(
        view,
        base.id,
        event.data.severity,
        event.data.message,
        ms,
        event.data.source,
        seq,
      );
    case 'policy_violation_detected':
      return notice(view, base.id, 'warning', event.data.detail, ms, event.data.policy, seq);
    case 'runtime_reminder':
      return notice(view, base.id, 'info', event.data.text, ms, event.data.kind, seq);
    case 'compaction_lifecycle':
    case 'compaction_status':
    case 'compaction_progress': {
      const key =
        event.event_type === 'compaction_lifecycle'
          ? 'lifecycle'
          : event.event_type === 'compaction_status'
            ? 'status'
            : 'progress';
      return {
        ...view,
        compaction: {
          ...view.compaction,
          [event.data.agent_id]: { ...view.compaction[event.data.agent_id], [key]: event.data },
        },
      };
    }
    case 'session_compaction':
    case 'branch_summary':
    case 'compaction_requested':
    case 'compaction_written':
    case 'compaction_applied':
    case 'compaction_failed': {
      const supersedes =
        event.event_type === 'session_compaction' ? event.data.supersedes_seq : undefined;
      return {
        ...view,
        entries: [
          ...view.entries.map((entry) =>
            entry.kind === 'compaction' && entry.seq === supersedes
              ? { ...entry, superseded: true }
              : entry,
          ),
          {
            ...base,
            kind: 'compaction',
            event,
            superseded: false,
            warning: 'warning' in event.data ? (event.data.warning ?? undefined) : undefined,
          },
        ],
      };
    }
    case 'compaction_patched':
      return {
        ...view,
        entries: view.entries.map((entry) =>
          entry.kind === 'compaction' && entry.seq === event.data.compaction_seq
            ? event.data.patch.kind === 'warning'
              ? { ...entry, warning: event.data.patch.text }
              : {
                  ...entry,
                  readerViews: {
                    ...entry.readerViews,
                    [event.data.patch.reader_key]: event.data.patch,
                  },
                }
            : entry,
        ),
      };
    case 'context_pruned':
      return prune({ ...view, contextPruning: [...view.contextPruning, event.data] }, event.data);
    case 'workspace_snapshot':
      return { ...view, snapshots: [...view.snapshots, event.data] };
    case 'workspace_reverted':
      return { ...view, workspaceReverts: [...view.workspaceReverts, event.data] };
    case 'edit_proposed':
    case 'edit_applied':
    case 'edit_rejected':
      return { ...view, edits: { ...view.edits, [event.data.edit_id]: event.data } };
    case 'artifact_written':
      if (event.data.tool_call_id)
        updateTool(event.data.tool_call_id, {
          artifacts: [...(view.tools[event.data.tool_call_id]?.artifacts ?? []), event.data],
        });
      return { ...view, artifacts: [...view.artifacts, event.data] };
    case 'yolo_mode_changed':
      return { ...view, yolo: event.data.enabled };
    case 'ui_intent_received': {
      const params = event.data.params,
        id = params.request_id ?? correlation;
      if (!id) return view;
      return {
        ...view,
        entries: view.entries.map((entry) =>
          entry.kind === 'user' && entry.requestId === id
            ? {
                ...entry,
                fileTags: array(parse(params.file_tags ?? '[]')),
                agentTags: array(parse(params.agent_tags ?? '[]')),
                resourceTags: array(parse(params.resource_tags ?? '[]')),
              }
            : entry,
        ),
      };
    }
    case 'subagent_transition': {
      const data = event.data,
        id = data.child_id,
        previous = view.subagents[id];
      const terminal = data.transition === 'finished' || data.transition === 'retired';
      const state = terminal
        ? (data.outcome ?? 'completed')
        : data.transition === 'spawned'
          ? 'running'
          : (previous?.state ?? 'running');
      return {
        ...view,
        subagents: {
          ...view.subagents,
          [id]: {
            ...previous,
            id,
            state,
            background: data.metadata.notification_route.background,
            startedMs: previous?.startedMs ?? ms,
            ...(terminal ? { endedMs: ms } : {}),
            lineage: data.metadata,
            data,
          },
        },
      };
    }
    case 'native_subagent_registered': {
      const data = event.data;
      return {
        ...view,
        subagents: {
          ...view.subagents,
          [data.child_id]: {
            id: data.child_id,
            state: 'running',
            background: data.background,
            startedMs: ms,
            summary: data.description,
            lineage: {
              parentTool: data.parent_tool,
              parentRequest: data.parent_request,
              spawner: data.spawner,
            },
            data,
          },
        },
      };
    }
    case 'subagent_cancel_requested': {
      const subagents = { ...view.subagents };
      for (const id of event.data.targets)
        subagents[id] = { ...subagents[id], id, state: 'stopping', data: event.data };
      return { ...view, subagents };
    }
    case 'subagent_progress':
    case 'finalized_agent_state':
    case 'agent_context_initialized':
    case 'agent_execution_context_changed':
    case 'native_subagent_message':
    case 'native_subagent_receipt':
    case 'native_subagent_workspace': {
      const data = object(event.data),
        id = String(
          data.child_id ??
            data.owner_session_id ??
            data.agent_id ??
            data.recipient ??
            correlation ??
            envelope.event_id,
        );
      const previous = view.subagents[id];
      return {
        ...view,
        subagents: {
          ...view.subagents,
          [id]: {
            ...previous,
            id,
            state: previous?.state ?? 'running',
            data: { ...object(previous?.data), [event.event_type]: event.data },
          },
        },
      };
    }
    case 'monitor_state':
      return {
        ...view,
        tasks: {
          ...view.tasks,
          [event.data.task_id]: {
            ...view.tasks[event.data.task_id],
            id: event.data.task_id,
            state: event.data.paused ? 'paused' : 'monitoring',
          },
        },
      };
    case 'conversation_rewound':
    case 'permission_grant_recorded':
    case 'context_notes':
    case 'command_notice':
      return view;
    default:
      return view;
  }
}

function replaceUtf8(text: string, start: number, end: number, replacement: string): string {
  const bytes = new TextEncoder().encode(text);
  if (
    start > end ||
    end > bytes.length ||
    (start < bytes.length && (bytes[start]! & 0xc0) === 0x80) ||
    (end < bytes.length && (bytes[end]! & 0xc0) === 0x80)
  )
    return text;
  const decoder = new TextDecoder();
  return decoder.decode(bytes.slice(0, start)) + replacement + decoder.decode(bytes.slice(end));
}
function prune(view: SessionView, data: Data<'context_pruned'>): SessionView {
  for (const edit of data.edits) {
    if (edit.kind === 'drop_attachments' && edit.tool_call_id) {
      const old = view.tools[edit.tool_call_id];
      if (old?.metadata)
        view = {
          ...view,
          tools: {
            ...view.tools,
            [old.id]: { ...old, metadata: { ...old.metadata, attachments: [] } },
          },
        };
    }
    if (edit.kind === 'tool_result') {
      const source =
        edit.seq === undefined ? undefined : view.events.find((event) => event.seq === edit.seq);
      const call =
        source?.payload.event_type === 'assistant_message_finished'
          ? source.payload.data.parts.find(
              (part): part is Extract<AssistantPart, { kind: 'tool_call' }> =>
                part.kind === 'tool_call' &&
                (part.provider_tool_call_id ?? part.tool_call_id) === edit.tool_call_id,
            )
          : undefined;
      const id = call?.tool_call_id ?? edit.tool_call_id,
        old = view.tools[id];
      if (old) {
        const tool = {
          ...old,
          output: undefined,
          outputSummary: edit.text,
          ...(edit.drop_attachments
            ? { metadata: old.metadata ? { ...old.metadata, attachments: [] } : old.metadata }
            : {}),
        };
        view = {
          ...view,
          tools: { ...view.tools, [id]: { ...tool, presentation: presentation(tool) } },
        };
      }
    } else
      view = {
        ...view,
        entries: view.entries.flatMap((entry) => {
          if (entry.seq !== edit.seq) return [entry];
          if (edit.kind === 'drop_reasoning' && entry.kind === 'reasoning') return [];
          if (edit.kind === 'text_range' && (isText(entry) || entry.kind === 'user'))
            return [{ ...entry, text: replaceUtf8(entry.text, edit.start, edit.end, edit.text) }];
          if (edit.kind === 'drop_attachments' && entry.kind === 'user')
            return [{ ...entry, attachments: [] }];
          return [entry];
        }),
      };
  }
  return view;
}

function todoItems(value: unknown): TodoItem[] | undefined {
  if (typeof value === 'string') {
    const parsed = parse(value);
    return typeof parsed === 'string' ? undefined : todoItems(parsed);
  }
  const record = object(value),
    items = Array.isArray(value) ? value : Array.isArray(record.todos) ? record.todos : undefined;
  if (items)
    return items.flatMap((item) => {
      const row = object(item),
        content = row.content ?? row.text ?? row.title;
      if (typeof content !== 'string' || !content.trim()) return [];
      const state = row.status ?? row.state ?? (row.done === true ? 'completed' : 'pending');
      const status: TodoItem['status'] =
        state === 'in_progress' || state === 'completed' || state === 'cancelled'
          ? state
          : 'pending';
      return [{ content: content.trim().replace(/\s+/g, ' '), status }];
    });
  for (const key of ['structured_output', 'metadata', 'output', 'result']) {
    if (record[key] !== undefined) {
      const nested = todoItems(record[key]);
      if (nested) return nested;
    }
  }
  return undefined;
}

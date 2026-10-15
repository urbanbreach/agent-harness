import { describe, expect, test } from 'bun:test';
import {
  activeEvents,
  applyUpdate,
  emptySessionView,
  projectEvents,
} from '../../src/model/session';
import type {
  EventEnvelopeV1,
  EventV1,
  LiveEventEnvelope,
  LiveEventV1,
} from '../../src/model/harness-events';

function event(seq: number, payload: EventV1, correlation = 'turn'): EventEnvelopeV1 {
  return {
    schema_version: 1,
    seq,
    event_id: `event-${seq}`,
    run_id: 'synthetic-run',
    mono_ms: seq * 100,
    ts: '2026-01-01T12:00:00Z',
    actor: { kind: 'user' },
    correlation_id: correlation,
    payload,
  };
}
function live(payload: LiveEventV1, ms = 350): LiveEventEnvelope {
  return {
    event_id: `live-${ms}`,
    run_id: 'synthetic-run',
    mono_ms: ms,
    ts: null,
    actor: { kind: 'supervisor' },
    correlation_id: 'turn',
    causation_id: null,
    stream_key: null,
    payload,
  };
}
const prompt = event(1, {
  event_type: 'user_message_submitted',
  data: { request_id: 'turn', text: 'Explain this synthetic example' },
});
const start = event(2, {
  event_type: 'provider_request_started',
  data: {
    request_id: 'provider',
    provider_id: 'synthetic',
    model_id: 'model',
    prompt_summary: '',
    request_digest: '',
    metadata: null,
  },
});
const answer = event(3, {
  event_type: 'assistant_message_finished',
  data: {
    request_id: 'provider',
    tool_call_count: 0,
    parts: [{ kind: 'text', text: 'An example answer.' }],
    provenance: null,
    assistant_message: null,
  },
});

describe('session projection', () => {
  test('plain exchange settles a reusable turn and drops duplicate durable delivery without changing references', () => {
    const view = projectEvents([
      prompt,
      start,
      answer,
      event(4, {
        event_type: 'task_completed',
        data: { task_id: 'turn', result_summary: 'done', result_digest: '', metadata: null },
      }),
    ]);
    expect(view.entries.map((entry) => entry.kind)).toEqual(['user', 'text']);
    expect(view.status).toBe('idle');
    expect(view.turns[0]?.status).toBe('completed');
    expect(view.turns[0]?.durationMs).toBe(300);
    expect(applyUpdate(view, { kind: 'event', delivery: 'durable', event: answer })).toBe(view);
    expect(
      projectEvents([prompt, start, answer, answer]).entries.filter(
        (entry) => entry.kind === 'text',
      ),
    ).toHaveLength(1);
  });

  test('streaming phases preserve order and durable commit replaces rather than duplicates deltas', () => {
    let view = projectEvents([prompt, start]);
    for (const payload of [
      {
        event_type: 'provider_reasoning_delta',
        data: { request_id: 'provider', delta: 'Consider it.' },
      },
      { event_type: 'provider_text_delta', data: { request_id: 'provider', delta: 'An ' } },
      { event_type: 'provider_text_delta', data: { request_id: 'provider', delta: 'example' } },
    ] satisfies LiveEventV1[])
      view = applyUpdate(view, { kind: 'event', delivery: 'live', event: live(payload) });
    expect(view.entries.map((entry) => entry.kind)).toEqual(['user', 'reasoning', 'text']);
    expect(view.entries.at(-1)).toMatchObject({ text: 'An example', streaming: true });
    expect(view.entries[1]).toMatchObject({ streaming: false, endedMs: 350 });
    view = applyUpdate(view, {
      kind: 'event',
      delivery: 'durable',
      event: event(3, {
        event_type: 'assistant_message_finished',
        data: {
          request_id: 'provider',
          tool_call_count: 0,
          parts: [
            { kind: 'reasoning', text: 'Consider it.' },
            { kind: 'text', text: 'An example answer.' },
          ],
          provenance: null,
          assistant_message: null,
        },
      }),
    });
    expect(view.entries.map((entry) => entry.kind)).toEqual(['user', 'reasoning', 'text']);
    expect(view.entries.at(-1)).toMatchObject({ text: 'An example answer.', streaming: false });
    expect(
      applyUpdate(view, {
        kind: 'event',
        delivery: 'live',
        event: live({
          event_type: 'provider_text_delta',
          data: { request_id: 'provider', delta: 'late' },
        }),
      }).entries,
    ).toBe(view.entries);
  });

  test('tool permission lifecycle retains decision, failure output and elapsed time', () => {
    const requested = event(3, {
      event_type: 'tool_call_requested',
      data: {
        tool_call_id: 'call',
        tool_id: 'bash',
        args_summary: '{"command":"false"}',
        args_digest: '',
        metadata: null,
      },
    });
    let view = projectEvents([
      prompt,
      start,
      requested,
      event(4, {
        event_type: 'permission_requested',
        data: {
          permission_id: 'permission',
          kind: 'command',
          tool_call_id: 'call',
          summary: 'Run command',
          request_digest: '',
          timeout_ms: 10000,
          default_decision: 'deny',
        },
      }),
    ]);
    expect(view.status).toBe('waiting');
    expect(view.tools.call?.status).toBe('waiting');
    view = applyUpdate(view, {
      kind: 'event',
      delivery: 'durable',
      event: event(5, {
        event_type: 'permission_resolved',
        data: { permission_id: 'permission', decision: 'allow', reason: null },
      }),
    });
    expect(view.permissions).toEqual([]);
    expect(view.status).toBe('working');
    view = applyUpdate(view, {
      kind: 'event',
      delivery: 'durable',
      event: event(6, { event_type: 'tool_call_started', data: { tool_call_id: 'call' } }),
    });
    view = applyUpdate(view, {
      kind: 'event',
      delivery: 'durable',
      event: event(8, {
        event_type: 'tool_call_finished',
        data: {
          tool_call_id: 'call',
          status: 'failed',
          output_summary: 'Command exited 1',
          output_digest: null,
          output_json: { exit_code: 1 },
          metadata: null,
        },
      }),
    });
    expect(view.tools.call).toMatchObject({
      status: 'failed',
      durationMs: 200,
      outputSummary: 'Command exited 1',
    });
    expect(view.tools.call?.permissions[0]).toMatchObject({ decision: 'allow', resolvedMs: 500 });
    expect(view.entries.filter((entry) => entry.kind === 'tool')).toHaveLength(1);
  });

  test('rewind removes target inclusively and retains later rewind records across repeated branches', () => {
    const second = event(
      4,
      { event_type: 'user_message_submitted', data: { request_id: 'second', text: 'Discard me' } },
      'second',
    );
    const rewind = event(5, {
      event_type: 'conversation_rewound',
      data: { target_seq: 4, request_id: 'second' },
    });
    const replacement = event(
      6,
      {
        event_type: 'user_message_submitted',
        data: { request_id: 'replacement', text: 'Keep me' },
      },
      'replacement',
    );
    const journal = [prompt, start, answer, second, rewind, replacement];
    expect(activeEvents(journal).map((event) => event.seq)).toEqual([1, 2, 3, 5, 6]);
    const view = projectEvents(journal);
    expect(
      view.entries.filter((entry) => entry.kind === 'user').map((entry) => entry.text),
    ).toEqual(['Explain this synthetic example', 'Keep me']);
    const rewound = applyUpdate(view, {
      kind: 'event',
      delivery: 'durable',
      event: event(7, {
        event_type: 'conversation_rewound',
        data: { target_seq: 1, request_id: 'turn' },
      }),
    });
    expect(rewound.entries).toEqual([]);
    expect(rewound.seenSeq.has(6)).toBe(true);
    expect(
      applyUpdate(rewound, {
        kind: 'event',
        delivery: 'live',
        event: live({
          event_type: 'provider_text_delta',
          data: { request_id: 'provider', delta: 'stale branch text' },
        }),
      }).entries,
    ).toBe(rewound.entries);
  });

  test('withdrawn queued input disappears retroactively without dropping unrelated messages', () => {
    const queued = event(
      4,
      {
        event_type: 'user_message_submitted',
        data: { request_id: 'queued', text: 'Never delivered' },
      },
      'queued',
    );
    let view = projectEvents([
      prompt,
      start,
      queued,
      event(
        5,
        {
          event_type: 'task_scheduled',
          data: { task_id: 'queued', state: 'queued', queue_key: null, metadata: null },
        },
        'queued',
      ),
    ]);
    expect(view.queued.map((item) => item.text)).toEqual(['Never delivered']);
    view = applyUpdate(view, {
      kind: 'event',
      delivery: 'durable',
      event: event(
        6,
        {
          event_type: 'task_cancelled',
          data: {
            task_id: 'queued',
            reason: 'returned to the editor',
            failure: false,
            task_scope: 'agent_turn',
          },
        },
        'queued',
      ),
    });
    expect(view.queued).toEqual([]);
    expect(
      view.entries.filter((entry) => entry.kind === 'user').map((entry) => entry.text),
    ).toEqual(['Explain this synthetic example']);
  });

  test('compaction cards remain anchored and live progress is independent of durable history', () => {
    const compact = event(4, {
      event_type: 'branch_summary',
      data: {
        agent_id: 'root',
        summary: 'Synthetic branch summary',
        from_event_seq: 1,
        read_files: [],
        modified_files: [],
        from_hook: false,
      },
    });
    const view = projectEvents([prompt, start, answer, compact]);
    expect(view.entries.at(-1)).toMatchObject({ kind: 'compaction', seq: 4, superseded: false });
    const next = applyUpdate(view, {
      kind: 'event',
      delivery: 'live',
      event: live({
        event_type: 'compaction_progress',
        data: { agent_id: 'root', generation: 1, trigger_reason: 'manual', preview: 'Summary' },
      }),
    });
    expect(next.compaction.root?.progress).toMatchObject({ generation: 1, preview: 'Summary' });
    expect(next.entries).toBe(view.entries);
    expect(next.events).toBe(view.events);
  });

  test('question permissions recover orphan payloads and latest todo outputs normalize statuses', () => {
    let view = projectEvents([
      prompt,
      start,
      event(3, {
        event_type: 'permission_requested',
        data: {
          permission_id: 'question',
          kind: 'question',
          tool_call_id: 'ask',
          summary:
            '{"questions":[{"question":"Which synthetic choice?","options":[{"label":"A"}]}]}',
          request_digest: '',
          timeout_ms: 1000,
          default_decision: 'deny',
        },
      }),
    ]);
    expect(view.questionRequests[0]?.questions).toEqual([
      { question: 'Which synthetic choice?', options: [{ label: 'A' }] },
    ]);
    view = applyUpdate(view, {
      kind: 'event',
      delivery: 'durable',
      event: event(4, {
        event_type: 'permission_resolved',
        data: { permission_id: 'question', decision: 'allow', reason: '{"answers":[["A"]]}' },
      }),
    });
    expect(view.questionRequests).toEqual([]);
    const todos = projectEvents([
      prompt,
      start,
      event(3, {
        event_type: 'tool_call_requested',
        data: {
          tool_call_id: 'todo',
          tool_id: 'todo.write',
          args_summary: '{"todos":[{"content":"outdated","status":"pending"}]}',
          args_digest: '',
          metadata: null,
        },
      }),
      event(4, {
        event_type: 'tool_call_finished',
        data: {
          tool_call_id: 'todo',
          status: 'succeeded',
          output_summary: null,
          output_digest: null,
          metadata: null,
          output_json: { result: '{"todos":[{"title":" Finish  task ","done":true}]}' },
        },
      }),
    ]);
    expect(todos.todos).toEqual([{ content: 'Finish task', status: 'completed' }]);
  });

  test('context pruning uses UTF-8 byte offsets rather than JavaScript character positions', () => {
    const user = event(1, {
      event_type: 'user_message_submitted',
      data: { request_id: 'turn', text: 'A😀B' },
    });
    const view = projectEvents([
      user,
      event(2, {
        event_type: 'context_pruned',
        data: {
          agent_id: 'root',
          kind: 'shake',
          tokens_saved: 1,
          artifact_path: null,
          warning: null,
          edits: [{ kind: 'text_range', seq: 1, start: 1, end: 5, text: '[removed]' }],
        },
      }),
    ]);
    expect(view.entries[0]).toMatchObject({ text: 'A[removed]B' });
    expect(view.contextPruning).toHaveLength(1);
  });
});

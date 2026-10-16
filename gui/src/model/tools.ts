import { t, getLocale, type TranslationKey } from '../i18n';

const taskStateKeys: Record<string, TranslationKey | undefined> = {
  queued: 'transcript.state_queued',
  running: 'transcript.state_running',
  completed: 'transcript.state_completed',
  failed: 'transcript.state_failed',
  cancelled: 'transcript.state_cancelled',
  timed_out: 'transcript.state_timed_out',
  succeeded: 'transcript.state_succeeded',
  done: 'transcript.state_done',
  finished: 'transcript.state_finished',
  stopped: 'transcript.state_stopped',
  waiting: 'transcript.state_waiting',
  stale: 'transcript.state_stale',
};

export function taskStateLabel(state: string, spaced = false): string {
  if (getLocale() === 'en') return spaced ? state.replace(/_/g, ' ') : state;
  const key = taskStateKeys[state];
  return key ? t(key) : state;
}
export type ToolKind =
  | 'read'
  | 'write'
  | 'edit'
  | 'list'
  | 'search'
  | 'command'
  | 'web'
  | 'agent'
  | 'session'
  | 'plan'
  | 'todo'
  | 'question'
  | 'skill'
  | 'lsp'
  | 'eval'
  | 'context'
  | 'github'
  | 'mcp'
  | 'generic';
export type ToolStatus = 'queued' | 'running' | 'succeeded' | 'failed';
export interface SearchGroup {
  path: string;
  rows: { line?: number; text: string; count?: number }[];
}
export interface ToolDiffFile {
  path: string;
  diff?: string;
  artifact?: string;
  before?: string;
  after?: string;
  additions?: number;
  deletions?: number;
}
export type ToolOutput =
  | {
      kind: 'command';
      command: string;
      stdout: string;
      stderr: string;
      text: string;
      exitCode?: number;
      truncated: boolean;
      cwd?: string;
    }
  | { kind: 'diff'; files: ToolDiffFile[]; error?: string }
  | {
      kind: 'search';
      mode: string;
      groups: SearchGroup[];
      total?: number;
      truncated: boolean;
      text?: string;
    }
  | {
      kind: 'web';
      text: string;
      sources: { url: string; title?: string }[];
      metadata: Record<string, unknown>;
    }
  | {
      kind: 'mcp';
      text: string;
      arguments: Record<string, unknown>;
      server?: string;
      error?: string;
      value: unknown;
    }
  | { kind: 'text'; text: string }
  | { kind: 'json'; value: unknown };
export interface ToolPresentation {
  toolId: string;
  kind: ToolKind;
  status: ToolStatus;
  action: string;
  title: string;
  target?: string;
  detail?: string;
  output?: ToolOutput;
}

type ObjectValue = Record<string, unknown>;
function object(value: unknown): ObjectValue {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as ObjectValue)
    : {};
}
function decode(value: unknown): unknown {
  if (typeof value !== 'string') return value;
  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}
function text(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined;
  return value
    .replace(/\u001b\[[0-?]*[ -/]*[@-~]/g, '')
    .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, '');
}
function string(value: ObjectValue, ...keys: string[]): string | undefined {
  for (const key of keys) {
    const result = text(value[key])?.replace(/\s+/g, ' ').trim();
    if (result) return result;
  }
  return undefined;
}
function number(value: ObjectValue, ...keys: string[]): number | undefined {
  for (const key of keys)
    if (typeof value[key] === 'number' && Number.isFinite(value[key])) return value[key] as number;
  return undefined;
}
function array(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}
function leaf(path: string): string {
  return (
    path
      .replace(/[/\\]+$/, '')
      .split(/[/\\]/)
      .pop() || path
  );
}
function humanize(id: string): string {
  if (/\s/.test(id.trim())) return id.trim();
  const name = id.split('.').pop() || id;
  return name
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .replace(/[_-]+/g, ' ')
    .replace(/^./, (c) => c.toUpperCase());
}

const kinds: Record<string, ToolKind> = {};
for (const [kind, ids] of Object.entries({
  read: 'read fs.read edit.hashline_scan',
  write: 'write fs.write',
  edit: 'edit apply_patch edit.hashline_apply ast_grep_replace lsp.rename',
  list: 'list fs.ls',
  search: 'glob fs.glob grep fs.grep ast_grep_search',
  command: 'bash shell.run monitor',
  web: 'webfetch web.fetch websearch search.web codesearch search.code',
  agent:
    'spawn_subagent agent.spawn task get_command_or_subagent_output get_task_output background_output wait_commands_or_subagents wait_tasks kill_command_or_subagent kill_task background_cancel send_subagent_message agent.message agent.send_message',
  session: 'session_list session_read session_search session_info',
  plan: 'plan_enter plan_exit',
  todo: 'todoread todo.read todowrite todo.write',
  question: 'question user.question',
  skill: 'skill skill.load',
  lsp: 'lsp code.lsp',
  eval: 'eval',
  context: 'context_notes new_context',
  github: 'github.issue github.pull_request',
  generic: 'invalid',
}))
  for (const id of ids.split(' ')) kinds[id] = kind as ToolKind;

function diffFiles(
  args: ObjectValue,
  out: ObjectValue,
  meta: ObjectValue,
  target?: string,
): ToolDiffFile[] {
  const files: ToolDiffFile[] = [];
  const add = (value: unknown) => {
    const entry = object(value);
    const path = string(entry, 'path', 'resource', 'filePath') || target;
    if (!path) return;
    const existing = files.find((file) => file.path === path);
    const file: ToolDiffFile = {
      path,
      diff: text(entry.diff) ?? text(entry.patch),
      artifact: string(entry, 'diff_rel_path'),
      before: text(entry.before) ?? text(entry.oldContent),
      after: text(entry.after) ?? text(entry.newContent),
      additions: number(entry, 'additions', 'lines_added'),
      deletions: number(entry, 'deletions', 'lines_removed'),
    };
    if (existing) {
      for (const [key, value] of Object.entries(file))
        if (value !== undefined) Object.assign(existing, { [key]: value });
    } else files.push(file);
  };
  for (const value of array(out.edits)) add(value);
  for (const value of array(out.applied)) add(value);
  if (meta.edit) add(meta.edit);
  if (out.diff || out.diff_rel_path) add(out);
  for (const value of array(out.files)) {
    if (typeof value === 'string') add({ path: value.replace(/^[AMD]\s+/, '') });
    else add(value);
  }
  if (target && (typeof args.content === 'string' || typeof args.newString === 'string')) {
    add({
      path: target,
      before: text(args.oldString) ?? text(args.oldContent) ?? text(args.before) ?? '',
      after: text(args.newString) ?? text(args.content),
    });
  }
  for (const value of array(meta.artifact_refs ?? meta.artifactRefs)) {
    const artifact = string(object(value), 'path');
    if (!artifact?.endsWith('.diff') || files.some((file) => file.artifact === artifact)) continue;
    const file = files.find((file) => !file.artifact);
    if (file) file.artifact = artifact;
    else add({ path: target || artifact, diff_rel_path: artifact });
  }
  return files;
}

function searchOutput(
  out: ObjectValue,
  id: string,
  args: ObjectValue,
  meta: ObjectValue,
): ToolOutput {
  const mode =
    string(out, 'output_mode') ||
    string(args, 'output_mode') ||
    (id.endsWith('glob') ? 'files_with_matches' : 'content');
  const groups: SearchGroup[] = [];
  const add = (path: string, row?: SearchGroup['rows'][number]) => {
    let group = groups.find((group) => group.path === path);
    if (!group) {
      group = { path, rows: [] };
      groups.push(group);
    }
    if (row) group.rows.push(row);
  };
  if (mode === 'files_with_matches') {
    for (const value of array(out.paths ?? out.files)) {
      const path = typeof value === 'string' ? value : string(object(value), 'path', 'file');
      if (path) add(path);
    }
  } else {
    for (const value of array((mode === 'count' ? out.counts : out.matches) ?? out.files)) {
      if (typeof value === 'string') {
        const match = /^(.*?):(\d+):(.*)$/.exec(value);
        if (match?.[1]) add(match[1], { line: Number(match[2]), text: match[3] || '' });
        continue;
      }
      const entry = object(value);
      const path = string(entry, 'file', 'path', 'file_path');
      if (path)
        add(path, {
          line: number(entry, 'line', 'line_number'),
          text: text(entry.text) ?? text(entry.content) ?? text(entry.match) ?? '',
          count: number(entry, 'count'),
        });
    }
  }
  return {
    kind: 'search',
    mode,
    groups,
    total: number(out, 'total_count', 'count'),
    truncated: out.truncated === true,
    text: text(meta.output_summary) ?? text(meta.outputSummary),
  };
}

function projectOutput(
  kind: ToolKind,
  id: string,
  args: ObjectValue,
  output: unknown,
  meta: ObjectValue,
  target: string | undefined,
  failed: boolean,
): ToolOutput | undefined {
  const out = object(output);
  if (kind === 'command')
    return {
      kind: 'command',
      command: text(args.command) || '',
      cwd: string(args, 'workdir', 'cwd'),
      stdout: text(out.stdout) || '',
      stderr: text(out.stderr) || '',
      text:
        typeof output === 'string'
          ? text(output) || ''
          : (text(out.output) ?? text(out.text) ?? ''),
      exitCode: number(out, 'exit_code', 'exitCode'),
      truncated: out.truncated === true,
    };
  if (kind === 'edit' || kind === 'write') {
    const files = diffFiles(args, out, meta, target);
    if (files.length)
      return {
        kind: 'diff',
        files,
        error: failed ? (text(output) ?? string(out, 'error', 'message', 'text')) : undefined,
      };
  }
  if (output === undefined || output === null) return undefined;
  if (kind === 'search' && !failed && typeof output !== 'string')
    return searchOutput(out, id, args, meta);
  if (kind === 'web' && !failed) {
    const sources = array(out.sources ?? out.citations ?? out.results).flatMap((value) => {
      const entry = object(value);
      const url = typeof value === 'string' ? value : string(entry, 'url');
      return url ? [{ url, title: string(entry, 'title') }] : [];
    });
    const metadata: ObjectValue = {};
    for (const key of ['status', 'content_type', 'bytes', 'byte_len', 'numResults'])
      if (out[key] !== undefined) metadata[key] = out[key];
    return {
      kind: 'web',
      text: text(output) ?? text(out.text) ?? text(out.content) ?? text(out.output) ?? '',
      sources,
      metadata,
    };
  }
  if (kind === 'mcp') {
    const payload = object(out.payload);
    const content = array(out.content ?? object(payload.result).content);
    return {
      kind: 'mcp',
      arguments: object(args.arguments ?? args),
      server: string(object(out.server), 'id') || id.split('.')[1],
      value: output,
      text:
        content
          .map(
            (value) => text(object(value).text) ?? text(object(object(value).resource).text) ?? '',
          )
          .filter(Boolean)
          .join('\n') ||
        text(output) ||
        '',
      error: failed
        ? string(out, 'error', 'message') || text(output) || t('transcript.tool_failed')
        : undefined,
    };
  }
  return typeof output === 'string'
    ? { kind: 'text', text: text(output) || '' }
    : { kind: 'json', value: output };
}

/** Pure presentation of recorded tool data; artifact references never trigger I/O. */
export function presentTool(
  toolId: string,
  args: unknown,
  output?: unknown,
  metadata?: unknown,
): ToolPresentation {
  const id = toolId.trim();
  const input = object(decode(args));
  const decoded = decode(output);
  const out = object(decoded);
  const meta = object(metadata);
  const rawStatus = string(meta, 'status');
  const status: ToolStatus =
    rawStatus === 'failed' || rawStatus === 'error' || out.is_error === true
      ? 'failed'
      : rawStatus === 'running' || rawStatus === 'live'
        ? 'running'
        : rawStatus === 'queued' || rawStatus === 'pending_permission'
          ? 'queued'
          : rawStatus === 'succeeded' || rawStatus === 'completed' || output !== undefined
            ? 'succeeded'
            : 'running';
  const failed = status === 'failed';
  const live = status === 'running' || status === 'queued';
  const kind = kinds[id] || (id.startsWith('mcp.') ? 'mcp' : 'generic');
  const custom = string(meta, 'title');
  const path = string(input, 'filePath', 'file_path', 'path');
  const query = string(input, 'pattern', 'query');
  let target = path;
  let detail = custom;
  let title = custom || humanize(id);
  let action = t('transcript.action_tool');
  const state = (active: string, settled: string, failure: string) =>
    failed ? failure : live ? active : settled;
  switch (kind) {
    case 'command': {
      action = t('transcript.action_run');
      const description = string(input, 'description', 'i');
      target = string(input, 'command');
      detail = description || target || custom;
      title = detail
        ? state(
            t('transcript.command_running', {
              detail: `${description ? t('transcript.command_prefix') : ''}${detail}`,
            }),
            t('transcript.command_ran', {
              detail: `${description ? t('transcript.command_prefix') : ''}${detail}`,
            }),
            t('transcript.command_failed_detail', { detail }),
          )
        : state(
            t('transcript.command_running_empty'),
            t('transcript.command_ran_empty'),
            t('transcript.command_failed'),
          );
      break;
    }
    case 'read':
    case 'skill': {
      action = t('transcript.action_read');
      target = kind === 'skill' ? string(input, 'name') : path;
      detail = target ? leaf(target) : custom;
      const subject = detail || t(kind === 'skill' ? 'transcript.skill' : 'transcript.file');
      title = state(
        t('transcript.read_running', { subject }),
        t('transcript.read_done', { subject }),
        t('transcript.read_failed', { subject }),
      );
      const display = object(object(out.metadata).display);
      const start =
        number(out, 'offset') ??
        number(input, 'offset', 'start_line') ??
        number(display, 'lineStart') ??
        1;
      const limit = number(out, 'limit') ?? number(input, 'limit');
      const total = number(out, 'total_lines') ?? number(display, 'totalLines');
      const end =
        number(display, 'lineEnd') ?? (limit && limit > 0 ? start + limit - 1 : undefined);
      if (end !== undefined && end >= start)
        detail = t('transcript.line_range', {
          subject: detail || subject,
          start,
          end: total === undefined ? end : Math.min(end, total),
          total:
            total !== undefined && total > end - start + 1
              ? t('transcript.of_total', { total })
              : '',
        });
      break;
    }
    case 'write':
    case 'edit': {
      action = t('transcript.action_edit');
      const files = diffFiles(input, out, meta, path);
      target = files.length === 1 ? files[0]?.path : path;
      detail =
        files.length > 1
          ? t('transcript.file_count', { count: files.length })
          : target
            ? leaf(target)
            : custom;
      const subject = detail || t('transcript.files');
      title = state(
        t('transcript.edit_running', { subject }),
        t('transcript.edit_done', { subject }),
        t('transcript.edit_failed', { subject }),
      );
      break;
    }
    case 'list':
      action = t('transcript.action_list');
      target = path || '.';
      detail = leaf(target);
      title = state(
        t('transcript.list_running', { subject: detail }),
        t('transcript.list_done', { subject: detail }),
        t('transcript.list_failed', { subject: detail }),
      );
      break;
    case 'search': {
      action = t('transcript.action_search');
      target = query;
      const scope = string(input, 'glob', 'include');
      const term = query ? (scope ? t('transcript.search_scope', { query, scope }) : query) : scope;
      title = state(
        t('transcript.search_running'),
        t('transcript.search_done'),
        t('transcript.search_failed'),
      );
      if (term) title = t('transcript.search_term', { title, term });
      detail = title;
      break;
    }
    case 'web': {
      action = t('transcript.action_search');
      target = string(input, 'query', 'url');
      const fetch = id === 'webfetch' || id === 'web.fetch';
      title = fetch
        ? state(
            t('transcript.fetch_running', { subject: target || t('transcript.web_page') }),
            t('transcript.fetch_done', { subject: target || t('transcript.web_page') }),
            t('transcript.fetch_failed', { subject: target || t('transcript.web_page') }),
          )
        : state(t('transcript.web_running'), t('transcript.web_done'), t('transcript.web_failed'));
      if (!fetch && target) title = t('transcript.search_term', { title, term: target });
      detail = target ? (fetch ? target : t('transcript.search_for', { term: target })) : custom;
      break;
    }
    case 'todo':
    case 'plan':
      action = t('transcript.action_plan');
      title = state(
        t('transcript.plan_running'),
        t('transcript.plan_done'),
        t('transcript.plan_failed'),
      );
      if (id === 'plan_enter')
        title = state(
          t('transcript.plan_enter_running'),
          t('transcript.plan_enter_done'),
          t('transcript.plan_enter_failed'),
        );
      if (id === 'plan_exit')
        title = state(
          t('transcript.plan_exit_running'),
          t('transcript.plan_exit_done'),
          t('transcript.plan_exit_failed'),
        );
      if (id === 'todoread' || id === 'todo.read')
        title = state(
          t('transcript.plan_read_running'),
          t('transcript.plan_read_done'),
          t('transcript.plan_read_failed'),
        );
      detail = custom || string(input, 'goal', 'reason') || title;
      break;
    case 'question':
      action = t('transcript.ask_questions');
      title = t('transcript.ask_questions');
      detail =
        string(input, 'question') ||
        string(object(array(input.questions)[0]), 'question', 'header') ||
        custom;
      break;
    case 'agent':
      target = string(
        input,
        'description',
        'task_id',
        'subagent_id',
        'request_id',
        'session_id',
        'recipient',
      );
      title =
        (
          {
            spawn_subagent: t('transcript.spawn_subagent'),
            'agent.spawn': t('transcript.spawn_subagent'),
            task: t('transcript.spawn_subagent'),
            get_command_or_subagent_output: t('transcript.check_background'),
            get_task_output: t('transcript.check_background'),
            background_output: t('transcript.check_background'),
            wait_commands_or_subagents: t('transcript.wait_background'),
            wait_tasks: t('transcript.wait_background'),
            kill_command_or_subagent: t('transcript.cancel_background'),
            kill_task: t('transcript.cancel_background'),
            background_cancel: t('transcript.cancel_background'),
            send_subagent_message: t('transcript.message_subagent'),
            'agent.message': t('transcript.message_subagent'),
            'agent.send_message': t('transcript.message_subagent'),
          } as Record<string, string>
        )[id] || title;
      action = title;
      detail =
        target ||
        custom ||
        (Array.isArray(input.task_ids) ? input.task_ids.map(String).join(', ') : '');
      break;
    case 'session':
      target = string(input, 'query', 'session_id', 'run_id');
      title = t(
        (
          {
            session_list: 'transcript.session_list',
            session_read: 'transcript.session_read',
            session_search: 'transcript.session_search',
            session_info: 'transcript.session_info',
          } as Record<string, TranslationKey>
        )[id]!,
      );
      detail = target || title;
      break;
    case 'lsp':
      title = `LSP ${string(input, 'operation', 'command') || t('transcript.lsp_request')}`;
      target = path;
      detail = path
        ? `${path}${number(input, 'line') !== undefined ? `:${number(input, 'line')}:${number(input, 'character') ?? 0}` : ''}`
        : title;
      break;
    case 'eval':
      action = t('transcript.evaluate');
      title = string(input, 'summary', 'title', 'i') || t('transcript.evaluate_code');
      target = string(input, 'language');
      detail = target || title;
      break;
    case 'context':
      title = t(id === 'new_context' ? 'transcript.new_context' : 'transcript.context_notes');
      detail = custom || title;
      break;
    case 'github': {
      title = t(id.endsWith('pull_request') ? 'transcript.github_pr' : 'transcript.github_issue');
      const owner = string(input, 'owner');
      const repo = string(input, 'repo');
      const issue = number(input, 'issue_number', 'pull_number');
      target =
        [owner, repo].filter(Boolean).join('/') + (issue !== undefined ? `#${issue}` : '') ||
        undefined;
      detail = [string(input, 'operation'), target].filter(Boolean).join(' ') || title;
      break;
    }
    case 'mcp': {
      const body = id.slice(4);
      const split = body.indexOf('.');
      const server = string(object(out.server), 'id') || (split < 0 ? body : body.slice(0, split));
      const tool =
        string(object(out.payload), 'tool') ||
        string(input, 'tool') ||
        (split < 0 ? '' : body.slice(split + 1));
      title = [server, tool]
        .filter(Boolean)
        .map((part) => part.replace(/[._-]+/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase()))
        .join(' ');
      target = tool || server;
      detail = title;
      break;
    }
    case 'generic':
      target = string(input, 'target');
      detail = target || custom || humanize(id);
      break;
  }
  if (failed && detail?.toLowerCase() === 'failed')
    detail =
      (text(decoded) || string(out, 'error', 'message', 'text'))
        ?.split('\n')
        .find((line) => line.trim()) || detail;
  return {
    toolId: id,
    kind,
    status,
    action,
    title,
    target,
    detail,
    output: projectOutput(kind, id, input, decoded, meta, target, failed),
  };
}

import type { Command, FileEntry, SelectedFileTag, Skill } from '../../backend/protocol';
import { isAbsolute, resolve, basename } from 'node:path';
import { pathToFileURL } from 'node:url';

export interface Trigger {
  kind: '/' | '@';
  start: number;
  end: number;
  query: string;
}
export interface Suggestion {
  value: string;
  label: string;
  description: string;
  scope?: string;
  directory?: boolean;
  local?: boolean;
  agent?: { name: string };
  resource?: { name: string; uri: string; mime: string };
}
const appCommandDescriptions: Record<string, string> = {
  compact: 'Compact the conversation',
  handoff: 'Prepare a handoff',
  shake: 'Prune conversation context',
  rewind: 'Rewind the conversation',
  fork: 'Fork this conversation',
  clone: 'Duplicate this task',
  rename: 'Rename this task',
  export: 'Export Markdown',
  resume: 'Resume a terminal agent session',
  new: 'Start a new task',
  clear: 'Start a new task',
  sessions: 'Find a saved session',
  continue: 'Resume a saved session',
  tree: 'Browse the session tree',
  models: 'Choose a model',
  model: 'Choose a model',
  mo: 'Choose a model',
  agents: 'Choose an agent profile',
  mcps: 'Manage tools and MCP servers',
  toggles: 'Manage tools and MCP servers',
  yolo: 'Toggle Full access',
  login: 'Manage provider sign-in',
  'claude-account': 'Manage Claude accounts',
  help: 'Open command help',
  feedback: 'Open help and feedback',
  follow: 'Jump to the latest message',
  undo: 'Rewind the conversation',
  summarize: 'Compact the conversation',
  exit: 'Quit Harness',
  quit: 'Quit Harness',
  q: 'Quit Harness',
  copy: 'Copy transcript Markdown',
  timestamps: 'Toggle message times',
  thinking: 'Toggle reasoning',
  settings: 'Open settings',
  'view-plan': 'Open the plan',
  worktree: 'Choose a worktree',
  dashboard: 'Open session details',
  status: 'Open session details',
  usage: 'Open usage',
  import: 'Import or resume a session',
};
export const appCommands = Object.keys(appCommandDescriptions);
// These commands only configure terminal input, terminal shells, or the TUI event viewer.
export const terminalCommands: Record<string, true> = {
  vim: true,
  shell: true,
  'session-shell': true,
  events: true,
};
export function triggerAt(text: string, caret: number): Trigger | undefined {
  const before = text.slice(0, caret);
  const match = /(^|\s)([@/])([^\s]*)$/.exec(before);
  if (!match) return;
  const start = match.index + match[1]!.length;
  const kind = match[2] as '/' | '@';
  if (kind === '/' && text.slice(text.lastIndexOf('\n', start - 1) + 1, start).trim()) return;
  const rest = /^\S*/.exec(text.slice(caret))![0];
  return { kind, start, end: caret + rest.length, query: match[3]! };
}
export function fuzzyScore(value: string, query: string): number | undefined {
  value = value.toLocaleLowerCase();
  query = query.toLocaleLowerCase();
  let at = 0;
  let score = 0;
  let previous = -2;
  for (const char of query) {
    const index = value.indexOf(char, at);
    if (index < 0) return;
    score += index === previous + 1 ? 8 : 0;
    score += index === 0 || '/_- .'.includes(value[index - 1]!) ? 12 : 0;
    score -= index - at;
    previous = index;
    at = index + 1;
  }
  return score - value.length / 100;
}
export function suggestions(
  trigger: Trigger,
  commands: Command[],
  files: FileEntry[],
  skills: Skill[] = [],
  agents: { name: string }[] = [],
  resources: { name: string; uri: string; mime: string }[] = [],
): Suggestion[] {
  let rows: Suggestion[];
  if (trigger.kind === '@') {
    rows = files.map((file) => ({
      value: `@${file.path}${file.dir && !file.path.endsWith('/') ? '/' : ''}`,
      label: basename(file.path),
      description: file.path.includes('/') ? file.path.slice(0, file.path.lastIndexOf('/')) : '',
      directory: file.dir,
    }));
    rows.push(
      ...agents.map((agent) => ({
        value: `@agent:${encodeURIComponent(agent.name)}`,
        label: agent.name,
        description: 'Agent',
        agent,
      })),
      ...resources.map((resource) => ({
        value: `@resource:${encodeURIComponent(resource.name)}`,
        label: resource.name,
        description: resource.uri,
        resource,
      })),
    );
  } else {
    const byName = new Map<string, Command>();
    const priority = { builtin: 0, user: 1, project: 2 };
    for (const command of commands) {
      const name = command.name.replace(/^\//, '');
      if (command.scope === 'builtin' && terminalCommands[name]) continue;
      const prior = byName.get(name);
      if (!prior || priority[command.scope] >= priority[prior.scope])
        byName.set(name, { ...command, name });
    }
    rows = [...byName.values()]
      .sort((a, b) => priority[a.scope] - priority[b.scope] || a.name.localeCompare(b.name))
      .map((command) => ({
        value: `/${command.name}`,
        label: `/${command.name}`,
        description: command.description,
        scope: command.scope,
      }));
    for (const name of appCommands)
      if (!byName.has(name))
        rows.push({
          value: `/${name}`,
          label: `/${name}`,
          description: appCommandDescriptions[name]!,
          scope: 'Harness',
          local: true,
        });
    for (const skill of skills
      .filter((entry) => entry.enabled)
      .sort((a, b) => a.name.localeCompare(b.name))) {
      if (!byName.has(skill.name))
        rows.push({
          value: `/${skill.name}`,
          label: `/${skill.name}`,
          description: skill.description,
          scope: 'skill',
        });
    }
  }
  if (!trigger.query) return rows.slice(0, 64);
  return rows
    .map((row, order) => ({ row, order, score: fuzzyScore(row.value.slice(1), trigger.query) }))
    .filter((item) => item.score !== undefined)
    .sort((a, b) => b.score! - a.score! || a.order - b.order)
    .slice(0, 64)
    .map((item) => item.row);
}
export function fileTags(text: string, cwd: string): SelectedFileTag[] {
  const tags: SelectedFileTag[] = [];
  const tokens = /(^|\s)@([^\s]+)/g;
  for (const match of text.matchAll(tokens)) {
    const value = match[2]!;
    if (value.startsWith('agent:') || value.startsWith('resource:')) continue;
    const path = isAbsolute(value) ? value : resolve(cwd, value);
    const start = match.index! + match[1]!.length;
    tags.push({
      path,
      filename: basename(path),
      url: pathToFileURL(path).href,
      mime: value.endsWith('/') ? 'application/x-directory' : 'application/octet-stream',
      source: { start, end: start + value.length + 1, value: `@${value}` },
    });
  }
  return tags;
}

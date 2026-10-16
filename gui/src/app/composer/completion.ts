import type { Command, FileEntry, SelectedFileTag, Skill } from '../../backend/protocol';
import { isAbsolute, resolve, basename } from 'node:path';
import { pathToFileURL } from 'node:url';
import { t, type TranslationKey } from '../../i18n';

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
const appCommandDescriptions: Record<string, TranslationKey> = {
  compact: 'composer.command_compact',
  handoff: 'composer.command_handoff',
  shake: 'composer.prune_context',
  rewind: 'composer.command_rewind',
  fork: 'composer.command_fork',
  clone: 'composer.command_clone',
  rename: 'composer.command_rename',
  export: 'composer.command_export',
  resume: 'commands.resume_description',
  new: 'composer.command_new',
  clear: 'composer.command_new',
  sessions: 'composer.command_sessions',
  continue: 'composer.command_continue',
  tree: 'composer.command_tree',
  models: 'composer.command_model',
  model: 'composer.command_model',
  mo: 'composer.command_model',
  agents: 'composer.command_agents',
  mcps: 'composer.command_mcps',
  toggles: 'composer.command_mcps',
  yolo: 'composer.command_yolo',
  login: 'composer.command_login',
  'claude-account': 'composer.command_claude_account',
  help: 'composer.command_help',
  feedback: 'composer.command_feedback',
  follow: 'composer.command_follow',
  undo: 'composer.command_rewind',
  summarize: 'composer.command_compact',
  exit: 'composer.command_quit',
  quit: 'composer.command_quit',
  q: 'composer.command_quit',
  copy: 'composer.command_copy',
  timestamps: 'composer.command_timestamps',
  thinking: 'composer.command_thinking',
  settings: 'composer.command_settings',
  'view-plan': 'composer.command_plan',
  worktree: 'composer.command_worktree',
  dashboard: 'composer.command_status',
  status: 'composer.command_status',
  usage: 'composer.command_usage',
  import: 'composer.command_import',
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
        description: t('composer.agent'),
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
          description: t(appCommandDescriptions[name]!),
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
export function fileTag(value: string, cwd: string, start: number): SelectedFileTag {
  const path = isAbsolute(value) ? value : resolve(cwd, value);
  return {
    path,
    filename: basename(path),
    url: pathToFileURL(path).href,
    mime: value.endsWith('/') ? 'application/x-directory' : 'application/octet-stream',
    source: { start, end: start + value.length + 1, value: `@${value}` },
  };
}
export function fileTags(
  text: string,
  cwd: string,
  selected: readonly SelectedFileTag[] = [],
): SelectedFileTag[] {
  const tags: SelectedFileTag[] = [];
  const tokens = /(^|\s)@([^\s]+)/g;
  for (const match of text.matchAll(tokens)) {
    const offset = match.index! + match[1]!.length;
    if (tags.some((tag) => offset < tag.source.end)) continue;
    const picked = selected
      .filter((tag) => text.startsWith(tag.source.value, offset))
      .filter(
        (tag) =>
          !text[offset + tag.source.value.length] ||
          /\s/.test(text[offset + tag.source.value.length]!),
      )
      .sort((a, b) => b.source.value.length - a.source.value.length)[0];
    const value = picked ? picked.source.value.slice(1) : match[2]!;
    if (value.startsWith('agent:') || value.startsWith('resource:')) continue;
    const tag = fileTag(value, cwd, offset);
    tags.push(picked ? { ...picked, source: tag.source } : tag);
  }
  return tags;
}

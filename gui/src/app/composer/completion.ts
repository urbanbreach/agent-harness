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
};
export const appCommands = Object.keys(appCommandDescriptions);
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
): Suggestion[] {
  let rows: Suggestion[];
  if (trigger.kind === '@') {
    rows = files.map((file) => ({
      value: `@${file.path}${file.dir && !file.path.endsWith('/') ? '/' : ''}`,
      label: basename(file.path),
      description: file.path.includes('/') ? file.path.slice(0, file.path.lastIndexOf('/')) : '',
      directory: file.dir,
    }));
  } else {
    const byName = new Map<string, Command>();
    const priority = { builtin: 0, user: 1, project: 2 };
    for (const command of commands) {
      const name = command.name.replace(/^\//, '');
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

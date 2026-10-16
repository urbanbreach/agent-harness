import type { DiffFile, ReviewSnapshot } from '../../workspace/git';

export type DiffLine =
  | {
      kind: 'context' | 'added' | 'removed' | 'hunk' | 'meta';
      text: string;
      old?: number;
      next?: number;
    }
  | {
      kind: 'gap';
      text: string;
      id: string;
      lines: DiffLine[];
      leading: boolean;
      trailing: boolean;
    };
export interface ReviewFile extends DiffFile {
  status: 'A' | 'M' | 'D' | 'B';
  lines: DiffLine[];
}
export type ReviewData = ReviewSnapshot & { statuses?: Record<string, 'A' | 'M' | 'D'> };

/** Git emits files in summary order; headers, not +/- content, determine A/D status. */
export function parseReviewPatch(snapshot: ReviewData): ReviewFile[] {
  const sections = snapshot.patch.split(/^diff --git /m).slice(1);
  return snapshot.files.map((file, index) => {
    const section = sections[index] ?? '';
    const lines: DiffLine[] = [];
    let old = 0;
    let next = 0;
    let inHunk = false;
    for (const text of section.split('\n').slice(1)) {
      const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(text);
      if (hunk) {
        const start = Number(hunk[1]);
        const missing = start - (inHunk ? old : 1);
        if (missing > 1)
          lines.push({
            kind: 'gap',
            id: `missing:${old}:${start}`,
            text: missing.toLocaleString(),
            lines: [],
            leading: !inHunk,
            trailing: false,
          });
        old = Number(hunk[1]);
        next = Number(hunk[2]);
        inHunk = true;
        lines.push({ kind: 'hunk', text });
      } else if (inHunk && text.startsWith('+'))
        lines.push({ kind: 'added', text: text.slice(1), next: next++ });
      else if (inHunk && text.startsWith('-'))
        lines.push({ kind: 'removed', text: text.slice(1), old: old++ });
      else if (inHunk && text.startsWith(' '))
        lines.push({ kind: 'context', text: text.slice(1), old: old++, next: next++ });
      else if (
        text.startsWith('\\') ||
        text.startsWith('Binary files ') ||
        text.startsWith('GIT binary patch')
      )
        lines.push({ kind: 'meta', text });
    }
    return {
      ...file,
      status: file.binary
        ? 'B'
        : (snapshot.statuses?.[file.path] ??
          (/^new file mode /m.test(section)
            ? 'A'
            : /^deleted file mode /m.test(section)
              ? 'D'
              : 'M')),
      lines,
    };
  });
}

/** Keep three context rows at change boundaries; expansion is keyed to immutable line numbers. */
export function foldContext(
  lines: DiffLine[],
  expanded: Readonly<Record<string, { above: number; below: number }>> = {},
): DiffLine[] {
  const firstChange = lines.findIndex((line) => line.kind === 'added' || line.kind === 'removed');
  let lastChange = -1;
  for (let index = lines.length - 1; index >= 0; index--) {
    if (lines[index]!.kind === 'added' || lines[index]!.kind === 'removed') {
      lastChange = index;
      break;
    }
  }
  const output: DiffLine[] = [];
  for (let at = 0; at < lines.length;) {
    const first = lines[at]!;
    if (first.kind !== 'context') {
      output.push(first);
      at++;
      continue;
    }
    let end = at;
    while (end < lines.length && lines[end]!.kind === 'context') end++;
    const leading = firstChange < 0 || at < firstChange;
    const trailing = end > lastChange;
    const id = `${first.old}:${first.next}`;
    const reveal = expanded[id];
    const head = Math.min(end - at, (leading ? 0 : 3) + (reveal?.above ?? 0));
    const tail = Math.min(end - at - head, (trailing ? 0 : 3) + (reveal?.below ?? 0));
    const hidden = end - at - head - tail;
    if (hidden <= 1) output.push(...lines.slice(at, end));
    else {
      output.push(...lines.slice(at, at + head));
      output.push({
        kind: 'gap',
        id,
        text: hidden.toLocaleString(),
        lines: lines.slice(at + head, end - tail),
        leading,
        trailing,
      });
      output.push(...lines.slice(end - tail, end));
    }
    at = end;
  }
  return output;
}

export interface TreeRow {
  path: string;
  name: string;
  depth: number;
  directory: boolean;
  file?: ReviewFile;
}
const asciiLower = (text: string) => text.replace(/[A-Z]/g, (letter) => letter.toLowerCase());
export function reviewTree(
  files: ReviewFile[],
  query: string,
  collapsed: ReadonlySet<string>,
): TreeRow[] {
  const needle = asciiLower(query.trim());
  const rows = new Map<string, TreeRow>();
  for (const file of files) {
    if (needle && !asciiLower(file.path).includes(needle)) continue;
    const parts = file.path.split('/');
    for (let depth = 0; depth < parts.length; depth++) {
      const path = parts.slice(0, depth + 1).join('/');
      const directory = depth < parts.length - 1;
      rows.set(path, {
        path,
        name: parts[depth]!,
        depth,
        directory,
        ...(directory ? {} : { file }),
      });
      if (directory && !needle && collapsed.has(path)) break;
    }
  }
  const children = new Map<string, TreeRow[]>();
  for (const item of rows.values()) {
    const parent = item.path.includes('/') ? item.path.slice(0, item.path.lastIndexOf('/')) : '';
    const group = children.get(parent) ?? [];
    group.push(item);
    children.set(parent, group);
  }
  const output: TreeRow[] = [];
  function visit(parent: string) {
    for (const item of (children.get(parent) ?? []).sort(
      (a, b) =>
        Number(b.directory) - Number(a.directory) ||
        asciiLower(a.name).localeCompare(asciiLower(b.name)),
    )) {
      output.push(item);
      if (item.directory) visit(item.path);
    }
  }
  visit('');
  return output;
}

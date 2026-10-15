export const pendingDestination = 'harness-markdown-pending:';

/** Only inline token content is mended: fences and display math never enter here. */
export function mendStreamingInline(source: string): string {
  const open: { marker: string; end: number }[] = [];
  for (let i = 0; i < source.length;) {
    const char = source[i]!;
    if (char === '\\') {
      i += 2;
      continue;
    }
    if (char === '`') {
      const marker = /^`+/.exec(source.slice(i))![0];
      const runs = /`+/g;
      runs.lastIndex = i + marker.length;
      let closing: RegExpExecArray | null;
      do {
        closing = runs.exec(source);
      } while (closing && closing[0].length !== marker.length);
      const end = closing?.index ?? -1;
      if (end < 0)
        return i + marker.length < source.length
          ? source + marker + closures(open, source.length)
          : source;
      i = end + marker.length;
      continue;
    }
    if (char === '$') {
      const marker = source[i + 1] === '$' ? '$$' : '$';
      let end = i + marker.length;
      while ((end = source.indexOf(marker, end)) >= 0 && escaped(source, end)) end += marker.length;
      if (end < 0) return source; // TeX's underscores and stars must not become Markdown closures.
      i = end + marker.length;
      continue;
    }
    if (char === '[' && source[i - 1] !== '!') {
      open.push({ marker: '[', end: i + 1 });
      i++;
      continue;
    }
    if (char === ']' && open.some((entry) => entry.marker === '[')) {
      const bracket = open.map((entry) => entry.marker).lastIndexOf('[');
      const label = open.splice(bracket, 1)[0]!;
      if (source[i + 1] === '(') {
        let depth = 1;
        let end = i + 2;
        for (; end < source.length && depth; end++) {
          if (source[end] === '\\') {
            end++;
            continue;
          }
          if (source[end] === '(') depth++;
          if (source[end] === ')') depth--;
        }
        if (depth && label.end < i)
          return source.slice(0, i + 2) + pendingDestination + ')' + closures(open, i);
        i = end;
        continue;
      }
      i++;
      continue;
    }
    if (char === '*' || char === '_' || (char === '~' && source[i + 1] === '~')) {
      const run = char === '~' ? '~~' : new RegExp(`^\\${char}+`).exec(source.slice(i))![0];
      const previous = source[i - 1] ?? '';
      const next = source[i + run.length] ?? '';
      const canClose = previous !== '' && !/\s/.test(previous);
      const canOpen =
        next !== '' && !/\s/.test(next) && !(char === '_' && /[\p{L}\p{N}]/u.test(previous));
      let remaining = run;
      while (canClose && open.length && open[open.length - 1]!.marker[0] === char) {
        const top = open[open.length - 1]!;
        if (top.marker.length > remaining.length) {
          top.marker = top.marker.slice(remaining.length);
          remaining = '';
          break;
        }
        remaining = remaining.slice(top.marker.length);
        open.pop();
      }
      if (remaining && canOpen) open.push({ marker: remaining, end: i + run.length });
      i += run.length;
      continue;
    }
    i++;
  }
  const tail = source.match(/\s*$/)![0];
  const end = source.length - tail.length;
  return source.slice(0, end) + closures(open, end) + tail;
}

function escaped(source: string, offset: number): boolean {
  let count = 0;
  while (offset > 0 && source[--offset] === '\\') count++;
  return count % 2 === 1;
}

function closures(open: { marker: string; end: number }[], length: number): string {
  return open
    .slice()
    .reverse()
    .filter((entry) => entry.end < length)
    .map((entry) => (entry.marker === '[' ? `](${pendingDestination})` : entry.marker))
    .join('');
}

export function guardStreamingSetext(source: string): string {
  return source.replace(/([^\n]+\n)( {0,3})([=-]+)$/, '$1$2\\$3');
}

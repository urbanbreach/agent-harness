export interface SearchOptions {
  caseSensitive: boolean;
  wholeWord: boolean;
  regex: boolean;
}
export interface Match {
  start: number;
  end: number;
}
export const MATCH_CAP = 20_000;
export function searchPattern(query: string, options: SearchOptions): RegExp | null {
  if (!query) return null;
  const source = options.regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(
    options.wholeWord ? `\\b(?:${source})\\b` : source,
    options.caseSensitive ? 'gu' : 'giu',
  );
}
export function searchText(
  text: string,
  query: string,
  options: SearchOptions,
): { matches: Match[]; capped: boolean; error?: string } {
  try {
    const pattern = searchPattern(query, options);
    const matches: Match[] = [];
    if (!pattern) return { matches, capped: false };
    for (const match of text.matchAll(pattern)) {
      if (matches.length === MATCH_CAP) return { matches, capped: true };
      matches.push({ start: match.index, end: match.index + match[0].length });
    }
    return { matches, capped: false };
  } catch (error) {
    return { matches: [], capped: false, error: String(error) };
  }
}
export function replaceMatches(
  text: string,
  query: string,
  replacement: string,
  options: SearchOptions,
): string {
  let pattern: RegExp | null;
  try {
    pattern = searchPattern(query, options);
  } catch {
    return text;
  }
  if (!pattern) return text;
  return text.replace(pattern, (...args) => {
    const matched = args[0] as string;
    if (!matched.length) return matched;
    if (!options.regex) return replacement;
    const named = typeof args[args.length - 1] === 'object';
    const offset = args[args.length - (named ? 3 : 2)] as number;
    const captures = args.slice(1, args.length - (named ? 3 : 2)) as (string | undefined)[];
    const groups = named
      ? (args[args.length - 1] as Record<string, string | undefined>)
      : undefined;
    return replacement.replace(/\$(\$|&|`|'|[0-9]{1,2}|<[^>]*>)/g, (token, name: string) => {
      if (name === '$') return '$';
      if (name === '&') return matched;
      if (name === '`') return text.slice(0, offset);
      if (name === "'") return text.slice(offset + matched.length);
      if (name.startsWith('<')) return groups ? (groups[name.slice(1, -1)] ?? '') : token;
      const index = Number(name);
      if (index > 0 && index <= captures.length) return captures[index - 1] ?? '';
      const first = Number(name[0]);
      if (name.length === 2 && first > 0 && first <= captures.length)
        return (captures[first - 1] ?? '') + name[1];
      return token;
    });
  });
}
export function replaceMatch(
  text: string,
  query: string,
  replacement: string,
  options: SearchOptions,
  match: Match,
): string {
  let pattern: RegExp | null;
  try {
    pattern = searchPattern(query, options);
  } catch {
    return text;
  }
  if (!pattern || match.start === match.end) return text;
  const current = new RegExp(pattern.source, pattern.flags.replace('g', '') + 'y');
  current.lastIndex = match.start;
  return options.regex
    ? text.replace(current, replacement)
    : text.replace(current, () => replacement);
}

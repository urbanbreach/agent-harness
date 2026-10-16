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
  return options.regex
    ? text.replace(pattern, replacement)
    : text.replace(pattern, () => replacement);
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
  if (!pattern) return text;
  const current = new RegExp(pattern.source, pattern.flags.replace('g', '') + 'y');
  current.lastIndex = match.start;
  return options.regex
    ? text.replace(current, replacement)
    : text.replace(current, () => replacement);
}

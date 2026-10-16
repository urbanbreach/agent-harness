export function matchScore(query: string, text: string): number {
  const needle = query.trim().normalize('NFKD').toLowerCase();
  const haystack = text.normalize('NFKD').toLowerCase();
  if (!needle) return 1;
  const exact = haystack.indexOf(needle);
  if (exact >= 0) return 1000 - exact;
  let cursor = 0;
  let score = 0;
  let previous = -2;
  for (const character of needle) {
    const index = haystack.indexOf(character, cursor);
    if (index < 0) return 0;
    score += index === previous + 1 ? 8 : 1;
    if (index === 0 || /[\s/_-]/.test(haystack[index - 1]!)) score += 4;
    previous = index;
    cursor = index + 1;
  }
  return score;
}

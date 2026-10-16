export function matchScore(query: string, text: string): number {
  const needle = query.trim().normalize('NFKD').toLowerCase();
  const haystack = text.normalize('NFKD').toLowerCase();
  if (!needle) return 1;
  const exact = haystack.indexOf(needle);
  if (exact >= 0) return 1000 - exact;
  let cursor = 0;
  let score = 0;
  let previous = -2;
  let run = 0;
  let longestRun = 0;
  let crossesWord = false;
  for (const character of needle) {
    const index = haystack.indexOf(character, cursor);
    if (index < 0) return 0;
    score += index === previous + 1 ? 8 : 1;
    run = index === previous + 1 ? run + 1 : 1;
    longestRun = Math.max(longestRun, run);
    if (previous >= 0 && /\s/.test(haystack.slice(previous + 1, index))) crossesWord = true;
    if (index === 0 || /[\s/_-]/.test(haystack[index - 1]!)) score += 4;
    previous = index;
    cursor = index + 1;
  }
  // Long fuzzy queries need a meaningful fragment, not letters scattered through keywords.
  if (needle.length > 2 && crossesWord && longestRun < 3) return 0;
  return score;
}

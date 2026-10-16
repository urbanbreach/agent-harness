import { describe, expect, test } from 'bun:test';
import { MATCH_CAP, replaceMatch, replaceMatches, searchText } from '../../src/app/panel/search';
const literal = { caseSensitive: false, wholeWord: false, regex: false };
describe('file search and replace', () => {
  test('literal metacharacters, word boundaries and replacement dollars stay literal', () => {
    expect(searchText('a.b A.B aXb', 'a.b', literal).matches).toEqual([
      { start: 0, end: 3 },
      { start: 4, end: 7 },
    ]);
    expect(
      searchText('cat catalog CAT', 'cat', { ...literal, wholeWord: true, caseSensitive: true })
        .matches,
    ).toEqual([{ start: 0, end: 3 }]);
    expect(replaceMatches('a.b A.B', 'a.b', '$&', literal)).toBe('$& $&');
  });
  test('regex captures replace, invalid patterns report, and capped navigation does not cap replacement', () => {
    const regex = { ...literal, regex: true };
    expect(replaceMatches('one 12 two 34', '(\\d+)', '[$1]', regex)).toBe('one [12] two [34]');
    expect(replaceMatch('x12 x34', '(?<=x)(\\d+)', '[$1]', regex, { start: 5, end: 7 })).toBe(
      'x12 x[34]',
    );
    expect(searchText('abc', '[', regex).error).toBeDefined();
    expect(searchText('abc', '(?=b)', regex).matches).toEqual([{ start: 1, end: 1 }]);
    const text = 'a'.repeat(MATCH_CAP + 2);
    expect(searchText(text, 'a', literal)).toMatchObject({
      capped: true,
      matches: expect.any(Array),
    });
    expect(searchText(text, 'a', literal).matches.length).toBe(MATCH_CAP);
    expect(replaceMatches(text, 'a', 'b', literal)).toBe('b'.repeat(MATCH_CAP + 2));
  });
});

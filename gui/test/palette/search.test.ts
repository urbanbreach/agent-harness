import { expect, test } from 'bun:test';
import { matchScore } from '../../src/app/palette/search';

test('palette search prefers literal labels while accepting normalized subsequences', () => {
  expect(matchScore('resume', 'Resume…')).toBeGreaterThan(
    matchScore('resume', 'Restore user memory entries'),
  );
  expect(matchScore('ＲＥＳＵＭＥ', 'Resume…')).toBeGreaterThan(0);
  expect(matchScore('fb', 'Focus sidebar')).toBeGreaterThan(0);
  expect(matchScore('missing', 'General')).toBe(0);
});

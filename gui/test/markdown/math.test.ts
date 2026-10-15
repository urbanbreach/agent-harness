import { expect, test } from 'bun:test';
import { renderFormula } from '../../src/markdown/math';

test('formula typesetting yields embedded glyph SVG and rejects invalid or oversized source', async () => {
  const image = await renderFormula('\\frac{x^2}{2}', false, 14, '#242424');
  expect(image).not.toBeNull();
  expect(image!.width).toBeGreaterThan(2);
  expect(image!.height).toBeGreaterThan(14);
  const svg = Buffer.from(image!.url.split(',')[1]!, 'base64').toString();
  expect(svg).toContain('<path');
  expect(svg).not.toContain('currentColor');
  expect(await renderFormula('\\unknowncommand{x}', false, 14, '#242424')).toBeNull();
  expect(await renderFormula('x'.repeat(8193), true, 14, '#242424')).toBeNull();
});

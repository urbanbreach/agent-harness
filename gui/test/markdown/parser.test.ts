import { expect, test } from 'bun:test';
import { parseMarkdown, renderedText } from '../../src/markdown/parse';
import type { MarkdownInline } from '../../src/markdown/model';

test('bare HTTP links trim sentence punctuation and unmatched delimiters, but retain balanced delimiters', () => {
  const source = "HTTPS://example.test/a(b)c). https://example.test/a[b]{c}];?!'";
  const blocks = parseMarkdown(source);
  expect(renderedText(blocks)).toBe(source);
  const paragraph = blocks[0];
  if (paragraph?.type !== 'paragraph') throw new Error('Expected paragraph');
  expect(paragraph.spans.filter((span) => span.link).map((span) => span.link?.destination)).toEqual(
    ['HTTPS://example.test/a(b)c', 'https://example.test/a[b]{c}'],
  );
  const protectedText = parseMarkdown(
    '`https://code.test` $https://math.test$ [https://label.test](https://target.test)',
  );
  const protectedParagraph = protectedText[0];
  if (protectedParagraph?.type !== 'paragraph') throw new Error('Expected paragraph');
  expect(
    protectedParagraph.spans.filter((span) => span.link).map((span) => span.link?.destination),
  ).toEqual(['https://target.test']);
});

test('ordered starts, nested tasks and list continuation blocks preserve hierarchy without copied markers', () => {
  const blocks = parseMarkdown(
    '7. outer\n   - [x] done\n   - [ ] next\n\n     second paragraph\n8. final',
  );
  const list = blocks[0];
  if (list?.type !== 'list') throw new Error('Expected list');
  expect([list.ordered, list.start, list.items.length]).toEqual([true, 7, 2]);
  const nested = list.items[0]?.blocks[1];
  if (nested?.type !== 'list') throw new Error('Expected nested list');
  expect(nested.items.map((item) => item.checked)).toEqual([true, false]);
  expect(nested.items[1]?.blocks.length).toBe(2);
  expect(renderedText(blocks)).toBe('outer\n\ndone\nnext\n\nsecond paragraph\nfinal');
});

test('table alignments and inline-only image degradation retain rendered cell copy order', () => {
  const blocks = parseMarkdown(
    '| left | center | right |\n| :--- | :---: | ---: |\n| **one** | ![caption](image.png) | `three` |',
  );
  const table = blocks[0];
  if (table?.type !== 'table') throw new Error('Expected table');
  expect(table.alignments).toEqual(['left', 'center', 'right']);
  expect(table.rows[0]?.[0]).toContainEqual({ type: 'text', text: 'one', strong: true });
  expect(renderedText(blocks)).toBe('left\ncenter\nright\none\ncaption\nthree');
});

test('literal HTML, source breaks, paragraph images, math and code use plain rendered copy semantics', () => {
  const blocks = parseMarkdown(
    '# ![heading alt](a.png)\n\nfirst\nsecond  \nthird ![not copied](b.png "title") last\n\n<div>literal</div>\n\n$x_1$ $y$\n\n$$\na+b\n$$\n\n```RuSt extra\nlet x = 1;\n```',
  );
  expect(blocks.filter((block) => block.type === 'image')).toEqual([
    { type: 'image', source: 'b.png', alt: 'not copied', title: 'title' },
  ]);
  expect(blocks.at(-1)).toEqual({ type: 'code', language: 'rust', text: 'let x = 1;' });
  expect(renderedText(blocks)).toBe(
    'heading alt\n\nfirst\nsecond\nthird \n\n last\n\n<div>literal</div>\n\nx_1 y\n\na+b\n\nlet x = 1;',
  );
  const math = blocks.find(
    (block) => block.type === 'paragraph' && block.spans.some((span) => span.type === 'math'),
  );
  if (math?.type !== 'paragraph') throw new Error('Expected math paragraph');
  expect(math.spans.filter((span) => span.type === 'math').length).toBe(2);
});

test('streaming mends only content-bearing trailing inline syntax and settles to canonical source', () => {
  const cases: [string, Partial<MarkdownInline>, string][] = [
    ['*word', { emphasis: true }, 'word'],
    ['**word', { strong: true }, 'word'],
    ['~~word', { strike: true }, 'word'],
    ['`word', { code: true }, 'word'],
    ['[word', { link: { destination: '', pending: true } }, 'word'],
    ['[word](https://partial', { link: { destination: '', pending: true } }, 'word'],
  ];
  for (const [source, style, copy] of cases) {
    const blocks = parseMarkdown(source, { streaming: true });
    const paragraph = blocks[0];
    if (paragraph?.type !== 'paragraph') throw new Error('Expected paragraph');
    expect(
      paragraph.spans.find((span) => span.type === 'text' && span.text === copy),
    ).toMatchObject(style);
    expect(renderedText(blocks)).toBe(copy);
    expect(renderedText(parseMarkdown(source))).toBe(source);
  }
  for (const source of ['*', '**', '~~', '`', '[', '\\*word', '$x_*', '$$\nx_*']) {
    expect(renderedText(parseMarkdown(source, { streaming: true }))).toBe(
      renderedText(parseMarkdown(source)),
    );
  }
  expect(parseMarkdown('text\n---', { streaming: true })[0]?.type).toBe('paragraph');
  expect(parseMarkdown('text\n---')[0]?.type).toBe('heading');
  expect(parseMarkdown('```js\n**unfinished', { streaming: true })).toEqual([
    { type: 'code', language: 'js', text: '**unfinished' },
  ]);
  expect(renderedText(parseMarkdown('**settled\n\nlast', { streaming: true }))).toBe(
    '**settled\n\nlast',
  );
});

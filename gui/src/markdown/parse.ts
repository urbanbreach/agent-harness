import MarkdownIt from 'markdown-it';
import type Token from 'markdown-it/lib/token.mjs';
import type { MarkdownAlignment, MarkdownBlock, MarkdownInline, MarkdownStyle } from './model';
import { guardStreamingSetext, mendStreamingInline, pendingDestination } from './streaming';

const parser = new MarkdownIt('commonmark', { html: true, breaks: true, linkify: false });
parser.enable(['table', 'strikethrough']);

parser.inline.ruler.before('escape', 'math', (state, silent) => {
  if (state.src[state.pos] !== '$') return false;
  const marker = state.src[state.pos + 1] === '$' ? '$$' : '$';
  const start = state.pos + marker.length;
  let end = start;
  while ((end = state.src.indexOf(marker, end)) >= 0) {
    let escapes = 0;
    for (let i = end - 1; i >= start && state.src[i] === '\\'; i--) escapes++;
    if (escapes % 2 === 0) break;
    end += marker.length;
  }
  if (end < 0) {
    if (!state.env.streaming) return false;
    if (!silent) state.push('text', '', 0).content = state.src.slice(state.pos);
    state.pos = state.posMax;
    return true;
  }
  if (!silent)
    state.push(marker === '$$' ? 'math_display' : 'math_inline', '', 0).content = state.src.slice(
      start,
      end,
    );
  state.pos = end + marker.length;
  return true;
});

parser.block.ruler.before(
  'fence',
  'math_display',
  (state, startLine, endLine, silent) => {
    const start = state.bMarks[startLine]! + state.tShift[startLine]!;
    if (state.sCount[startLine]! - state.blkIndent >= 4 || !state.src.startsWith('$$', start))
      return false;
    const lineEnd = state.eMarks[startLine]!;
    const first = state.src.slice(start + 2, lineEnd);
    let closingLine = startLine;
    let closing = first.indexOf('$$');
    let content = '';
    if (closing >= 0) {
      if (first.slice(closing + 2).trim()) return false;
      content = first.slice(0, closing);
    } else {
      for (closingLine = startLine + 1; closingLine < endLine; closingLine++) {
        const line = state.src.slice(
          state.bMarks[closingLine]! + state.tShift[closingLine]!,
          state.eMarks[closingLine]!,
        );
        closing = line.indexOf('$$');
        if (closing >= 0 && !line.slice(closing + 2).trim()) {
          content =
            first +
            '\n' +
            state.getLines(startLine + 1, closingLine, state.blkIndent, true) +
            line.slice(0, closing);
          break;
        }
      }
      if (closingLine === endLine) return false;
    }
    if (silent) return true;
    const token = state.push('math_display', '', 0);
    token.content = content.trim();
    token.map = [startLine, closingLine + 1];
    state.line = closingLine + 1;
    return true;
  },
  { alt: ['paragraph', 'reference', 'blockquote', 'list'] },
);

parser.core.ruler.before('inline', 'streaming_tail', (state) => {
  if (!state.env.streaming) return;
  const last = state.tokens
    .slice()
    .reverse()
    .find((token) => token.type === 'inline');
  // A trailing fence/math block must not cause an earlier settled paragraph to be mended.
  if (last && last.map?.[1] === state.env.tailLine)
    last.content = mendStreamingInline(last.content);
});

export function parseMarkdown(
  source: string,
  { streaming = false }: { streaming?: boolean } = {},
): MarkdownBlock[] {
  return parseDocument(source, streaming).blocks;
}

function parseDocument(source: string, streaming: boolean) {
  const display = streaming ? guardStreamingSetext(source) : source;
  const environment = {
    streaming,
    tailLine: display.replace(/\n$/, '').split('\n').length,
    references: undefined as Record<string, { href: string; title: string }> | undefined,
  };
  const tokens = parser.parse(display, environment);
  const lines = [0];
  for (const match of source.matchAll(/\r\n?|\n/g)) lines.push(match.index + match[0].length);
  let tailOffset = 0;
  let tailBlock = 0;
  let pendingMath = false;
  let position = 0;
  function blocks(until?: string): MarkdownBlock[] {
    const result: MarkdownBlock[] = [];
    while (position < tokens.length) {
      const token = tokens[position++]!;
      if (!until && token.map && token.nesting >= 0 && !pendingMath) {
        tailOffset = lines[token.map[0]] ?? source.length;
        tailBlock = result.length;
        // An unfinished display-math opener can absorb later paragraphs when it closes.
        pendingMath =
          token.type !== 'fence' &&
          token.type !== 'code_block' &&
          token.type !== 'html_block' &&
          token.type !== 'math_display' &&
          /(?:^|\r?\n|\r) {0,3}\$\$/.test(
            source.slice(tailOffset, lines[token.map[1]] ?? source.length),
          );
      }
      if (token.type === until) break;
      switch (token.type) {
        case 'paragraph_open':
        case 'heading_open': {
          const inline = tokens[position++];
          const spans = inlines(inline?.children ?? [], token.type === 'heading_open');
          position++; // matching paragraph/heading close
          if (token.type === 'heading_open') {
            result.push({
              type: 'heading',
              level: Number(token.tag.slice(1)),
              spans: spans.filter(isInline),
            });
          } else {
            let run: MarkdownInline[] = [];
            const flush = () => {
              if (run.length) result.push({ type: 'paragraph', spans: run });
              run = [];
            };
            for (const span of spans) {
              if (isInline(span)) run.push(span);
              else {
                flush();
                result.push(
                  span.type === 'display_math' ? { type: 'math', source: span.source } : span,
                );
              }
            }
            flush();
          }
          break;
        }
        case 'blockquote_open':
          result.push({ type: 'quote', blocks: blocks('blockquote_close') });
          break;
        case 'bullet_list_open':
        case 'ordered_list_open': {
          const list: Extract<MarkdownBlock, { type: 'list' }> = {
            type: 'list',
            ordered: token.type === 'ordered_list_open',
            start: Number(token.attrGet('start') ?? 1),
            items: [],
          };
          const close = token.type.replace('_open', '_close');
          while (position < tokens.length && tokens[position]!.type !== close) {
            position++; // list_item_open
            const item = {
              blocks: blocks('list_item_close'),
              checked: undefined as boolean | undefined,
            };
            const first = item.blocks[0];
            if (first?.type === 'paragraph' && first.spans[0]?.type === 'text') {
              const match = /^\[([ xX])\](?:[ \t]+|$)/.exec(first.spans[0].text);
              if (match) {
                item.checked = match[1] !== ' ';
                first.spans[0].text = first.spans[0].text.slice(match[0].length);
              }
            }
            list.items.push(item);
          }
          position++;
          result.push(list);
          break;
        }
        case 'table_open': {
          const table: Extract<MarkdownBlock, { type: 'table' }> = {
            type: 'table',
            alignments: [],
            header: [],
            rows: [],
          };
          let header = false;
          let row: MarkdownInline[][] = [];
          while (position < tokens.length) {
            const cell = tokens[position++]!;
            if (cell.type === 'table_close') break;
            if (cell.type === 'thead_open') header = true;
            if (cell.type === 'tbody_open') header = false;
            if (cell.type === 'tr_open') row = [];
            if (cell.type === 'th_open' || cell.type === 'td_open') {
              if (header)
                table.alignments.push(
                  (cell.attrGet('style')?.replace('text-align:', '') ??
                    'left') as MarkdownAlignment,
                );
              row.push(inlines(tokens[position++]?.children ?? [], true).filter(isInline));
            }
            if (cell.type === 'tr_close') {
              if (header) table.header = row;
              else table.rows.push(row);
            }
          }
          result.push(table);
          break;
        }
        case 'hr':
          result.push({ type: 'rule' });
          break;
        case 'fence':
        case 'code_block':
          result.push({
            type: 'code',
            language: token.info.trim().split(/\s+/)[0]?.toLowerCase() ?? '',
            text: token.content.replace(/\n$/, ''),
          });
          break;
        case 'math_display':
          result.push({ type: 'math', source: token.content });
          break;
        case 'html_block':
          result.push({
            type: 'paragraph',
            spans: [{ type: 'text', text: token.content.replace(/\n$/, '') }],
          });
          break;
      }
    }
    return result;
  }
  return {
    blocks: blocks(),
    tailOffset,
    tailBlock,
    hasReferences: environment.references !== undefined,
  };
}

/** Reparse only the last top-level block; earlier blocks keep their render identity. */
export class StreamingMarkdownParser {
  private source = '';
  private blocks: MarkdownBlock[] = [];
  private tailOffset = 0;
  private tailBlock = 0;
  private hasReferences = false;

  parse(source: string, streaming = false): MarkdownBlock[] {
    let offset = !this.hasReferences && source.startsWith(this.source) ? this.tailOffset : 0;
    let retained = offset ? this.tailBlock : 0;
    let parsed = parseDocument(source.slice(offset), streaming);
    // Reference definitions can change links anywhere, including already settled blocks.
    if (offset && parsed.hasReferences) {
      offset = 0;
      retained = 0;
      parsed = parseDocument(source, streaming);
    }
    const next = this.blocks.slice(0, retained).concat(parsed.blocks);
    for (let i = retained; i < next.length && i < this.blocks.length; i++) {
      if (JSON.stringify(next[i]) !== JSON.stringify(this.blocks[i])) break;
      next[i] = this.blocks[i]!;
    }
    this.source = source;
    this.blocks = next;
    this.tailOffset = offset + parsed.tailOffset;
    this.tailBlock = retained + parsed.tailBlock;
    this.hasReferences = parsed.hasReferences;
    return next;
  }
}

type InlinePart =
  | MarkdownInline
  | Extract<MarkdownBlock, { type: 'image' }>
  | { type: 'display_math'; source: string };
function isInline(part: InlinePart): part is MarkdownInline {
  return part.type === 'text' || part.type === 'math';
}

function inlines(tokens: Token[], inlineOnly: boolean): InlinePart[] {
  const result: InlinePart[] = [];
  const style: MarkdownStyle = {};
  const nesting = { strong: 0, emphasis: 0, strike: 0 };
  const decorations: Record<string, keyof typeof nesting | undefined> = {
    strong_open: 'strong',
    strong_close: 'strong',
    em_open: 'emphasis',
    em_close: 'emphasis',
    s_open: 'strike',
    s_close: 'strike',
  };
  for (const token of tokens) {
    const decoration = decorations[token.type];
    if (decoration) {
      nesting[decoration] += token.nesting;
      if (nesting[decoration]) style[decoration] = true;
      else delete style[decoration];
      continue;
    }
    if (token.type === 'link_open') {
      const destination = token.attrGet('href') ?? '';
      style.link = {
        destination: destination === pendingDestination ? '' : destination,
        pending: destination === pendingDestination,
      };
      continue;
    }
    if (token.type === 'link_close') {
      delete style.link;
      continue;
    }
    if (token.type === 'image') {
      const alt = token.children
        ? token.children.map((child) => child.content).join('')
        : token.content;
      if (inlineOnly) result.push({ type: 'text', text: alt, ...style });
      else
        result.push({
          type: 'image',
          source: token.attrGet('src') ?? '',
          alt,
          title: token.attrGet('title') ?? '',
        });
      continue;
    }
    if (token.type === 'math_inline') {
      result.push({ type: 'math', source: token.content, ...style });
      continue;
    }
    if (token.type === 'math_display') {
      if (inlineOnly) result.push({ type: 'math', source: token.content, ...style });
      else result.push({ type: 'display_math', source: token.content });
      continue;
    }
    const text = token.type === 'softbreak' || token.type === 'hardbreak' ? '\n' : token.content;
    if (!text) continue;
    if (token.type === 'code_inline') result.push({ type: 'text', text, ...style, code: true });
    else if (style.link || token.type !== 'text') result.push({ type: 'text', text, ...style });
    else result.push(...bareLinks(text, style));
  }
  return result;
}

function bareLinks(text: string, style: MarkdownStyle): MarkdownInline[] {
  const result: MarkdownInline[] = [];
  let offset = 0;
  for (const match of text.matchAll(/https?:\/\/[^\s<>]+/gi)) {
    const start = match.index;
    if (start > 0 && /[\p{L}\p{N}_]/u.test(text[start - 1]!)) continue;
    let url = match[0];
    while (url) {
      if (/[.,:;?!']$/.test(url)) {
        url = url.slice(0, -1);
        continue;
      }
      const end = url.at(-1)!;
      const opener = ({ ')': '(', ']': '[', '}': '{' } as Record<string, string>)[end];
      if (opener && url.split(end).length > url.split(opener).length) {
        url = url.slice(0, -1);
        continue;
      }
      break;
    }
    if (url.replace(/^https?:\/\//i, '').length === 0) continue;
    if (start > offset) result.push({ type: 'text', text: text.slice(offset, start), ...style });
    result.push({ type: 'text', text: url, ...style, link: { destination: url, pending: false } });
    offset = start + url.length;
  }
  if (offset < text.length) result.push({ type: 'text', text: text.slice(offset), ...style });
  return result;
}

export function renderedText(blocks: readonly MarkdownBlock[]): string {
  const inline = (spans: MarkdownInline[]) =>
    spans.map((span) => (span.type === 'text' ? span.text : span.source)).join('');
  return blocks
    .map((block) => {
      switch (block.type) {
        case 'paragraph':
        case 'heading':
          return inline(block.spans);
        case 'quote':
          return renderedText(block.blocks);
        case 'list':
          return block.items
            .map((item) => renderedText(item.blocks))
            .filter(Boolean)
            .join('\n');
        case 'table':
          return [block.header, ...block.rows].flatMap((row) => row.map(inline)).join('\n');
        case 'code':
          return block.text;
        case 'math':
          return block.source;
        case 'rule':
        case 'image':
          return '';
      }
    })
    .filter(Boolean)
    .join('\n\n');
}

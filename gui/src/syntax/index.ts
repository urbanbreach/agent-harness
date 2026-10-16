import hljs from 'highlight.js/lib/core';
import type { CallbackResponse, Emitter, Mode } from 'highlight.js';
import bash from 'highlight.js/lib/languages/bash';
import c from 'highlight.js/lib/languages/c';
import cpp from 'highlight.js/lib/languages/cpp';
import csharp from 'highlight.js/lib/languages/csharp';
import css from 'highlight.js/lib/languages/css';
import diff from 'highlight.js/lib/languages/diff';
import dockerfile from 'highlight.js/lib/languages/dockerfile';
import go from 'highlight.js/lib/languages/go';
import ini from 'highlight.js/lib/languages/ini';
import java from 'highlight.js/lib/languages/java';
import javascript from 'highlight.js/lib/languages/javascript';
import json from 'highlight.js/lib/languages/json';
import kotlin from 'highlight.js/lib/languages/kotlin';
import less from 'highlight.js/lib/languages/less';
import makefile from 'highlight.js/lib/languages/makefile';
import markdown from 'highlight.js/lib/languages/markdown';
import objectivec from 'highlight.js/lib/languages/objectivec';
import pgsql from 'highlight.js/lib/languages/pgsql';
import python from 'highlight.js/lib/languages/python';
import ruby from 'highlight.js/lib/languages/ruby';
import rust from 'highlight.js/lib/languages/rust';
import scala from 'highlight.js/lib/languages/scala';
import scss from 'highlight.js/lib/languages/scss';
import shell from 'highlight.js/lib/languages/shell';
import sql from 'highlight.js/lib/languages/sql';
import swift from 'highlight.js/lib/languages/swift';
import typescript from 'highlight.js/lib/languages/typescript';
import xml from 'highlight.js/lib/languages/xml';
import yaml from 'highlight.js/lib/languages/yaml';

export interface HighlightRange {
  start: number;
  end: number;
  color: string;
}

type Token =
  'plain' | 'keyword' | 'literal' | 'string' | 'type' | 'comment' | 'meta' | 'added' | 'removed';
const palettes: Record<Token, readonly [string, string]> = {
  plain: ['#666666', '#A3A3A3'],
  keyword: ['#9A4B92', '#C98BC0'],
  literal: ['#9A6019', '#D9A05B'],
  string: ['#3F7A36', '#94C08A'],
  type: ['#2F6690', '#8FB8D9'],
  comment: ['#A4A4A4', '#575757'],
  meta: ['#858585', '#7D7D7D'],
  added: ['#2F8F52', '#62C987'],
  removed: ['#C64A42', '#E2726A'],
};
const scopes: Record<string, Token> = {
  keyword: 'keyword',
  literal: 'literal',
  number: 'literal',
  symbol: 'literal',
  string: 'string',
  regexp: 'string',
  char: 'string',
  type: 'type',
  built_in: 'plain',
  title: 'type',
  class: 'type',
  function: 'type',
  comment: 'comment',
  doctag: 'comment',
  meta: 'meta',
  variable: 'meta',
  'variable.language': 'plain',
  template: 'meta',
  section: 'keyword',
  strong: 'literal',
  emphasis: 'literal',
  code: 'string',
  bullet: 'meta',
  link: 'comment',
  markdown_link_label: 'type',
  addition: 'added',
  deletion: 'removed',
};

interface TokenRange {
  start: number;
  end: number;
  token: Token;
}

// Emit raw UTF-16 spans instead of generating and then decoding escaped HTML.
class RangeEmitter implements Emitter {
  readonly ranges: TokenRange[] = [];
  private readonly stack: Token[] = ['plain'];
  private offset = 0;

  // highlight.js also calls these tree methods directly for enclosing modes.
  openNode(scope: string): void {
    this.stack.push(
      scopes[scope] ?? scopes[scope.split('.')[0]!] ?? this.stack[this.stack.length - 1]!,
    );
  }
  closeNode(): void {
    if (this.stack.length > 1) this.stack.pop();
  }
  startScope(scope: string): void {
    this.openNode(scope);
  }
  endScope(): void {
    this.closeNode();
  }
  addText(text: string): void {
    this.append(text.length, this.stack[this.stack.length - 1]!);
  }
  private append(length: number, token: Token): void {
    if (!length) return;
    const end = this.offset + length;
    const previous = this.ranges[this.ranges.length - 1];
    if (previous?.token === token) previous.end = end;
    else this.ranges.push({ start: this.offset, end, token });
    this.offset = end;
  }
  __addSublanguage(emitter: Emitter): void {
    for (const range of (emitter as RangeEmitter).ranges) {
      this.append(range.end - range.start, range.token);
    }
  }
  finalize(): void {
    this.stack.length = 1;
  }
  toHTML(): string {
    return '';
  }
}

const languages = {
  rust,
  javascript,
  typescript,
  python,
  go,
  c,
  cpp,
  objectivec,
  java,
  kotlin,
  scala,
  csharp,
  ruby,
  swift,
  json,
  yaml,
  ini,
  bash,
  shell,
  dockerfile,
  makefile,
  css,
  scss,
  less,
  xml,
  sql,
  pgsql,
  markdown,
  diff,
};
const labels: Record<keyof typeof languages, string> = {
  rust: 'rust rs',
  javascript: 'js jsx mjs cjs javascript node',
  typescript: 'ts tsx mts cts typescript',
  python: 'py python python3',
  go: 'go golang',
  c: 'c h',
  cpp: 'cc cpp c++ cxx hpp',
  objectivec: 'objc m',
  java: 'java',
  kotlin: 'kt kotlin',
  scala: 'scala',
  csharp: 'cs csharp c#',
  ruby: 'rb ruby gemfile rake',
  swift: 'swift',
  json: 'json jsonc json5',
  yaml: 'yaml yml',
  ini: 'toml ini cfg',
  bash: 'sh bash zsh shell shellscript fish',
  shell: 'console',
  dockerfile: 'dockerfile docker',
  makefile: 'makefile make',
  css: 'css',
  scss: 'scss sass',
  less: 'less',
  xml: 'html htm xml svg vue svelte',
  sql: 'sql mysql sqlite',
  pgsql: 'postgres postgresql',
  markdown: 'md markdown mdx mdown',
  diff: 'diff patch',
};
const aliases: Record<string, string> = Object.create(null);
for (const [language, words] of Object.entries(labels)) {
  for (const word of words.split(' ')) aliases[word] = language;
}

const highlighter = hljs.newInstance();
highlighter.configure({ __emitter: RangeEmitter });
for (const [name, grammar] of Object.entries(languages)) {
  highlighter.registerLanguage(name, grammar);
}

// The stock Markdown grammar treats fenced contents as opaque code. Give known
// fences a lexical sublanguage while keeping their opening/closing lines plain.
const embeddedFences: Mode[] = Object.entries(labels)
  .filter(([name]) => name !== 'markdown')
  .map(([language, words]) => ({
    begin: new RegExp(
      '^ {0,3}(`{3,}|~{3,})[ \\t]*(?:' +
        words
          .split(' ')
          .map((word) => word.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
          .join('|') +
        ')(?=[ \\t\\r\\n]|$)[^\\n]*(?:\\n|$)',
      'i',
    ),
    end: /^ {0,3}(`{3,}|~{3,})[ \t]*$/,
    excludeBegin: true,
    excludeEnd: true,
    subLanguage: language,
    'on:begin': (match: RegExpMatchArray, response: CallbackResponse) => {
      response.data.fence = match[1];
    },
    'on:end': (match: RegExpMatchArray, response: CallbackResponse) => {
      const fence = response.data.fence!;
      if (match[1]![0] !== fence[0] || match[1]!.length < fence.length) response.ignoreMatch();
    },
  }));
highlighter.registerLanguage('markdown', (api) => {
  const grammar = markdown(api);
  grammar.case_insensitive = true;
  for (const mode of grammar.contains) {
    if (mode === 'self') continue;
    const parts = mode.contains;
    if (!parts?.some((part) => part !== 'self' && part.className === 'link')) continue;
    // Markdown's grammar calls link labels strings; keep URLs muted and labels blue.
    for (const part of parts) {
      if (part !== 'self' && part.className === 'string') part.className = 'markdown_link_label';
    }
  }
  grammar.contains.unshift(...embeddedFences, {
    className: 'literal',
    begin: /(?<=^[ \t]*[-+*][ \t]+)\[[ xX]\]/,
  });
  return grammar;
});

const cache = new Map<string, HighlightRange[]>();
const maxEntries = 128;
const maxCachedUnits = 1_000_000;
let cachedUnits = 0;

/** Contiguous, end-exclusive UTF-16 ranges covering the original code verbatim. */
export function highlightCode(
  text: string,
  language: string | undefined,
  dark: boolean,
): HighlightRange[] {
  if (!text.length) return [];
  const label = language?.trim().split(/\s+/, 1)[0]?.toLowerCase();
  const canonical = label ? aliases[label] : undefined;
  const key = `${dark ? 1 : 0}:${canonical ?? ''}:${text}`;
  const cached = cache.get(key);
  if (cached) {
    cache.delete(key);
    cache.set(key, cached);
    return cached;
  }
  const tokens = canonical
    ? (
        highlighter.highlight(text, { language: canonical, ignoreIllegals: true })
          ._emitter as RangeEmitter
      ).ranges
    : [{ start: 0, end: text.length, token: 'plain' as const }];
  const ranges = tokens.map(({ start, end, token }) => ({
    start,
    end,
    color: palettes[token][dark ? 1 : 0],
  }));
  if (key.length <= maxCachedUnits) {
    while (cache.size >= maxEntries || cachedUnits + key.length > maxCachedUnits) {
      const oldest = cache.keys().next().value!;
      cache.delete(oldest);
      cachedUnits -= oldest.length;
    }
    cache.set(key, ranges);
    cachedUnits += key.length;
  }
  return ranges;
}

export interface MarkdownStyle {
  strong?: boolean;
  emphasis?: boolean;
  strike?: boolean;
  code?: boolean;
  link?: { destination: string; pending: boolean };
}

export type MarkdownInline =
  | ({ type: 'text'; text: string } & MarkdownStyle)
  | ({ type: 'math'; source: string } & MarkdownStyle);

export interface MarkdownListItem {
  checked?: boolean;
  blocks: MarkdownBlock[];
}

export type MarkdownAlignment = 'left' | 'center' | 'right';

export type MarkdownBlock =
  | { type: 'paragraph'; spans: MarkdownInline[] }
  | { type: 'heading'; level: number; spans: MarkdownInline[] }
  | { type: 'quote'; blocks: MarkdownBlock[] }
  | { type: 'list'; ordered: boolean; start: number; items: MarkdownListItem[] }
  | {
      type: 'table';
      alignments: MarkdownAlignment[];
      header: MarkdownInline[][];
      rows: MarkdownInline[][][];
    }
  | { type: 'rule' }
  | { type: 'code'; language: string; text: string }
  | { type: 'image'; source: string; alt: string; title: string }
  | { type: 'math'; source: string };

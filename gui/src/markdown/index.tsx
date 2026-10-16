import React, {
  createContext,
  memo,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import type { EventPayload, StyleDesc } from '@gpuix/react';
import { useTheme, UI_FONT_FAMILY } from '../theme';
import type { ThemeColors } from '../theme';
import { useNativeRenderer } from '../native';
import type { NativeEvent, Run } from '../native';
import { Button, Tooltip } from '../ui/controls';
import { Menu } from '../ui/menus';
import type { MenuItem } from '../ui/menus';
import type { IconName } from '../ui/icon-names';
import { t } from '../i18n';
import { highlightCode } from '../syntax';
import { parseMarkdown } from './parse';
import type { MarkdownBlock, MarkdownInline } from './model';
import { renderFormula } from './math';
import type { FormulaImage } from './math';
import { useStreamingVeil } from './veil';
export { parseMarkdown, renderedText } from './parse';
export type { MarkdownBlock, MarkdownInline } from './model';
export interface MarkdownProps {
  source: string;
  streaming?: boolean;
  context?: 'assistant' | 'user' | 'compact' | 'document';
  reasoning?: boolean;
  mathEnabled?: boolean;
  onOpenLink?: (destination: string) => void;
  onHover?: (event: EventPayload) => void;
  /** Usual containing-message context actions, appended after formula actions. */
  menuItems?: MenuItem[];
  style?: StyleDesc;
}
interface Metrics {
  size: number;
  line: number;
  codeSize: number;
  codeLine: number;
  gap: number;
}
interface Environment extends Metrics {
  colors: ThemeColors;
  streaming: boolean;
  reducedMotion: boolean;
  mathEnabled: boolean;
  reasoning: boolean;
  onOpenLink?: (destination: string) => void;
  onHover?: (event: EventPayload) => void;
  menuItems: MenuItem[];
}
const EnvironmentContext = createContext<Environment | null>(null);
function useMarkdown() {
  const value = useContext(EnvironmentContext);
  if (!value) throw new Error('Markdown child requires Markdown');
  return value;
}
const column: StyleDesc = {
  display: 'flex',
  flexDirection: 'column',
  width: '100%',
  minWidth: 0,
  flexShrink: 0,
};
const row: StyleDesc = { display: 'flex', flexDirection: 'row', minWidth: 0 };

/** Markdown ornaments stay fixed-size when UI typography changes. */
function Icon({ name, size, color }: { name: IconName; size: number; color: string }) {
  return (
    <svg
      src={new URL(`../../assets/icons/${name}.svg`, import.meta.url).pathname}
      style={{ width: size, height: size, flexShrink: 0, color }}
    />
  );
}

export function markdownMetrics(
  context: MarkdownProps['context'],
  uiSize: number,
  codeSize: number,
): Metrics {
  const half = (value: number) => Math.round(value * 2) / 2;
  if (context === 'document')
    return {
      size: uiSize,
      line: Math.round(uiSize * 1.55),
      gap: Math.round(uiSize * 0.72),
      codeSize,
      codeLine: Math.round(codeSize * 1.5),
    };
  return {
    size: half(((context === 'user' ? 14.5 : context === 'compact' ? 13.5 : 14) * uiSize) / 14),
    line: half(((context === 'compact' ? 19.5 : 21) * uiSize) / 14),
    gap: half(((context === 'compact' ? 7 : 10) * uiSize) / 14),
    codeSize: half(codeSize),
    codeLine: half((19.5 * codeSize) / 13),
  };
}

export function Markdown({
  source,
  streaming = false,
  context = 'assistant',
  reasoning = false,
  mathEnabled = true,
  onOpenLink,
  onHover,
  menuItems = [],
  style,
}: MarkdownProps) {
  const theme = useTheme();
  const previous = useRef<MarkdownBlock[]>([]);
  const blocks = useMemo(() => {
    const parsed = parseMarkdown(source, { streaming });
    for (let i = 0; i < parsed.length && i < previous.current.length; i++) {
      if (JSON.stringify(parsed[i]) !== JSON.stringify(previous.current[i])) break;
      parsed[i] = previous.current[i]!;
    }
    previous.current = parsed;
    return parsed;
  }, [source, streaming]);
  const environment = useMemo<Environment>(
    () => ({
      ...markdownMetrics(context, theme.uiFontSize, theme.codeFontSize),
      colors: theme.colors,
      reducedMotion: theme.reducedMotion,
      streaming,
      reasoning,
      mathEnabled,
      onOpenLink,
      onHover,
      menuItems,
    }),
    [
      context,
      theme.uiFontSize,
      theme.codeFontSize,
      theme.colors,
      theme.reducedMotion,
      streaming,
      reasoning,
      mathEnabled,
      onOpenLink,
      onHover,
      menuItems,
    ],
  );
  if (!blocks.length) return null;
  return (
    <EnvironmentContext.Provider value={environment}>
      <div
        onMouseEnter={onHover}
        onMouseLeave={onHover}
        style={{
          ...column,
          gap: environment.gap,
          fontFamily: UI_FONT_FAMILY,
          fontSize: environment.size,
          lineHeight: environment.line,
          color: reasoning ? theme.colors.text_secondary : theme.colors.text,
          cursor: 'text',
          selectionColor: theme.colors.selection,
          ...style,
        }}
      >
        {blocks.map((block, index) => (
          <Block key={index} block={block} />
        ))}
      </div>
    </EnvironmentContext.Provider>
  );
}

function styleRuns(
  spans: MarkdownInline[],
  colors: ThemeColors,
  weight = 400,
): {
  text: string;
  runs: Run[];
  links: { start: number; end: number; destination: string; pending: boolean }[];
} {
  let text = '';
  const runs: Run[] = [];
  const links: { start: number; end: number; destination: string; pending: boolean }[] = [];
  for (const span of spans) {
    const start = text.length;
    text += span.type === 'math' ? span.source : span.text;
    const end = text.length;
    runs.push({
      start,
      end,
      fontWeight: span.strong ? Math.max(weight, 600) : weight,
      italic: span.emphasis,
      color: span.code ? colors.code_text : undefined,
      fontFamily: span.code ? 'JetBrains Mono' : undefined,
      wash: span.code ? { color: colors.code_wash, radius: 4, padX: 2.5, insetY: 1.5 } : undefined,
      underline: span.link ? { color: colors.text_tertiary, thickness: 1, wavy: false } : undefined,
      strikethrough: span.strike ? { color: colors.text_tertiary, thickness: 1 } : undefined,
    });
    if (span.link) links.push({ start, end, ...span.link });
  }
  return { text, runs, links };
}

function InlineText({
  spans,
  style,
  weight = 400,
  veil = true,
}: {
  spans: MarkdownInline[];
  style?: StyleDesc;
  weight?: number;
  veil?: boolean;
}) {
  const env = useMarkdown();
  const native = useNativeRenderer();
  const model = useMemo(() => styleRuns(spans, env.colors, weight), [spans, env.colors, weight]);
  const ink = env.reasoning ? env.colors.text_secondary : env.colors.text;
  const runs = useStreamingVeil(
    model.text,
    model.runs,
    ink,
    veil && env.streaming,
    env.reducedMotion,
  );
  const [chosen, setChosen] = useState<number>(-1);
  const [focused, setFocused] = useState(false);
  const links = model.links.filter((link) => !link.pending);
  const activate = (event: NativeEvent) => {
    if (native.getSelectedText?.()) return;
    const index = event.charIndex;
    if (index === null || index === undefined) return;
    const link = links.find((link) => index >= link.start && index < link.end);
    if (link) env.onOpenLink?.(link.destination);
  };
  function keys(event: EventPayload) {
    const key = event.key ?? '';
    if (['right', 'down', 'left', 'up', 'home', 'end'].includes(key) && links.length)
      setChosen((value) =>
        key === 'home'
          ? 0
          : key === 'end'
            ? links.length - 1
            : (value + (key === 'left' || key === 'up' ? -1 : 1) + links.length) % links.length,
      );
    else if ((key === 'enter' || key === 'space' || key === ' ') && chosen >= 0)
      env.onOpenLink?.(links[chosen]!.destination);
    else if (key === 'escape') setChosen(-1);
  }
  const focusedRuns =
    chosen >= 0 && links[chosen]
      ? [
          ...runs,
          { start: links[chosen]!.start, end: links[chosen]!.end, background: env.colors.overlay },
        ]
      : runs;
  return (
    <rich-text
      text={model.text}
      onMouseEnter={env.onHover}
      onMouseLeave={env.onHover}
      runs={focusedRuns}
      tabIndex={links.length ? 0 : undefined}
      onClick={activate}
      onKeyDown={keys}
      onFocus={() => setFocused(true)}
      onBlur={() => {
        setFocused(false);
        setChosen(-1);
      }}
      style={{
        width: '100%',
        minWidth: 0,
        flexShrink: 0,
        fontSize: env.size,
        lineHeight: env.line,
        color: ink,
        cursor: 'text',
        ...style,
        ...(links.length && focused ? { borderWidth: 1, borderColor: env.colors.accent } : {}),
      }}
    />
  );
}

function Formula({ source, display = false }: { source: string; display?: boolean }) {
  const env = useMarkdown();
  const [image, setImage] = useState<FormulaImage | null>(null);
  const ink = env.reasoning ? env.colors.text_secondary : env.colors.text;
  useEffect(() => {
    let active = true;
    setImage(null);
    if (env.mathEnabled)
      void renderFormula(source, display, env.size, ink).then((value) => {
        if (active) setImage(value);
      });
    return () => {
      active = false;
    };
  }, [source, display, env.size, ink, env.mathEnabled]);
  if (!image)
    return (
      <rich-text
        text={source}
        onMouseEnter={env.onHover}
        onMouseLeave={env.onHover}
        style={{
          fontFamily: display ? 'JetBrains Mono' : UI_FONT_FAMILY,
          fontSize: env.size,
          lineHeight: env.line,
          color: ink,
          maxWidth: '100%',
          flexShrink: 1,
        }}
      />
    );
  return (
    <img
      src={image.url}
      alt={source}
      onMouseEnter={env.onHover}
      onMouseLeave={env.onHover}
      objectFit="scaleDown"
      style={{
        width: image.width,
        height: image.height,
        maxWidth: '100%',
        flexShrink: 1,
        userSelect: 'none',
      }}
    />
  );
}

function MathFlow({
  spans,
  style,
  weight = 400,
  display = false,
}: {
  spans: MarkdownInline[];
  style?: StyleDesc;
  weight?: number;
  display?: boolean;
}) {
  const env = useMarkdown();
  const native = useNativeRenderer();
  const [menu, setMenu] = useState<{ x: number; y: number }>();
  const [focused, setFocused] = useState(false);
  const [chosen, setChosen] = useState(-1);
  const expressions = spans.filter((span) => span.type === 'math');
  const links = spans
    .flatMap((span) => (span.link && !span.link.pending ? [span.link.destination] : []))
    .filter((value, i, all) => i === 0 || value !== all[i - 1]);
  const items: MenuItem[] =
    expressions.length === 1
      ? [
          {
            id: 'expression',
            label: t('clipboard.expression'),
            onSelect: () => native.writeClipboardText(expressions[0]!.source),
          },
        ]
      : [
          { id: 'expressions', type: 'caption', label: t('clipboard.expression') },
          ...expressions.map((span, i) => ({
            id: `expression-${i}`,
            label: span.source,
            onSelect: () => native.writeClipboardText(span.source),
          })),
        ];
  function keys(event: EventPayload) {
    const key = event.key ?? '';
    if (
      env.mathEnabled &&
      (key === 'contextmenu' || key === 'apps' || (key === 'f10' && event.modifiers?.shift))
    )
      setMenu({ x: 0, y: 0 });
    else if (['right', 'down', 'left', 'up', 'home', 'end'].includes(key) && links.length)
      setChosen((value) =>
        key === 'home'
          ? 0
          : key === 'end'
            ? links.length - 1
            : (value + (key === 'left' || key === 'up' ? -1 : 1) + links.length) % links.length,
      );
    else if ((key === 'enter' || key === 'space' || key === ' ') && chosen >= 0)
      env.onOpenLink?.(links[chosen]!);
    else if (key === 'escape') setChosen(-1);
  }
  const mathEnvironment = {
    ...env,
    size: style?.fontSize ?? env.size,
    line: style?.lineHeight ?? env.line,
  };
  return (
    <EnvironmentContext.Provider value={mathEnvironment}>
      <div
        tabIndex={env.mathEnabled || links.length ? 0 : undefined}
        onFocus={() => setFocused(true)}
        onBlur={() => {
          setFocused(false);
          setChosen(-1);
        }}
        onMouseDown={(event) => {
          if (event.button === 2 && env.mathEnabled) setMenu({ x: event.x ?? 0, y: event.y ?? 0 });
        }}
        onKeyDown={keys}
        style={{
          ...row,
          flexWrap: 'wrap',
          alignItems: 'baseline',
          width: '100%',
          backgroundColor: focused && !links.length ? env.colors.overlay : undefined,
          borderWidth: links.length ? 1 : 0,
          borderColor: focused && links.length ? env.colors.accent : 'transparent',
          ...style,
        }}
      >
        {spans.flatMap((span, index) =>
          span.type === 'math'
            ? [<Formula key={`m${index}`} source={span.source} display={display} />]
            : (span.text.match(/\S+[^\S\n]*|[^\S\n]+|\n/g) ?? []).map((word, part) =>
                word === '\n' ? (
                  <div key={`${index}-${part}`} style={{ width: '100%', height: 0 }} />
                ) : (
                  <InlineText
                    key={`${index}-${part}`}
                    spans={[{ ...span, text: word }]}
                    weight={weight}
                    veil={false}
                    style={{ width: undefined, maxWidth: '100%', flexShrink: 1 }}
                  />
                ),
              ),
        )}
        {menu && (
          <Menu
            items={[...items, ...env.menuItems]}
            position={menu}
            onClose={() => setMenu(undefined)}
          />
        )}
      </div>
    </EnvironmentContext.Provider>
  );
}
function Inline({
  spans,
  ...props
}: {
  spans: MarkdownInline[];
  style?: StyleDesc;
  weight?: number;
}) {
  return spans.some((span) => span.type === 'math') ? (
    <MathFlow spans={spans} {...props} />
  ) : (
    <InlineText spans={spans} {...props} />
  );
}

const CodeBlock = memo(function CodeBlock({ text, language }: { text: string; language: string }) {
  const env = useMarkdown();
  const native = useNativeRenderer();
  const [copied, setCopied] = useState(false);
  const timer = useRef<Timer | undefined>(undefined);
  useEffect(
    () => () => {
      clearTimeout(timer.current);
    },
    [],
  );
  const plain = env.reasoning ? env.colors.text_tertiary : env.colors.text_secondary;
  const runs = useMemo(
    () =>
      highlightCode(text, language, env.colors.is_dark).map((run) =>
        env.reasoning && run.color === env.colors.text_secondary
          ? { ...run, color: env.colors.text_tertiary }
          : run,
      ),
    [text, language, env.colors, env.reasoning],
  );
  const faded = useStreamingVeil(text, runs, plain, env.streaming, env.reducedMotion);
  function copy() {
    native.writeClipboardText(text);
    setCopied(true);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(false), 3000);
  }
  return (
    <div
      style={{
        ...column,
        borderRadius: 8,
        borderWidth: 1,
        borderColor: env.colors.border,
        backgroundColor: env.colors.inset,
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          ...row,
          height: 28,
          alignItems: 'center',
          paddingLeft: 10,
          paddingRight: 2,
          borderBottomWidth: 1,
          borderColor: env.colors.border,
          userSelect: 'none',
        }}
      >
        <text
          style={{
            flexGrow: 1,
            flexShrink: 1,
            minWidth: 0,
            fontSize: 12.5,
            lineHeight: 14,
            fontWeight: 500,
            color: env.colors.text_ghost,
            whiteSpace: 'nowrap',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
          }}
        >
          {language.toLowerCase()}
        </text>
        <Tooltip content={copied ? t('clipboard.copyDone') : t('clipboard.code')}>
          <Button
            aria-label={copied ? t('clipboard.copyDone') : t('clipboard.code')}
            onClick={copy}
            style={{ width: 24, height: 24, borderRadius: 5 }}
          >
            <Icon name={copied ? 'check' : 'copy'} size={11} color={env.colors.text_ghost} />
          </Button>
        </Tooltip>
      </div>
      <div
        style={{ ...column, paddingLeft: 10, paddingRight: 10, paddingTop: 8, paddingBottom: 8 }}
      >
        <rich-text
          text={text}
          onMouseEnter={env.onHover}
          onMouseLeave={env.onHover}
          runs={faded}
          style={{
            width: '100%',
            minWidth: 0,
            fontFamily: 'JetBrains Mono',
            fontSize: env.codeSize,
            lineHeight: env.codeLine,
            color: plain,
          }}
        />
      </div>
    </div>
  );
});

export function tableColumnFractions(table: Extract<MarkdownBlock, { type: 'table' }>): number[] {
  const sizes = table.header.map((_, index) =>
    Math.max(
      0,
      ...[table.header, ...table.rows].map(
        (row) =>
          [
            ...(row[index] ?? [])
              .map((span) => (span.type === 'math' ? span.source : span.text))
              .join(''),
          ].length,
      ),
    ),
  );
  if (!sizes.length) return [];
  const total = sizes.reduce((sum, size) => sum + size, 0);
  if (!total) return sizes.map(() => 1 / sizes.length);
  const minimum = 0.55 / sizes.length;
  const result = sizes.map(() => 0);
  let available = 1;
  let remaining = sizes.map((_, i) => i);
  while (remaining.length) {
    const sum = remaining.reduce((value, i) => value + sizes[i]!, 0);
    const small = remaining.filter((i) => sum === 0 || (available * sizes[i]!) / sum < minimum);
    if (!small.length) {
      for (const i of remaining) result[i] = (available * sizes[i]!) / sum;
      break;
    }
    for (const i of small) {
      result[i] = minimum;
      available -= minimum;
    }
    remaining = remaining.filter((i) => !small.includes(i));
  }
  return result;
}

const Block = memo(function Block({ block }: { block: MarkdownBlock }) {
  const env = useMarkdown();
  switch (block.type) {
    case 'paragraph':
      return <Inline spans={block.spans} />;
    case 'heading': {
      const size = Math.round(env.size * [1.45, 1.28, 1.14, 1.05, 1, 1][block.level - 1]!);
      return (
        <div style={{ ...column, paddingTop: block.level <= 2 ? 4 : 0 }}>
          <Inline
            spans={block.spans}
            weight={block.level <= 2 ? 700 : 600}
            style={{ fontSize: size, lineHeight: Math.round(size * 1.42) }}
          />
        </div>
      );
    }
    case 'quote':
      return (
        <div style={{ ...row, width: '100%', gap: 10 }}>
          <div
            style={{
              width: 2,
              alignSelf: 'stretch',
              flexShrink: 0,
              borderRadius: 1,
              backgroundColor: env.colors.border,
              userSelect: 'none',
            }}
          />
          <div style={{ ...column, width: undefined, flexGrow: 1, flexShrink: 1, gap: env.gap }}>
            {block.blocks.map((child, i) => (
              <Block key={i} block={child} />
            ))}
          </div>
        </div>
      );
    case 'list':
      return (
        <div style={{ ...column, gap: env.gap / 2 }}>
          {block.items.map((item, index) => {
            const side = Math.round(env.size * 0.92);
            return (
              <div key={index} style={{ ...row, width: '100%', alignItems: 'flex-start' }}>
                <div style={{ width: block.ordered ? 22 : 14, flexShrink: 0, userSelect: 'none' }}>
                  {item.checked === undefined ? (
                    <text
                      style={{
                        fontSize: env.size,
                        lineHeight: env.line,
                        color: env.colors.text_tertiary,
                      }}
                    >
                      {block.ordered ? `${block.start + index}.` : '•'}
                    </text>
                  ) : (
                    <div
                      style={{
                        width: side,
                        height: side,
                        marginTop: (env.line - side) / 2,
                        marginBottom: (env.line - side) / 2,
                        borderRadius: 3,
                        borderWidth: 1,
                        borderColor: item.checked ? env.colors.accent : env.colors.border,
                        backgroundColor: item.checked ? env.colors.accent : undefined,
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                      }}
                    >
                      {item.checked && (
                        <Icon name="check" size={side - 4} color={env.colors.inset} />
                      )}
                    </div>
                  )}
                </div>
                <div
                  style={{
                    ...column,
                    width: undefined,
                    flexGrow: 1,
                    flexShrink: 1,
                    gap: env.gap * 0.6,
                  }}
                >
                  {item.blocks.map((child, i) => (
                    <Block key={i} block={child} />
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      );
    case 'rule':
      return (
        <div
          style={{
            width: '100%',
            height: 1,
            backgroundColor: env.colors.border,
            marginTop: 4,
            marginBottom: 4,
            userSelect: 'none',
          }}
        />
      );
    case 'code':
      return <CodeBlock text={block.text} language={block.language} />;
    case 'image': {
      const caption = block.alt.trim() ? block.alt : block.title;
      return (
        <div style={{ ...column, gap: 4, userSelect: 'none' }}>
          <img
            src={block.source}
            onMouseEnter={env.onHover}
            onMouseLeave={env.onHover}
            objectFit="scaleDown"
            style={{ maxWidth: '100%', maxHeight: 320, borderRadius: 6, flexShrink: 1 }}
          />
          {caption && (
            <text
              style={{
                fontSize: Math.max(env.size - 2, 12.5),
                lineHeight: env.line - 4,
                color: env.colors.text_ghost,
              }}
            >
              {caption}
            </text>
          )}
        </div>
      );
    }
    case 'math':
      return (
        <MathFlow
          spans={[{ type: 'math', source: block.source }]}
          display
          style={{ justifyContent: 'center' }}
        />
      );
    case 'table': {
      const fractions = tableColumnFractions(block);
      if (!fractions.length) return null;
      return (
        <div
          style={{
            ...column,
            borderRadius: 8,
            borderWidth: 1,
            borderColor: env.colors.border,
            overflow: 'hidden',
          }}
        >
          {[block.header, ...block.rows].map((cells, r) => (
            <div
              key={r}
              style={{
                ...row,
                width: '100%',
                alignItems: 'flex-start',
                backgroundColor: r === 0 ? env.colors.overlay : undefined,
                borderBottomWidth: r < block.rows.length || r === 0 ? 1 : 0,
                borderColor: env.colors.border,
              }}
            >
              {fractions.map((fraction, c) => (
                <div
                  key={c}
                  style={{
                    ...column,
                    width: `${fraction * 100}%`,
                    paddingLeft: 9,
                    paddingRight: 9,
                    paddingTop: 6,
                    paddingBottom: 6,
                    flexShrink: 1,
                  }}
                >
                  <Inline
                    spans={cells[c] ?? []}
                    weight={r === 0 ? 600 : 400}
                    style={{
                      fontSize: Math.max(env.size - 0.5, 12.5),
                      lineHeight: env.line - 2,
                      textAlign: block.alignments[c] ?? 'left',
                    }}
                  />
                </div>
              ))}
            </div>
          ))}
        </div>
      );
    }
  }
});

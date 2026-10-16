import { useEffect, useRef } from 'react';
import { useGpuix, type PublicInstance, type StyleDesc } from '@gpuix/react';
import { useTheme } from '../../theme';
import { Badge, Icon, Spinner } from '../../ui';
import type { Suggestion } from './completion';
import type { Run } from '../../native';

function MatchingText({
  item,
  text,
  query,
  style,
}: {
  item: Suggestion;
  text: string;
  query: string;
  style: StyleDesc;
}) {
  const { colors } = useTheme();
  const runs: Run[] = [];
  const offset = item.value.lastIndexOf(text);
  let cursor = 1;
  for (const character of query.toLocaleLowerCase()) {
    const index = item.value.toLocaleLowerCase().indexOf(character, cursor);
    if (index < 0) break;
    if (offset >= 0 && index >= offset && index < offset + text.length)
      runs.push({
        start: index - offset,
        end: index - offset + 1,
        color: colors.accent,
        fontWeight: 600,
      });
    cursor = index + 1;
  }
  return <rich-text text={text} runs={runs} style={style} />;
}

export function Suggestions({
  rows,
  selected,
  loading,
  width,
  query,
  onAccept,
  onDismiss,
}: {
  rows: Suggestion[];
  selected: number;
  loading: boolean;
  width: number;
  query: string;
  onAccept: (row: Suggestion) => void;
  onDismiss: () => void;
}) {
  const { colors, sp } = useTheme();
  const { renderer } = useGpuix();
  const scroll = useRef<PublicInstance>(null);
  useEffect(() => {
    if (scroll.current)
      renderer?.scrollTo?.(scroll.current.id, 0, Math.max(0, selected * 30 - 240));
  }, [selected, renderer]);
  if (!rows.length && !loading) return null;
  return (
    <anchored
      side="top"
      align="start"
      gap={6}
      fit="snap"
      snapMargin={8}
      deferred
      priority={1}
      occlude
    >
      <div
        role="listbox"
        aria-label="Composer suggestions"
        onMouseDownOutside={onDismiss}
        style={{
          width,
          maxHeight: 302,
          padding: 4,
          backgroundColor: colors.raised,
          borderWidth: 1,
          borderColor: colors.border_strong,
          borderRadius: 11,
          overflow: 'hidden',
          boxShadow: {
            offsetX: 0,
            offsetY: 10,
            blurRadius: 24,
            spreadRadius: 0,
            color: '#00000030',
          },
        }}
      >
        <div
          ref={scroll}
          style={{ maxHeight: 292, overflowY: 'scroll', display: 'flex', flexDirection: 'column' }}
        >
          {!rows.length && loading ? (
            <div
              style={{
                height: 30,
                display: 'flex',
                flexDirection: 'row',
                alignItems: 'center',
                paddingLeft: 8,
                gap: 8,
              }}
            >
              <Spinner size={12} />
              <text style={{ fontSize: sp(12.5), color: colors.text_tertiary }}>
                Loading suggestions…
              </text>
            </div>
          ) : (
            rows.map((item, index) => (
              <div
                key={item.value}
                role="option"
                aria-selected={index === selected}
                onMouseDown={(event) => {
                  if (event.button === 0) onAccept(item);
                }}
                style={{
                  height: 30,
                  flexShrink: 0,
                  display: 'flex',
                  flexDirection: 'row',
                  alignItems: 'center',
                  paddingLeft: 8,
                  paddingRight: 8,
                  gap: 8,
                  borderRadius: 6,
                  backgroundColor: index === selected ? colors.overlay_strong : undefined,
                  hover: { backgroundColor: colors.overlay },
                }}
              >
                <Icon
                  name={
                    item.scope === 'skill'
                      ? 'sparkle'
                      : item.scope
                        ? 'command'
                        : item.directory
                          ? 'folder'
                          : 'file'
                  }
                  size={13}
                />
                <MatchingText
                  item={item}
                  text={item.label}
                  query={query}
                  style={{
                    fontSize: sp(12.5),
                    fontWeight: item.scope ? 500 : 400,
                    color: colors.text,
                    maxWidth: 300,
                    whiteSpace: 'nowrap',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                  }}
                />
                <MatchingText
                  item={item}
                  text={item.description}
                  query={query}
                  style={{
                    fontSize: sp(12.5),
                    color: colors.text_tertiary,
                    flexGrow: 1,
                    flexShrink: 1,
                    minWidth: 0,
                    whiteSpace: 'nowrap',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                  }}
                />
                {item.scope && (
                  <Badge
                    style={{
                      height: 18,
                      paddingLeft: 5,
                      paddingRight: 5,
                      borderWidth: 1,
                      borderColor: colors.border_strong,
                      borderRadius: 4,
                    }}
                  >
                    <text style={{ color: colors.text_tertiary, fontSize: sp(10.5) }}>
                      {item.scope}
                    </text>
                  </Badge>
                )}
              </div>
            ))
          )}
        </div>
      </div>
    </anchored>
  );
}

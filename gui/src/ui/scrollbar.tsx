import { useEffect, useRef, useState } from 'react';
import { useGpuix, useWindowSize, type EventPayload, type PublicInstance } from '@gpuix/react';
import { useTheme } from '../theme';

export interface ScrollbarProps {
  viewport: number;
  content: number;
  offset: number;
  onScroll: (offset: number) => void;
  edgeFades?: boolean;
  surface?: string;
}
export function Scrollbar({
  viewport,
  content,
  offset,
  onScroll,
  edgeFades = false,
  surface,
}: ScrollbarProps) {
  const { colors } = useTheme();
  const { renderer } = useGpuix();
  const window = useWindowSize();
  const track = useRef<PublicInstance>(null);
  const previous = useRef(offset);
  const revealUntil = useRef(0);
  const [opacity, setOpacity] = useState(0);
  const [hovered, setHovered] = useState(false);
  const [drag, setDrag] = useState<{ grab: number; top: number }>();
  const valid =
    Number.isFinite(viewport) &&
    Number.isFinite(content) &&
    viewport > 0 &&
    content - viewport > 0.5;
  const height = valid ? Math.min(viewport, Math.max(28, (viewport * viewport) / content)) : 0;
  const range = Math.max(0, content - viewport);
  const travel = viewport - height;
  const top = range ? (Math.max(0, Math.min(range, offset)) / range) * travel : 0;
  useEffect(() => {
    if (!valid) {
      setDrag(undefined);
      setHovered(false);
      setOpacity(0);
      previous.current = offset;
      return;
    }
    if (Math.abs(previous.current - offset) > 0.5) {
      previous.current = offset;
      revealUntil.current = Date.now() + 900;
      setOpacity(1);
    }
    if (Date.now() >= revealUntil.current + 350) {
      setOpacity(0);
      return;
    }
    const timer = setInterval(() => {
      const next = Math.max(0, Math.min(1, 1 - (Date.now() - revealUntil.current) / 350));
      setOpacity(next);
      if (!next) clearInterval(timer);
    }, 16);
    return () => clearInterval(timer);
  }, [offset, valid]);
  function begin(e: EventPayload) {
    if (e.button !== 0 || !track.current) return;
    const bounds = renderer?.getElementBounds?.(track.current.id);
    if (!bounds) return;
    const y = (e.y ?? 0) - bounds.y;
    const inThumb = y >= top && y <= top + height;
    const grab = inThumb ? y - top : height / 2;
    setDrag({ grab, top: bounds.y });
    if (!inThumb) onScroll(travel > 0 ? Math.max(0, Math.min(1, (y - grab) / travel)) * range : 0);
  }
  function move(e: EventPayload) {
    if (!drag) return;
    if (e.pressedButton !== 0) {
      setDrag(undefined);
      return;
    }
    onScroll(
      travel > 0
        ? Math.max(0, Math.min(1, ((e.y ?? 0) - drag.top - drag.grab) / travel)) * range
        : 0,
    );
  }
  if (!valid) return null;
  const active = hovered || Boolean(drag);
  const width = active ? 8 : 5;
  const paint = surface ?? colors.surface;
  return (
    <>
      {edgeFades && offset > 0.5 && (
        <div
          style={{
            position: 'absolute',
            top: 0,
            left: 0,
            right: 0,
            height: 18,
            pointerEvents: 'none',
            background: {
              type: 'linear-gradient',
              angle: 180,
              stops: [
                { color: paint, position: 0 },
                { color: 'transparent', position: 1 },
              ],
            },
          }}
        />
      )}
      {edgeFades && range - offset > 0.5 && (
        <div
          style={{
            position: 'absolute',
            bottom: 0,
            left: 0,
            right: 0,
            height: 18,
            pointerEvents: 'none',
            background: {
              type: 'linear-gradient',
              angle: 180,
              stops: [
                { color: 'transparent', position: 0 },
                { color: paint, position: 1 },
              ],
            },
          }}
        />
      )}
      <div
        ref={track}
        role="scrollbar"
        aria-label="Scroll"
        aria-valuetext={`${Math.round(offset)} of ${Math.round(range)}`}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        onMouseDown={begin}
        onMouseMove={move}
        onMouseUp={() => setDrag(undefined)}
        style={{ position: 'absolute', top: 0, right: 0, width: 11, height: viewport }}
      >
        <div
          style={{
            position: 'absolute',
            top,
            right: 2,
            width,
            height,
            borderRadius: width / 2,
            backgroundColor: active ? colors.text_tertiary : colors.text_ghost,
            opacity: active ? 1 : opacity * 0.55,
            pointerEvents: 'none',
          }}
        />
      </div>
      {drag && (
        <anchored position={{ x: 0, y: 0 }} deferred priority={8} occlude>
          <div
            onMouseMove={move}
            onMouseUp={() => setDrag(undefined)}
            style={{
              width: window.width,
              height: window.height,
              pointerEvents: 'auto',
              cursor: 'default',
            }}
          />
        </anchored>
      )}
    </>
  );
}

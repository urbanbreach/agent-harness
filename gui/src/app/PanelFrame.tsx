import { useEffect, useRef, useState, type ReactNode } from 'react';
import type { EventPayload } from '@gpuix/react';
import { useTheme } from '../theme';

export function Panel({
  side,
  width,
  visible,
  children,
  onResize,
  onDraggingChange,
  resizing = false,
  keepMounted = false,
}: {
  side: 'left' | 'right';
  width: number;
  visible: boolean;
  children: ReactNode;
  onResize(width: number): void;
  onDraggingChange?(dragging: boolean): void;
  resizing?: boolean;
  keepMounted?: boolean;
}) {
  const { colors, reducedMotion } = useTheme();
  const [painted, setPainted] = useState(visible ? width : 0);
  const current = useRef(painted);
  const [hover, setHover] = useState(false);
  const drag = useRef<{ x: number; width: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  useEffect(() => {
    const target = visible ? width : 0;
    if (reducedMotion || resizing || dragging) {
      current.current = target;
      setPainted(target);
      return;
    }
    const initial = current.current;
    if (Math.abs(target - initial) < 0.01) return;
    const start = performance.now();
    const timer = setInterval(() => {
      const progress = Math.min(1, (performance.now() - start) / 200);
      current.current = initial + (target - initial) * (1 - (1 - progress) ** 5);
      setPainted(current.current);
      if (progress === 1) clearInterval(timer);
    }, 16);
    return () => clearInterval(timer);
  }, [visible, width, reducedMotion, resizing, dragging]);
  const hidden = painted <= 0.01 && !visible;
  if (hidden && !keepMounted) return null;
  const down = (event: EventPayload) => {
    if (event.button !== 0) return;
    drag.current = { x: event.x ?? 0, width: current.current };
    onResize(current.current);
    onDraggingChange?.(true);
    setDragging(true);
    setHover(true);
  };
  return (
    <div
      style={{
        display: hidden ? 'none' : 'flex',
        width: painted,
        height: '100%',
        flexShrink: 0,
        position: 'relative',
      }}
    >
      <div style={{ width: '100%', height: '100%', overflow: 'hidden', position: 'relative' }}>
        <div
          style={{
            width,
            height: '100%',
            position: 'absolute',
            ...(side === 'right' ? { right: 0 } : { left: 0 }),
          }}
        >
          {children}
        </div>
      </div>
      <div
        onMouseEnter={() => setHover(true)}
        onMouseLeave={() => {
          if (!drag.current) setHover(false);
        }}
        onMouseDown={down}
        onMouseMove={(event) => {
          if (!drag.current) return;
          const delta = (event.x ?? drag.current.x) - drag.current.x;
          const next = drag.current.width + (side === 'left' ? delta : -delta);
          if (Math.abs(next - width) >= 0.5) onResize(next);
        }}
        onMouseUp={() => {
          drag.current = null;
          setDragging(false);
          onDraggingChange?.(false);
          setHover(false);
        }}
        style={{
          position: 'absolute',
          top: 0,
          bottom: 0,
          width: side === 'left' ? 10 : 8,
          ...(side === 'left' ? { right: -5 } : { left: -7 }),
          cursor: 'col-resize',
        }}
      >
        <div
          style={{
            position: 'absolute',
            left: 5,
            width: 2,
            top: 0,
            bottom: 0,
            backgroundColor: hover ? colors.resize_handle : 'transparent',
          }}
        />
      </div>
    </div>
  );
}

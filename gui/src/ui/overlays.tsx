import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import {
  Dialog as NativeDialog,
  DialogPortal,
  DialogBackdrop,
  DialogPopup,
  useWindowSize,
  useGpuix,
  type PublicInstance,
  type StyleDesc,
} from '@gpuix/react';
import { useTheme } from '../theme';
import { bare, Icon, IconButton, Tooltip, row } from './controls';

export type ToastKind = 'alert' | 'success';
export interface ToastMessage {
  id: number;
  message: string;
  kind: ToastKind;
}
interface ToastContextValue {
  current?: ToastMessage;
  showToast: (message: string, kind?: ToastKind) => void;
  clearToast: () => void;
}
const ToastContext = createContext<ToastContextValue | undefined>(undefined);
export function ToastProvider({ children }: { children: ReactNode }) {
  const [current, setCurrent] = useState<ToastMessage>();
  const serial = useRef(0);
  const showToast = useCallback(
    (message: string, kind: ToastKind = 'alert') =>
      setCurrent({ id: ++serial.current, message, kind }),
    [],
  );
  const clearToast = useCallback(() => setCurrent(undefined), []);
  return (
    <ToastContext.Provider value={{ current, showToast, clearToast }}>
      {children}
    </ToastContext.Provider>
  );
}
export function useToast() {
  const value = useContext(ToastContext);
  if (!value) throw new Error('useToast requires ToastProvider');
  return value;
}
export function ToastHost() {
  const { current, clearToast } = useToast();
  return current ? <Toast key={current.id} {...current} onDismiss={clearToast} /> : null;
}
export function Toast({
  message,
  kind = 'alert',
  onDismiss,
}: {
  message: string;
  kind?: ToastKind;
  onDismiss: () => void;
  id?: number;
}) {
  const { colors, sp, reducedMotion } = useTheme();
  const { renderer } = useGpuix();
  const anchor = useRef<PublicInstance>(null);
  const [position, setPosition] = useState<{ x: number; y: number }>();
  const [hover, setHover] = useState(false);
  const remaining = useRef(5000);
  const started = useRef(0);
  const [availableWidth, setAvailableWidth] = useState(560);
  useEffect(() => {
    remaining.current = 5000;
  }, [message]);
  useEffect(() => {
    if (hover) return;
    started.current = Date.now();
    const timer = setTimeout(onDismiss, remaining.current);
    return () => {
      clearTimeout(timer);
      remaining.current = Math.max(800, remaining.current - (Date.now() - started.current));
    };
  }, [hover, message, onDismiss]);
  return (
    <div
      ref={anchor}
      onLayout={(event) => {
        if (event.width !== undefined) setAvailableWidth(Math.min(560, event.width));
        const bounds = anchor.current && renderer?.getElementBounds?.(anchor.current.id);
        if (bounds) {
          const x = bounds.x + bounds.width / 2;
          const y = bounds.y;
          setPosition((current) => (current?.x === x && current.y === y ? current : { x, y }));
        }
      }}
      style={{
        position: 'absolute',
        top: 0,
        left: 20,
        right: 20,
        height: 1,
        ...row,
        justifyContent: 'center',
        pointerEvents: 'none',
      }}
    >
      <anchored
        position={position}
        side="bottom"
        align="center"
        gap={55.5}
        fit="snap"
        snapMargin={20}
        deferred
        priority={5}
        occlude
      >
        <div
          role="status"
          aria-label={message}
          onMouseEnter={() => setHover(true)}
          onMouseLeave={() => setHover(false)}
          motion={{
            initial: reducedMotion ? false : { top: -8, opacity: 0.4 },
            animate: { top: 0, opacity: 1 },
            transition: { duration: reducedMotion ? 0 : 0.15, ease: [0.22, 1, 0.36, 1] },
          }}
          style={{
            ...row,
            position: 'relative',
            maxWidth: availableWidth,
            minWidth: 0,
            paddingLeft: 10,
            paddingRight: 10,
            paddingTop: 7,
            paddingBottom: 7,
            gap: 8,
            borderRadius: 10,
            borderWidth: 1,
            borderColor: colors.border_strong,
            backgroundColor: colors.raised,
            pointerEvents: 'auto',
            boxShadow: {
              offsetX: 0,
              offsetY: 10,
              blurRadius: 15,
              spreadRadius: -3,
              color: '#0000001a',
            },
          }}
        >
          <Icon
            name={kind === 'alert' ? 'alert' : 'check'}
            size={14}
            color={kind === 'alert' ? colors.danger : colors.success}
          />
          <text
            style={{
              flexShrink: 1,
              minWidth: 0,
              fontSize: sp(12.5),
              lineHeight: sp(16),
              whiteSpace: 'normal',
              color: colors.text,
              userSelect: 'text',
            }}
          >
            {message}
          </text>
          <Tooltip content="Dismiss notification">
            <IconButton
              name="x"
              label="Dismiss notification"
              size={12}
              style={{ width: 26, height: 26 }}
              onClick={onDismiss}
              onKeyDown={(e) => {
                if (bare(e) && e.key === 'escape') onDismiss();
              }}
            />
          </Tooltip>
        </div>
      </anchored>
    </div>
  );
}
export interface DialogProps {
  open: boolean;
  onClose: () => void;
  title?: string;
  children: ReactNode;
  style?: StyleDesc;
  closeOnOutside?: boolean;
  scrimColor?: string;
}
export function Dialog({
  open,
  onClose,
  title,
  children,
  style,
  closeOnOutside = true,
  scrimColor = 'rgba(0,0,0,.4)',
}: DialogProps) {
  const { colors } = useTheme();
  const size = useWindowSize();
  if (!open) return null;
  return (
    <NativeDialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      disablePointerDismissal={!closeOnOutside}
    >
      <DialogPortal>
        <DialogBackdrop style={{ backgroundColor: scrimColor }} />
        <DialogPopup
          aria-label={title}
          style={{
            display: 'flex',
            flexDirection: 'column',
            flexShrink: 0,
            width: 560,
            minHeight: 0,
            borderRadius: 14,
            borderWidth: 1,
            borderColor: colors.border,
            maxWidth: Math.max(0, size.width - 40),
            maxHeight: Math.max(0, size.height - 40),
            backgroundColor: colors.raised,
            color: colors.text,
            ...style,
          }}
        >
          {children}
        </DialogPopup>
      </DialogPortal>
    </NativeDialog>
  );
}

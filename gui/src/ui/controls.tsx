import { forwardRef, useEffect, useRef, useState, type ReactNode } from 'react';
import {
  buttonProps,
  Tooltip as NativeTooltip,
  TooltipTrigger,
  TooltipContent,
  useGpuix,
  type EventPayload,
  type InputProps,
  type Props,
  type PublicInstance,
  type StyleDesc,
} from '@gpuix/react';
import { useTheme } from '../theme';
import { nativeRenderer, type InputCommand, type NativeEvent } from '../native';
import { t } from '../i18n';
import { Menu, type MenuItem } from './menus';

export const row: StyleDesc = { display: 'flex', flexDirection: 'row', alignItems: 'center' };
export const ellipsis: StyleDesc = {
  minWidth: 0,
  flexShrink: 1,
  whiteSpace: 'nowrap',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
};
export function bare(event: EventPayload) {
  const m = event.modifiers;
  return !m || !(m.alt || m.cmd || m.ctrl || m.shift);
}
export function activation(event: EventPayload) {
  return bare(event) && ['enter', 'space', ' '].includes(event.key ?? '');
}
export function Icon({ name, size = 13, color }: { name: string; size?: number; color?: string }) {
  const { colors, sp } = useTheme();
  return (
    <svg
      src={
        new URL(`../../assets/icons/${name.replace(/\.svg$/, '')}.svg`, import.meta.url).pathname
      }
      style={{
        width: sp(size),
        height: sp(size),
        flexShrink: 0,
        color: color ?? colors.text_tertiary,
      }}
    />
  );
}
export interface ButtonProps extends Omit<Props, 'onClick'> {
  disabled?: boolean;
  focusableWhenDisabled?: boolean;
  onClick?: (event: EventPayload) => void;
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
}
export const Button = forwardRef<PublicInstance, ButtonProps>(function Button(
  {
    children,
    disabled = false,
    focusableWhenDisabled,
    onClick,
    onKeyDown,
    onKeyUp,
    style,
    variant = 'ghost',
    ...props
  },
  ref,
) {
  const { colors } = useTheme();
  const fill =
    variant === 'primary' ? colors.inverse : variant === 'secondary' ? colors.raised : undefined;
  return (
    <div
      {...props}
      {...buttonProps({
        disabled,
        focusableWhenDisabled,
        tabIndex: props.tabIndex ?? 0,
        onClick,
        onKeyDown: (e) => {
          onKeyDown?.(e);
          if (!bare(e) && ['enter', 'space', ' '].includes(e.key ?? '')) e.preventDefault();
        },
        onKeyUp: (e) => {
          onKeyUp?.(e);
          if (!bare(e) && ['enter', 'space', ' '].includes(e.key ?? '')) e.preventDefault();
        },
      })}
      ref={ref}
      role={props.role ?? 'button'}
      style={{
        ...row,
        justifyContent: 'center',
        cursor: 'default',
        userSelect: 'none',
        backgroundColor: fill,
        color:
          variant === 'primary'
            ? colors.on_inverse
            : variant === 'danger'
              ? colors.danger
              : colors.text_secondary,
        opacity: disabled ? 0.55 : 1,
        borderWidth: 1,
        borderColor: 'transparent',
        keyboardFocusDim: false,
        ...style,
        hover: disabled
          ? undefined
          : (style?.hover ?? {
              backgroundColor: variant === 'primary' ? colors.inverse : colors.overlay,
            }),
        active: disabled
          ? undefined
          : (style?.active ?? { backgroundColor: colors.overlay_strong }),
        focusVisible: { borderColor: colors.accent, ...style?.focusVisible },
      }}
    >
      {children}
    </div>
  );
});
export interface IconButtonProps extends ButtonProps {
  name: string;
  size?: number;
  color?: string;
  label?: string;
}
export const IconButton = forwardRef<PublicInstance, IconButtonProps>(function IconButton(
  { name, size = 13, color, label, style, ...props },
  ref,
) {
  return (
    <Button
      {...props}
      ref={ref}
      aria-label={label ?? props['aria-label'] ?? name}
      style={{ width: 22, height: 22, borderRadius: 6, flexShrink: 0, ...style }}
    >
      <Icon name={name} size={size} color={color} />
    </Button>
  );
});
export interface MenuChipProps extends ButtonProps {
  label?: string;
  icon?: string;
  outlined?: boolean;
  open?: boolean;
  chevron?: boolean;
}
export const MenuChip = forwardRef<PublicInstance, MenuChipProps>(function MenuChip(
  { label, icon, outlined, open, chevron = true, children, style, disabled, ...props },
  ref,
) {
  const { colors, sp } = useTheme();
  return (
    <Button
      {...props}
      disabled={disabled}
      ref={ref}
      aria-expanded={open}
      style={{
        height: outlined ? 30 : 26,
        paddingLeft: outlined ? 10 : 7,
        paddingRight: outlined ? 10 : 7,
        borderRadius: outlined ? 7 : 6,
        borderColor: outlined ? colors.border_strong : 'transparent',
        backgroundColor: open ? colors.overlay : outlined ? colors.raised : undefined,
        gap: 6,
        opacity: disabled ? 0.7 : 1,
        active: {},
        ...style,
      }}
    >
      {icon && <Icon name={icon} size={12} />}
      <text
        style={{ ...ellipsis, color: colors.text_secondary, fontSize: sp(13), lineHeight: sp(16) }}
      >
        {label ?? children}
      </text>
      {chevron && <Icon name="chevron-down" size={10.5} color={colors.text_ghost} />}
    </Button>
  );
});
export function Switch({
  checked,
  onChange,
  disabled = false,
  label,
  style,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  label: string;
  style?: StyleDesc;
}) {
  const { colors } = useTheme();
  return (
    <Button
      role="switch"
      aria-label={label}
      aria-valuetext={checked ? 'On' : 'Off'}
      disabled={disabled}
      focusableWhenDisabled
      onClick={() => onChange(!checked)}
      style={{
        width: 36,
        height: 20,
        padding: 2,
        borderRadius: 20,
        flexShrink: 0,
        justifyContent: checked ? 'flex-end' : 'flex-start',
        backgroundColor: checked ? colors.inverse : colors.inset,
        borderColor: checked ? colors.inverse : colors.border_strong,
        hover: {},
        active: {},
        ...style,
      }}
    >
      <div
        style={{
          width: 14,
          height: 14,
          borderRadius: 14,
          backgroundColor: checked ? colors.on_inverse : colors.text_tertiary,
        }}
      />
    </Button>
  );
}
export interface TextFieldProps extends Omit<InputProps, 'onChange'> {
  icon?: string;
  iconSize?: number;
  onChange?: (value: string) => void;
}
export const TextField = forwardRef<PublicInstance, TextFieldProps>(function TextField(
  { icon, iconSize = 13, style, onChange, onFocus, onBlur, theme, ...props },
  ref,
) {
  const { colors, sp } = useTheme();
  const { renderer } = useGpuix();
  const [focused, setFocused] = useState(false);
  const input = useRef<PublicInstance>(null);
  const text = useRef(props.value ?? '');
  const selection = useRef({ start: 0, end: 0 });
  const [editMenu, setEditMenu] = useState<{
    items: MenuItem[];
    position: { x: number; y: number };
  }>();
  useEffect(() => {
    if (props.value !== undefined) text.current = props.value;
  }, [props.value]);
  useEffect(() => {
    if (!renderer || !input.current) return;
    const id = input.current.id;
    const native = nativeRenderer(renderer);
    native.setInputSelectionListener(id, (error, event) => {
      if (!error && event.selectionStart !== undefined && event.selectionEnd !== undefined)
        selection.current = { start: event.selectionStart, end: event.selectionEnd };
    });
    return () => native.setInputSelectionListener(id, null);
  }, [renderer]);
  function openEditMenu(event: EventPayload) {
    if (!renderer || !input.current) return;
    const native = nativeRenderer(renderer);
    const id = input.current.id;
    const selected = selection.current.start !== selection.current.end;
    const all =
      Math.min(selection.current.start, selection.current.end) === 0 &&
      Math.max(selection.current.start, selection.current.end) === text.current.length;
    const clipboard = native.readClipboardText();
    const bounds = renderer.getElementBounds?.(id);
    renderer.focusElement?.(id);
    const command = (name: InputCommand) => {
      renderer.focusElement?.(id);
      native.inputCommand(id, name);
    };
    setEditMenu({
      position: {
        x: event.x ?? bounds?.x ?? 0,
        y: event.y ?? (bounds?.y ?? 0) + (bounds?.height ?? 0),
      },
      items: [
        { id: 'cut', label: t('action.cut'), disabled: !selected, onSelect: () => command('cut') },
        {
          id: 'copy',
          label: t('action.copy'),
          disabled: !selected,
          onSelect: () => command('copy'),
        },
        {
          id: 'paste',
          label: t('action.paste'),
          disabled: !clipboard,
          onSelect: () => command('paste'),
        },
        { id: 'separator', type: 'separator' },
        {
          id: 'selectAll',
          label: t('action.selectAll'),
          disabled: !text.current.length || all,
          onSelect: () => command('selectAll'),
        },
      ],
    });
  }
  return (
    <div
      style={{
        ...row,
        position: 'relative',
        height: 28,
        paddingLeft: 8,
        paddingRight: 8,
        gap: 6,
        borderRadius: 6,
        borderWidth: 1,
        borderColor: focused || editMenu ? colors.accent : colors.border_strong,
        backgroundColor: colors.inset,
        minWidth: 0,
        ...style,
      }}
    >
      {icon && <Icon name={icon} size={iconSize} />}
      <input
        {...props}
        ref={(value) => {
          input.current = value;
          if (typeof ref === 'function') ref(value);
          else if (ref) ref.current = value;
        }}
        tabIndex={props.tabIndex ?? 0}
        onMouseDown={(event) => {
          props.onMouseDown?.(event);
          if (event.button === 2) openEditMenu(event);
        }}
        onKeyDown={(event) => {
          if (
            event.key === 'f10' &&
            event.modifiers?.shift &&
            !event.modifiers.ctrl &&
            !event.modifiers.alt &&
            !event.modifiers.cmd
          ) {
            event.preventDefault();
            event.stopPropagation();
            openEditMenu(event);
          } else props.onKeyDown?.(event);
        }}
        onFocus={(e) => {
          setFocused(true);
          onFocus?.(e);
        }}
        onBlur={(e) => {
          setFocused(false);
          onBlur?.(e);
        }}
        onChange={(event) => {
          const e = event as NativeEvent;
          text.current = (e.value ?? '').replace(/[\r\n]/g, ' ');
          if (e.selectionStart !== undefined && e.selectionEnd !== undefined)
            selection.current = { start: e.selectionStart, end: e.selectionEnd };
          onChange?.(text.current);
        }}
        theme={{
          appearance: colors.is_dark ? 'dark' : 'light',
          text: colors.text,
          textMuted: colors.text_ghost,
          caret: colors.accent,
          bg: colors.inset,
          accent: colors.inverse,
          ...{
            selectionWash: colors.is_dark ? 'rgba(231,233,236,.18)' : 'rgba(32,34,39,.18)',
            caretWidth: 1.5,
            placeholder: colors.text_ghost,
          },
          ...theme,
        }}
        style={{
          flexGrow: 1,
          flexShrink: 1,
          minWidth: 0,
          height: sp(16),
          fontSize: sp(12.5),
          lineHeight: sp(16),
          color: colors.text,
          cursor: 'text',
          overflow: 'hidden',
          whiteSpace: 'nowrap',
          backgroundColor: 'transparent',
        }}
      />
      {editMenu && (
        <Menu
          items={editMenu.items}
          position={editMenu.position}
          restoreFocusId={input.current?.id}
          onClose={() => setEditMenu(undefined)}
        />
      )}
    </div>
  );
});
export function Tooltip({ children, content }: { children: ReactNode; content: ReactNode }) {
  const { colors, sp } = useTheme();
  return (
    <NativeTooltip delayDuration={500} disableHoverableContent>
      <TooltipTrigger asChild>{children}</TooltipTrigger>
      <TooltipContent
        sideOffset={4}
        alignOffset={2}
        collisionPadding={8}
        style={{
          ...row,
          paddingLeft: 7,
          paddingRight: 7,
          paddingTop: 4,
          paddingBottom: 4,
          gap: 6,
          borderRadius: 6,
          borderWidth: 1,
          borderColor: colors.border_strong,
          backgroundColor: colors.raised,
          color: colors.text_secondary,
          fontSize: sp(12.5),
          lineHeight: sp(15),
          whiteSpace: 'nowrap',
          boxShadow: {
            offsetX: 0,
            offsetY: 4,
            blurRadius: 6,
            spreadRadius: -1,
            color: '#0000001a',
          },
        }}
      >
        {content}
      </TooltipContent>
    </NativeTooltip>
  );
}
export interface SegmentOption {
  value: string;
  label: string;
  disabled?: boolean;
}
export function Segmented({
  value,
  options,
  onChange,
  style,
  itemStyle,
  label,
}: {
  value: string;
  options: SegmentOption[];
  onChange: (value: string) => void;
  style?: StyleDesc;
  itemStyle?: StyleDesc;
  label?: string;
}) {
  const { colors } = useTheme();
  const { renderer } = useGpuix();
  const refs = useRef<Array<PublicInstance | null>>([]);
  function move(index: number, key: string) {
    const enabled = options.map((o, i) => (o.disabled ? -1 : i)).filter((i) => i >= 0);
    if (!enabled.length) return;
    const at = enabled.indexOf(index);
    const next =
      key === 'home'
        ? enabled[0]
        : key === 'end'
          ? enabled[enabled.length - 1]
          : enabled[
              (at + (key === 'left' || key === 'up' ? -1 : 1) + enabled.length) % enabled.length
            ];
    if (next !== undefined) {
      onChange(options[next]!.value);
      const target = refs.current[next];
      if (target) renderer?.focusElement?.(target.id);
    }
  }
  return (
    <div role="radiogroup" aria-label={label} style={{ ...row, ...style }}>
      {options.map((option, i) => (
        <Button
          key={option.value}
          ref={(el) => {
            refs.current[i] = el;
          }}
          role="radio"
          aria-selected={value === option.value}
          tabIndex={value === option.value ? 0 : -1}
          disabled={option.disabled}
          onClick={() => onChange(option.value)}
          onKeyDown={(e) => {
            if (bare(e) && ['left', 'right', 'up', 'down', 'home', 'end'].includes(e.key ?? ''))
              move(i, e.key!);
          }}
          style={{
            color: value === option.value ? colors.text : colors.text_secondary,
            backgroundColor: value === option.value ? colors.overlay_strong : undefined,
            ...itemStyle,
          }}
        >
          {option.label}
        </Button>
      ))}
    </div>
  );
}
export function Badge({ children, style, ...props }: Props) {
  return (
    <div {...props} style={{ ...row, flexShrink: 0, ...style }}>
      {children}
    </div>
  );
}
export function Kbd({ children, style, ...props }: Props) {
  return (
    <text {...props} style={{ whiteSpace: 'nowrap', flexShrink: 0, ...style }}>
      {children}
    </text>
  );
}
const spinnerEpoch = Date.now();
export function Spinner({
  size = 13,
  color,
  label = 'Loading',
}: {
  size?: number;
  color?: string;
  label?: string;
}) {
  const { reducedMotion, colors, sp } = useTheme();
  const [phase, setPhase] = useState(() => (Date.now() - spinnerEpoch) / 900);
  useEffect(() => {
    if (reducedMotion) return;
    const timer = setInterval(() => setPhase((Date.now() - spinnerEpoch) / 900), 16);
    return () => clearInterval(timer);
  }, [reducedMotion]);
  const angle = reducedMotion ? 0 : phase * Math.PI * 2;
  const x = 12 + 9 * Math.sin(angle);
  const y = 12 - 9 * Math.cos(angle);
  return (
    <svg
      role="status"
      aria-label={label}
      source={`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M ${x} ${y} A 9 9 0 1 1 ${12 + 9 * Math.sin(angle + 5)} ${12 - 9 * Math.cos(angle + 5)}" fill="none" stroke="white" stroke-width="2" stroke-linecap="round"/></svg>`}
      style={{
        width: sp(size),
        height: sp(size),
        flexShrink: 0,
        color: color ?? colors.text_tertiary,
      }}
    />
  );
}

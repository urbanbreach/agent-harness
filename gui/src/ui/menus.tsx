import { useEffect, useRef, useState, type ReactNode } from 'react';
import {
  DismissableLayer,
  flushSync,
  useGpuix,
  type EventPayload,
  type KeyEvent,
  type Props,
  type PublicInstance,
  type StyleDesc,
} from '@gpuix/react';
import { useTheme } from '../theme';
import { bare, activation, Icon, MenuChip, row, ellipsis, type MenuChipProps } from './controls';
import type { IconName } from './icon-names';

export interface MenuItem {
  id: string;
  label?: string;
  icon?: IconName;
  disabled?: boolean;
  selected?: boolean;
  shortcut?: string;
  value?: string;
  onSelect?: () => void;
  items?: MenuItem[];
  type?: 'item' | 'separator' | 'caption' | 'custom';
  content?: ReactNode;
}
export interface MenuProps {
  items: MenuItem[];
  onClose: () => void;
  style?: StyleDesc;
  itemStyle?: StyleDesc;
  restoreFocusId?: number;
  ignoreOutsideId?: number;
  position?: { x: number; y: number };
  side?: 'top' | 'bottom' | 'right';
  flyout?: boolean;
  autoHighlight?: boolean;
  onReturn?: () => void;
}
export function Menu({
  items,
  onClose,
  style,
  itemStyle,
  restoreFocusId,
  ignoreOutsideId,
  position,
  side = 'bottom',
  flyout = false,
  autoHighlight = false,
  onReturn,
}: MenuProps) {
  const { colors, sp } = useTheme();
  const { renderer } = useGpuix();
  const card = useRef<PublicInstance>(null);
  const [highlight, setHighlight] = useState(() =>
    autoHighlight
      ? items.findIndex(
          (item) =>
            (!item.type || item.type === 'item') && !item.disabled && !(flyout && item.items),
        )
      : -1,
  );
  const [submenu, setSubmenu] = useState<string>();
  const enabled = items
    .map((item, i) =>
      (!item.type || item.type === 'item') && !item.disabled && !(flyout && item.items) ? i : -1,
    )
    .filter((i) => i >= 0);
  useEffect(() => {
    if (submenu && !items.some((item) => item.id === submenu && item.items?.length)) {
      setSubmenu(undefined);
      if (card.current) renderer?.focusElement?.(card.current.id);
    }
  }, [items, submenu, renderer]);
  function close() {
    onClose();
  }
  function select(index: number) {
    const item = items[index];
    if (!item || item.disabled || (flyout && item.items)) return;
    if (item.items) {
      setSubmenu(item.id);
      return;
    }
    flushSync(close);
    item.onSelect?.();
  }
  function keys(event: KeyEvent) {
    if (!bare(event)) return;
    const key = event.key;
    if (
      ['escape', 'left', 'right', 'enter', 'space', ' ', 'down', 'up', 'home', 'end'].includes(
        key ?? '',
      )
    ) {
      event.preventDefault();
      event.stopPropagation();
    }
    if (key === 'escape') {
      event.preventDefault();
      event.stopPropagation();
      close();
      return;
    }
    if (key === 'left') {
      if (flyout) onReturn?.();
      else setSubmenu(undefined);
      return;
    }
    if (key === 'right') {
      if (!flyout && items[highlight]?.items) setSubmenu(items[highlight]!.id);
      return;
    }
    if (activation(event)) {
      select(highlight < 0 ? (enabled[0] ?? -1) : highlight);
      return;
    }
    if (['down', 'up', 'home', 'end'].includes(key ?? '')) {
      const at = enabled.indexOf(highlight);
      const next =
        key === 'home'
          ? enabled[0]
          : key === 'end'
            ? enabled[enabled.length - 1]
            : at < 0
              ? key === 'up'
                ? enabled.at(-1)
                : enabled[0]
              : enabled[(at + (key === 'up' ? -1 : 1) + enabled.length) % enabled.length];
      setHighlight(next ?? -1);
      setSubmenu(undefined);
    }
  }
  function outside(event: EventPayload) {
    if (event.button !== 0 || submenu) return;
    for (const id of [restoreFocusId, ignoreOutsideId]) {
      if (id === undefined) continue;
      const bounds = renderer?.getElementBounds?.(id);
      if (
        bounds &&
        event.x !== undefined &&
        event.y !== undefined &&
        event.x >= bounds.x &&
        event.x <= bounds.x + bounds.width &&
        event.y >= bounds.y &&
        event.y <= bounds.y + bounds.height
      )
        return;
    }
    close();
  }
  return (
    <DismissableLayer
      initialFocus={() => card.current?.id ?? null}
      finalFocus={(previous) => restoreFocusId ?? previous}
      onEscapeKeyDown={close}
    >
      <anchored
        position={position}
        side={side}
        align="start"
        gap={position ? 0 : side === 'right' ? -4 : 4}
        fit="switch"
        snapMargin={8}
        deferred
        priority={flyout ? 3 : 2}
        occlude
      >
        <div
          ref={card}
          tabIndex={-1}
          role="menu"
          onKeyDown={keys}
          onMouseDownOutside={outside}
          style={{
            display: 'flex',
            flexDirection: 'column',
            minWidth: 176,
            maxWidth: 320,
            paddingTop: 4,
            paddingBottom: 4,
            borderRadius: 9,
            borderWidth: 1,
            borderColor: colors.border_strong,
            backgroundColor: colors.raised,
            boxShadow: {
              offsetX: 0,
              offsetY: 10,
              blurRadius: 15,
              spreadRadius: -3,
              color: '#0000001a',
            },
            ...style,
          }}
        >
          {items.map((item, i) => {
            if (item.type === 'separator')
              return (
                <div
                  key={item.id}
                  style={{
                    height: 1,
                    backgroundColor: colors.border,
                    marginLeft: 6,
                    marginRight: 6,
                    marginTop: 4,
                    marginBottom: 4,
                  }}
                />
              );
            if (item.type === 'caption')
              return (
                <text
                  key={item.id}
                  style={{
                    paddingLeft: 10,
                    paddingRight: 10,
                    paddingTop: 6,
                    paddingBottom: 2,
                    color: colors.text_tertiary,
                    fontSize: sp(12.5),
                    lineHeight: sp(14),
                    fontWeight: 500,
                  }}
                >
                  {item.label}
                </text>
              );
            if (item.type === 'custom')
              return (
                <div
                  key={item.id}
                  style={{ marginLeft: 4, marginRight: 4, paddingLeft: 8, paddingRight: 8 }}
                >
                  {item.content}
                </div>
              );
            const disabled = item.disabled || Boolean(flyout && item.items);
            return (
              <div
                key={item.id}
                role="menuitem"
                aria-label={item.label}
                aria-selected={item.selected}
                aria-expanded={item.items ? submenu === item.id : undefined}
                onMouseEnter={() => {
                  if (!disabled) {
                    setHighlight(i);
                    setSubmenu(item.items ? item.id : undefined);
                  }
                }}
                onMouseDown={(e) => {
                  if (e.button === 0 && !disabled) select(i);
                }}
                style={{
                  ...row,
                  position: 'relative',
                  minHeight: 26,
                  marginLeft: 4,
                  marginRight: 4,
                  paddingLeft: 8,
                  paddingRight: 8,
                  gap: 8,
                  borderRadius: 6,
                  cursor: 'default',
                  userSelect: 'none',
                  color: disabled
                    ? colors.text_ghost
                    : item.selected
                      ? colors.text
                      : colors.text_secondary,
                  backgroundColor: !disabled && highlight === i ? colors.overlay_strong : undefined,
                  hover: disabled ? undefined : { backgroundColor: colors.overlay },
                  fontSize: sp(12.5),
                  lineHeight: sp(15),
                  fontWeight: item.selected ? 500 : 400,
                  ...itemStyle,
                }}
              >
                {item.content ?? (
                  <>
                    {item.icon && <Icon name={item.icon} size={12} />}
                    <text style={{ ...ellipsis, flexGrow: 1 }}>{item.label}</text>
                    {item.value && (
                      <text style={{ color: colors.text_tertiary }}>{item.value}</text>
                    )}
                    {item.shortcut && (
                      <text style={{ color: colors.text_tertiary }}>{item.shortcut}</text>
                    )}
                    {item.selected && <Icon name="check" size={11} />}
                    {item.items && <Icon name="chevron-right" size={10} />}
                  </>
                )}
                {item.items && submenu === item.id && !flyout && (
                  <Menu
                    items={item.items}
                    flyout
                    side="right"
                    ignoreOutsideId={card.current?.id}
                    restoreFocusId={restoreFocusId}
                    onClose={close}
                    onReturn={() => {
                      setSubmenu(undefined);
                      if (card.current) renderer?.focusElement?.(card.current.id);
                    }}
                  />
                )}
              </div>
            );
          })}
        </div>
      </anchored>
    </DismissableLayer>
  );
}
export interface DropdownProps extends Omit<MenuChipProps, 'onClick' | 'open'> {
  items: MenuItem[];
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  menuStyle?: StyleDesc;
  menuItemStyle?: StyleDesc;
  menuSide?: MenuProps['side'];
}
export function Dropdown({
  items,
  open: controlled,
  onOpenChange,
  menuStyle,
  menuItemStyle,
  menuSide,
  disabled,
  ...props
}: DropdownProps) {
  const [internal, setInternal] = useState(false);
  const [keyboardOpen, setKeyboardOpen] = useState(false);
  const trigger = useRef<PublicInstance>(null);
  const open = controlled ?? internal;
  function change(value: boolean) {
    if (controlled === undefined) setInternal(value);
    onOpenChange?.(value);
  }
  return (
    <div style={{ display: 'flex', position: 'relative' }}>
      <MenuChip
        {...props}
        ref={trigger}
        disabled={disabled}
        open={open}
        onMouseDown={(event) => {
          setKeyboardOpen(false);
          props.onMouseDown?.(event);
        }}
        onKeyDown={(event) => {
          if (activation(event)) setKeyboardOpen(true);
          props.onKeyDown?.(event);
        }}
        onClick={() => change(!open)}
      />
      {open && (
        <Menu
          items={items}
          autoHighlight={keyboardOpen}
          style={menuStyle}
          itemStyle={menuItemStyle}
          side={menuSide}
          restoreFocusId={trigger.current?.id}
          onClose={() => change(false)}
        />
      )}
    </div>
  );
}
export function ContextMenu({
  children,
  items,
  style,
  ...props
}: Omit<Props, 'children'> & { children: ReactNode; items: MenuItem[] | (() => MenuItem[]) }) {
  const { renderer } = useGpuix();
  const [snapshot, setSnapshot] = useState<{
    items: MenuItem[];
    position: { x: number; y: number };
    focus?: number;
  }>();
  return (
    <div
      {...props}
      tabIndex={props.tabIndex ?? 0}
      style={style}
      onKeyDown={(e) => {
        if (
          e.key === 'f10' &&
          e.modifiers?.shift &&
          !e.modifiers.ctrl &&
          !e.modifiers.alt &&
          !e.modifiers.cmd
        ) {
          e.preventDefault();
          e.stopPropagation();
          const bounds = renderer?.getElementBounds?.(e.elementId);
          setSnapshot({
            items: typeof items === 'function' ? items() : items,
            position: { x: bounds?.x ?? 0, y: (bounds?.y ?? 0) + (bounds?.height ?? 0) },
            focus: renderer?.getFocusedElementId?.() ?? undefined,
          });
        } else props.onKeyDown?.(e);
      }}
      onMouseDown={(e) => {
        props.onMouseDown?.(e);
        if (e.button === 2)
          setSnapshot({
            items: typeof items === 'function' ? items() : items,
            position: { x: e.x ?? 0, y: e.y ?? 0 },
            focus: renderer?.getFocusedElementId?.() ?? undefined,
          });
      }}
    >
      {children}
      {snapshot && (
        <Menu
          items={snapshot.items}
          position={snapshot.position}
          restoreFocusId={snapshot.focus}
          onClose={() => setSnapshot(undefined)}
        />
      )}
    </div>
  );
}

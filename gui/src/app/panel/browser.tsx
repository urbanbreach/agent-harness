import { useEffect, useRef, useState } from 'react';
import { t, useLocale } from '../../i18n';
import { useGpuix, type PublicInstance } from '@gpuix/react';
import { useTheme } from '../../theme';
import { IconButton, TextField, row } from '../../ui';
import { nativeRenderer } from '../../native';
import { EmptyMessage } from './shared';
export function BrowserView({ active }: { active: boolean }) {
  useLocale();
  const { colors } = useTheme();
  const { renderer } = useGpuix();
  const input = useRef<PublicInstance>(null);
  const [address, setAddress] = useState('');
  useEffect(() => {
    if (active && input.current) renderer?.focusElement?.(input.current.id);
  }, [active, renderer]);
  return (
    <div
      style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minHeight: 0 }}
      onKeyDown={(event) => {
        if (event.modifiers?.ctrl && event.key === 'l') {
          event.preventDefault();
          event.stopPropagation();
          if (input.current && renderer) {
            renderer.focusElement?.(input.current.id);
            nativeRenderer(renderer).inputSetSelection(input.current.id, 0, address.length);
          }
        }
      }}
    >
      <div
        style={{
          ...row,
          height: 42,
          paddingLeft: 10,
          paddingRight: 10,
          gap: 2,
          flexShrink: 0,
          borderBottomWidth: 1,
          borderColor: colors.border,
        }}
      >
        <IconButton name="arrow-left" size={14} disabled style={{ width: 26, height: 26 }} />
        <IconButton name="arrow-right" size={14} disabled style={{ width: 26, height: 26 }} />
        <IconButton name="rotate-cw" size={14} disabled style={{ width: 26, height: 26 }} />
        <TextField
          ref={input}
          icon="globe"
          iconSize={11}
          value={address}
          onChange={setAddress}
          placeholder={t('input.search_or_enter_address')}
          style={{ flexGrow: 1, marginLeft: 4, marginRight: 4, height: 28 }}
          onFocus={() => {
            if (input.current && renderer)
              nativeRenderer(renderer).inputSetSelection(input.current.id, 0, address.length);
          }}
          onKeyDown={(event) => {
            if (event.key === 'enter') {
              event.preventDefault();
              event.stopPropagation();
            }
          }}
        />
        <IconButton name="external-link" size={14} disabled style={{ width: 26, height: 26 }} />
      </div>
      <EmptyMessage
        title={t('browser.unavailable')}
        description={t('browser.unavailable_on_platform')}
        icon="alert"
        bottomPadding={40}
      />
    </div>
  );
}

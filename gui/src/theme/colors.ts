export const lightColors = {
  canvas: '#F6F5F6',
  sidebar: '#F3F3F3',
  sidebar_drag_background: '#F3F3F3',
  sidebar_item_background: 'rgba(20,20,20,0.06)',
  surface: '#F6F5F6',
  raised: '#ECECEC',
  composer: '#FFFFFF',
  inset: '#E6E6E6',
  terminal: '#FFFFFF',
  overlay: 'rgba(28,30,34,0.05)',
  overlay_strong: 'rgba(28,30,34,0.09)',
  border: 'rgba(28,30,34,0.08)',
  border_strong: 'rgba(28,30,34,0.15)',
  sidebar_border: 'rgba(20,20,20,0.12)',
  text: '#242424',
  text_secondary: '#666666',
  text_tertiary: '#858585',
  text_ghost: '#A4A4A4',
  accent: '#C85F44',
  resize_handle: '#2563EB',
  gauge: '#2563EB',
  selection: 'rgba(0,123,255,0.35)',
  code_text: '#9A5528',
  code_wash: 'rgba(28,30,34,0.07)',
  inverse: '#202227',
  on_inverse: '#F8F8F9',
  warning: '#A66B20',
  success: '#2F8F52',
  favorite: '#CA8A04',
  danger: '#C64A42',
  danger_soft: 'rgba(200,74,65,0.10)',
  is_dark: false,
};

export type ThemeColors = typeof lightColors;

export const darkColors: ThemeColors = {
  canvas: '#1A1A1A',
  sidebar: '#181818',
  sidebar_drag_background: '#181818',
  sidebar_item_background: 'rgba(240,240,240,0.06)',
  surface: '#1A1A1A',
  raised: '#232323',
  composer: '#212121',
  inset: '#151515',
  terminal: '#151515',
  overlay: 'rgba(227,229,232,0.05)',
  overlay_strong: 'rgba(227,229,232,0.09)',
  border: 'rgba(227,229,232,0.07)',
  border_strong: 'rgba(227,229,232,0.14)',
  sidebar_border: '#292929',
  text: '#E2E2E2',
  text_secondary: '#A3A3A3',
  text_tertiary: '#7D7D7D',
  text_ghost: '#575757',
  accent: '#E2795B',
  resize_handle: '#3B82F6',
  gauge: '#3B82F6',
  selection: 'rgba(0,123,255,0.55)',
  code_text: '#E0A882',
  code_wash: 'rgba(227,229,232,0.08)',
  inverse: '#E7E9EC',
  on_inverse: '#17181C',
  warning: '#E0B36A',
  success: '#62C987',
  favorite: '#EAB308',
  danger: '#E2726A',
  danger_soft: 'rgba(213,116,109,0.10)',
  is_dark: true,
};

export function providerColor(provider: string, colors: ThemeColors): string {
  switch (provider.toLowerCase()) {
    case 'claude':
    case 'anthropic':
      return '#D97757';
    case 'deepseek':
      return '#4D6BFE';
    default:
      return colors.is_dark ? '#F3F3F3' : '#34363B';
  }
}

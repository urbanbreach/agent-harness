import { useSyncExternalStore } from 'react';
import * as common from './common';
import * as composer from './composer';
import * as panel from './panel';
import * as settings from './settings';
import * as shell from './shell';
import * as transcript from './transcript';

export const english = {
  ...common.en,
  ...shell.en,
  ...composer.en,
  ...transcript.en,
  ...panel.en,
  ...settings.en,
} as const;

export type TranslationKey = keyof typeof english;
export type TranslationVariables = Readonly<Record<string, string | number>>;
/** The stored preference; `system` follows the process locale. */
export type Language = 'system' | 'en' | 'zh-CN' | 'ja';
export type Locale = Exclude<Language, 'system'>;

const catalogs: Record<Exclude<Locale, 'en'>, Readonly<Record<TranslationKey, string>>> = {
  'zh-CN': {
    ...common.zhCN,
    ...shell.zhCN,
    ...composer.zhCN,
    ...transcript.zhCN,
    ...panel.zhCN,
    ...settings.zhCN,
  },
  ja: {
    ...common.ja,
    ...shell.ja,
    ...composer.ja,
    ...transcript.ja,
    ...panel.ja,
    ...settings.ja,
  },
};

/**
 * System resolves Simplified Chinese and Japanese locales; everything else, including
 * Traditional Chinese, is English because only those three catalogs ship.
 */
export function resolveLanguage(
  value: Language,
  env: Readonly<Record<string, string | undefined>> = process.env,
): Locale {
  if (value !== 'system') return value;
  const raw = (env.LC_ALL || env.LC_MESSAGES || env.LANG || '').toLowerCase().replace('-', '_');
  if (raw.startsWith('ja')) return 'ja';
  if (raw.startsWith('zh')) return /^zh_(tw|hk|mo|hant)/.test(raw) ? 'en' : 'zh-CN';
  return 'en';
}

let preference: Language = 'system';
let locale: Locale = resolveLanguage(preference);
const listeners = new Set<() => void>();

/**
 * Applies the stored preference and returns the resolved locale. Safe to call while
 * rendering: subscribers are notified after the current render.
 */
export function setLanguage(value: Language): Locale {
  if (value === preference) return locale;
  preference = value;
  locale = resolveLanguage(value);
  queueMicrotask(() => listeners.forEach((listener) => listener()));
  return locale;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The stored preference, for the settings control. */
export function useLanguage(): Language {
  return useSyncExternalStore(subscribe, () => preference);
}

/** The resolved locale, for consumers outside the keyed workspace tree. */
export function useLocale(): Locale {
  return useSyncExternalStore(subscribe, () => locale);
}

/** Both placeholder syntaxes appear in the catalogs; `{AppName}` is always Harness. */
export function t(key: TranslationKey, vars: TranslationVariables = {}): string {
  const text = locale === 'en' ? english[key] : catalogs[locale][key];
  return text.replace(/%?\{([^}]+)\}/g, (placeholder, name: string) => {
    const value = vars[name] ?? (name === 'AppName' ? 'Harness' : undefined);
    return value === undefined ? placeholder : String(value);
  });
}

import { afterEach, expect, test } from 'bun:test';
import {
  english,
  resolveLanguage,
  setLanguage,
  t,
  type Locale,
  type TranslationKey,
} from '../src/i18n';

afterEach(() => {
  setLanguage('en');
});

test('system language follows the process locale and only ships Simplified Chinese', () => {
  const cases: [Record<string, string>, Locale][] = [
    [{ LANG: 'zh_CN.UTF-8' }, 'zh-CN'],
    [{ LANG: 'zh_SG.UTF-8' }, 'zh-CN'],
    [{ LANG: 'zh_TW.UTF-8' }, 'en'],
    [{ LANG: 'zh_HK.UTF-8' }, 'en'],
    [{ LANG: 'ja_JP.UTF-8' }, 'ja'],
    [{ LANG: 'fi_FI.UTF-8' }, 'en'],
    [{}, 'en'],
    // LC_ALL overrides LC_MESSAGES, which overrides LANG.
    [{ LC_ALL: 'ja_JP.UTF-8', LC_MESSAGES: 'zh_CN.UTF-8', LANG: 'en_US.UTF-8' }, 'ja'],
    [{ LC_MESSAGES: 'zh_CN.UTF-8', LANG: 'ja_JP.UTF-8' }, 'zh-CN'],
  ];
  for (const [env, expected] of cases) expect(resolveLanguage('system', env)).toBe(expected);
  expect(resolveLanguage('ja', { LANG: 'zh_CN.UTF-8' })).toBe('ja');
});

test.each(['zh-CN', 'ja'] as const)('%s preserves every English placeholder', (locale) => {
  setLanguage(locale);
  for (const [key, text] of Object.entries(english)) {
    const names = [...text.matchAll(/%?\{([^}]+)\}/g)].map((match) => match[1]!);
    const variables = Object.fromEntries(names.map((name) => [name, `__${name}__`]));
    const translated = t(key as TranslationKey, variables);
    for (const value of Object.values(variables)) expect(translated).toContain(value);
  }
});

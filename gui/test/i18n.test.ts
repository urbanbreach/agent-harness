import { afterEach, expect, test } from 'bun:test';
import { resolveLanguage, setLanguage, t, type Locale } from '../src/i18n';

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

test('labels follow the applied language and keep their placeholders', () => {
  setLanguage('zh-CN');
  expect(t('notice.saveFailed', { error: 'disk full' })).toBe('无法保存本地状态：disk full');
  setLanguage('ja');
  expect(t('action.quit')).toBe('Harness を終了');
  setLanguage('en');
  expect(t('transcript.showing_first_files', { count: 2, total: 9 })).toBe('Showing first 2 of 9');
});

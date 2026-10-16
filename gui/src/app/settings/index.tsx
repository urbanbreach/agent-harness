import { useEffect, useState, type ReactNode } from 'react';
import { useWindowSize, type StyleDesc } from '@gpuix/react';
import { useTheme } from '../../theme';
import { FONT_SIZE_CHOICES } from '../../theme/fonts';
import { Button, Dropdown, Icon, Switch, TextField } from '../../ui';
import { getAppStore, usePreferences, useSelectedProject } from '../../store';
import { english, t, useLanguage, useLocale, type TranslationKey } from '../../i18n';
import { SkillsPage } from '../skills';
import { UsagePage } from '../usage';
import { ProvidersPage } from './providers';
import { RuntimePage } from './runtime';
import { DataPage } from './data';
import { HelpPage } from './help';
import { WorktreesPage } from './worktrees';

export type SettingsPageId = string;
export const settingsPages = [
  ['general', 'General', 'settings'],
  ['appearance', 'Appearance', 'appearance'],
  ['providers', 'Providers', 'bot'],
  ['skills', 'Skills', 'package'],
  ['usage', 'Usage', 'chart-column'],
  ['runtime', 'Runtime', 'settings'],
  ['tools', 'Tools & MCP', 'server'],
  ['memory', 'Memory', 'file-text'],
  ['worktrees', 'Worktrees', 'folder'],
  ['diagnostics', 'Diagnostics', 'alert'],
  ['help', 'Help', 'file-text'],
] as const;

export function settingsPageLabel(id: string) {
  return t(`settings.${id}` as TranslationKey);
}
export function settingsPageKeywords(id: string) {
  const key = `settings.${id}_keywords` as TranslationKey;
  return `${settingsPages.find(([page]) => page === id)?.[1] ?? ''} ${settingsPageLabel(id)} ${english[key] ?? ''} ${t(key)}`;
}

export function SettingsCard({ children, style }: { children: ReactNode; style?: StyleDesc }) {
  const { colors } = useTheme();
  return (
    <div
      style={{
        backgroundColor: colors.raised,
        borderRadius: 13,
        overflow: 'hidden',
        marginTop: 15,
        ...style,
      }}
    >
      {children}
    </div>
  );
}
export function SettingsRow({
  title,
  description,
  children,
  last = false,
  leading,
  style,
}: {
  title: string;
  description?: string;
  children?: ReactNode;
  last?: boolean;
  leading?: ReactNode;
  style?: StyleDesc;
}) {
  const { colors, sp } = useTheme();
  useLocale();
  return (
    <div
      style={{
        marginLeft: 20,
        marginRight: 20,
        paddingTop: 12,
        paddingBottom: 12,
        minHeight: 60,
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        gap: leading ? 12 : 24,
        borderBottomWidth: last ? 0 : 1,
        borderColor: colors.border,
        ...style,
      }}
    >
      {leading}
      <div style={{ flexGrow: 1, minWidth: 0 }}>
        <div style={{ fontSize: sp(13.5), fontWeight: 500, color: colors.text }}>{title}</div>
        {description && (
          <div
            style={{
              fontSize: sp(12.5),
              lineHeight: sp(18),
              color: colors.text_secondary,
              marginTop: 5,
            }}
          >
            {description}
          </div>
        )}
      </div>
      {children}
    </div>
  );
}
function General() {
  const preferences = usePreferences();
  return (
    <>
      <SettingsCard>
        <SettingsRow
          title={t('settings.local_by_default')}
          description={t('settings.local_by_default_description')}
          style={{ minHeight: 74, paddingTop: 11.5, paddingBottom: 12.5 }}
          last
        />
      </SettingsCard>
      {[
        ['renderMath', t('settings.render_math'), t('settings.render_math_description')],
        ['showReasoning', t('settings.show_reasoning'), t('settings.show_reasoning_description')],
        [
          'showMessageTimes',
          t('settings.show_message_times'),
          t('settings.show_message_times_description'),
        ],
      ].map(([key, title, description]) => (
        <SettingsCard key={key}>
          <SettingsRow title={title!} description={description} last>
            <Switch
              label={title!}
              checked={preferences[key!] !== false}
              onChange={(value) => getAppStore().setPreferences({ [key!]: value })}
            />
          </SettingsRow>
        </SettingsCard>
      ))}
    </>
  );
}
function Appearance() {
  const theme = useTheme();
  const language = useLanguage();
  const languages = [
    ['system', t('language.system')],
    ['en', 'English'],
    ['zh-CN', '简体中文'],
    ['ja', '日本語'],
  ] as const;
  return (
    <SettingsCard>
      <SettingsRow title={t('settings.theme')} description={t('settings.theme_description')}>
        <Dropdown
          outlined
          menuAlign="end"
          label={
            theme.preference === 'system'
              ? t('settings.theme_system')
              : theme.preference === 'light'
                ? t('settings.theme_light')
                : t('settings.theme_dark')
          }
          style={{ width: 116, justifyContent: 'space-between' }}
          items={(['system', 'light', 'dark'] as const).map((value) => ({
            id: value,
            label: t(`settings.theme_${value}`),
            selected: theme.preference === value,
            onSelect: () => theme.setPreference(value),
          }))}
        />
      </SettingsRow>
      <SettingsRow title={t('language.title')} description={t('language.description')}>
        <Dropdown
          outlined
          menuAlign="end"
          style={{ width: 116, justifyContent: 'space-between' }}
          label={languages.find(([value]) => value === language)?.[1] ?? t('language.system')}
          items={languages.map(([value, label]) => ({
            id: value,
            label,
            selected: language === value,
            onSelect: () => getAppStore().setPreferences({ language: value }),
          }))}
        />
      </SettingsRow>
      <SettingsRow
        title={t('settings.ui_font_size')}
        description={t('settings.ui_font_size_description')}
      >
        <Dropdown
          outlined
          menuAlign="end"
          style={{ width: 116, justifyContent: 'space-between' }}
          label={`${theme.uiFontSize} px`}
          items={FONT_SIZE_CHOICES.map((size) => ({
            id: String(size),
            label: `${size} px`,
            selected: size === theme.uiFontSize,
            onSelect: () => theme.setUiFontSize(size),
          }))}
        />
      </SettingsRow>
      <SettingsRow
        title={t('settings.code_font_size')}
        description={t('settings.code_font_size_description')}
        last
      >
        <Dropdown
          outlined
          menuAlign="end"
          style={{ width: 116, justifyContent: 'space-between' }}
          label={`${theme.codeFontSize} px`}
          items={FONT_SIZE_CHOICES.map((size) => ({
            id: String(size),
            label: `${size} px`,
            selected: size === theme.codeFontSize,
            onSelect: () => theme.setCodeFontSize(size),
          }))}
        />
      </SettingsRow>
    </SettingsCard>
  );
}
export function SettingsPage({
  page = 'general',
  onClose,
}: {
  page?: SettingsPageId;
  onClose(): void;
}) {
  const { colors, sp } = useTheme();
  useLocale();
  const window = useWindowSize();
  const project = useSelectedProject();
  const [current, setCurrent] = useState(page);
  const [query, setQuery] = useState('');
  useEffect(() => setCurrent(page), [page]);
  const visible = settingsPages.filter(([id]) =>
    settingsPageKeywords(id).toLowerCase().includes(query.trim().toLowerCase()),
  );
  const cwd = project?.cwd ?? getAppStore().getSnapshot().initialized?.defaultCwd ?? '.';
  const title = settingsPageLabel(settingsPages.find(([id]) => id === current)?.[0] ?? 'general');
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'row',
        width: '100%',
        height: '100%',
        backgroundColor: colors.surface,
        color: colors.text,
      }}
    >
      <div
        style={{
          width: 253,
          flexShrink: 0,
          backgroundColor: colors.sidebar,
          borderRightWidth: 1,
          borderColor: colors.sidebar_border,
          paddingLeft: 12,
          paddingRight: 12,
          paddingTop: 12,
        }}
      >
        <Button
          onClick={onClose}
          style={{
            justifyContent: 'flex-start',
            height: 34,
            gap: 9,
            paddingLeft: 9,
            borderRadius: 8,
            fontSize: sp(13),
          }}
        >
          <Icon name="arrow-left" size={15} />
          {t('settings.back')}
        </Button>
        <TextField
          icon="search"
          placeholder={t('settings.search')}
          value={query}
          onChange={setQuery}
          style={{ marginTop: 8 }}
          onKeyDown={(event) => {
            if (!['up', 'down'].includes(event.key ?? '') || !visible.length) return;
            event.preventDefault();
            const index = visible.findIndex(([id]) => id === current);
            setCurrent(
              visible[
                ((index < 0 ? (event.key === 'up' ? 0 : -1) : index) +
                  (event.key === 'up' ? -1 : 1) +
                  visible.length) %
                  visible.length
              ]![0],
            );
          }}
        />
        <div style={{ display: 'flex', flexDirection: 'column', marginTop: 18, gap: 3 }}>
          {visible.map(([id, , icon]) => (
            <Button
              key={id}
              onClick={() => setCurrent(id)}
              style={{
                height: 36,
                paddingLeft: 10,
                gap: 10,
                justifyContent: 'flex-start',
                borderRadius: 8,
                fontSize: sp(13),
                color: current === id ? colors.text : colors.text_secondary,
                backgroundColor: current === id ? colors.sidebar_item_background : undefined,
              }}
            >
              <Icon name={icon} size={15} />
              {settingsPageLabel(id)}
            </Button>
          ))}
        </div>
      </div>
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          flexGrow: 1,
          minWidth: 0,
          minHeight: 0,
          height: '100%',
        }}
      >
        {current !== 'skills' && <div style={{ height: 48, flexShrink: 0 }} />}
        {current === 'skills' ? (
          <div style={{ display: 'flex', flexGrow: 1, minHeight: 0, marginLeft: -1 }}>
            <SkillsPage cwd={cwd} />
          </div>
        ) : (
          <div
            key={current}
            style={{
              display: 'flex',
              flexDirection: 'column',
              flexBasis: 0,
              alignItems: 'center',
              flexGrow: 1,
              minHeight: 0,
              overflowY: 'scroll',
              paddingLeft: Math.max(
                32,
                (window.width - 253 - (current === 'usage' ? 1024 : 760)) / 2,
              ),
              paddingRight: Math.max(
                32,
                (window.width - 253 - (current === 'usage' ? 1024 : 760)) / 2,
              ),
              paddingBottom: 48,
            }}
          >
            <div
              style={{ width: '100%', maxWidth: current === 'usage' ? 1024 : 760, paddingTop: 2 }}
            >
              <div style={{ fontSize: sp(18), fontWeight: 500 }}>{title}</div>
              {current === 'general' ? (
                <General />
              ) : current === 'appearance' ? (
                <Appearance />
              ) : current === 'providers' ? (
                <ProvidersPage />
              ) : current === 'runtime' ? (
                <RuntimePage cwd={cwd} />
              ) : current === 'usage' ? (
                <UsagePage cwd={cwd} />
              ) : current === 'help' ? (
                <HelpPage cwd={cwd} />
              ) : current === 'worktrees' ? (
                <WorktreesPage cwd={cwd} />
              ) : (
                <DataPage page={current} cwd={cwd} />
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

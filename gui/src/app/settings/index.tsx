import { useEffect, useState, type ReactNode } from 'react';
import { useWindowSize, type StyleDesc } from '@gpuix/react';
import { useTheme } from '../../theme';
import { Button, Dropdown, Icon, Switch, TextField } from '../../ui';
import { getAppStore, usePreferences, useSelectedProject } from '../../store';
import { setLanguage, type Language } from '../../i18n';
import { SkillsPage } from '../skills';
import { UsagePage } from '../usage';
import { ProvidersPage } from './providers';
import { RuntimePage } from './runtime';
import { DataPage } from './data';
import { useSettingsText } from './strings';
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
  const text = useSettingsText();
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
        <div style={{ fontSize: sp(13.5), fontWeight: 500, color: colors.text }}>{text(title)}</div>
        {description && (
          <div
            style={{
              fontSize: sp(12.5),
              lineHeight: sp(18),
              color: colors.text_secondary,
              marginTop: 5,
            }}
          >
            {text(description)}
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
          title="Local by default"
          description="Projects, conversations, and settings are stored on this computer"
          style={{ minHeight: 74, paddingTop: 11.5, paddingBottom: 12.5 }}
          last
        />
      </SettingsCard>
      {[
        [
          'renderMath',
          'Render math expressions',
          'Show formatted math in Markdown. Turn off to show LaTeX source.',
        ],
        ['showReasoning', 'Show reasoning', 'Show the agent’s reasoning in the conversation.'],
        [
          'showMessageTimes',
          'Show message times',
          'Display timestamps beside conversation messages.',
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
  const preferences = usePreferences();
  const sizes = [11, 12, 13, 14, 15, 16, 18, 20];
  const languages = [
    ['system', 'System'],
    ['en', 'English'],
    ['zh-CN', '简体中文'],
    ['ja', '日本語'],
  ] as const;
  return (
    <SettingsCard>
      <SettingsRow title="Theme" description="Choose between system, light, or dark themes">
        <Dropdown
          outlined
          label={
            theme.preference === 'system'
              ? 'System'
              : theme.preference === 'light'
                ? 'Light'
                : 'Dark'
          }
          style={{ width: 116, justifyContent: 'space-between' }}
          items={(['system', 'light', 'dark'] as const).map((value) => ({
            id: value,
            label: value[0]!.toUpperCase() + value.slice(1),
            selected: theme.preference === value,
            onSelect: () => theme.setPreference(value),
          }))}
        />
      </SettingsRow>
      <SettingsRow title="Language" description="Choose the language used throughout Harness">
        <Dropdown
          outlined
          style={{ width: 116, justifyContent: 'space-between' }}
          label={languages.find(([value]) => value === preferences.language)?.[1] ?? 'System'}
          items={languages.map(([value, label]) => ({
            id: value,
            label,
            selected: (preferences.language ?? 'system') === value,
            onSelect: () => {
              getAppStore().setPreferences({ language: value });
              setLanguage(value);
            },
          }))}
        />
      </SettingsRow>
      <SettingsRow title="UI font size" description="Text size across the interface and messages">
        <Dropdown
          outlined
          style={{ width: 116, justifyContent: 'space-between' }}
          label={`${theme.uiFontSize} px`}
          items={sizes.map((size) => ({
            id: String(size),
            label: `${size} px`,
            selected: size === theme.uiFontSize,
            onSelect: () => theme.setUiFontSize(size),
          }))}
        />
      </SettingsRow>
      <SettingsRow
        title="Code font size"
        description="Text size in the file editor, diffs, code blocks, and tool output"
        last
      >
        <Dropdown
          outlined
          style={{ width: 116, justifyContent: 'space-between' }}
          label={`${theme.codeFontSize} px`}
          items={sizes.map((size) => ({
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
  const text = useSettingsText();
  const window = useWindowSize();
  const project = useSelectedProject();
  const preferences = usePreferences();
  useEffect(() => {
    setLanguage(
      (['en', 'zh-CN', 'ja'].includes(String(preferences.language))
        ? preferences.language
        : 'system') as Language,
    );
  }, [preferences.language]);
  const [current, setCurrent] = useState(page);
  const [query, setQuery] = useState('');
  useEffect(() => setCurrent(page), [page]);
  const visible = settingsPages.filter(([, label]) =>
    label.toLowerCase().includes(query.trim().toLowerCase()),
  );
  const cwd = project?.cwd ?? getAppStore().getSnapshot().initialized?.defaultCwd ?? '.';
  const title = settingsPages.find(([id]) => id === current)?.[1] ?? 'General';
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
          {text('Back')}
        </Button>
        <TextField
          icon="search"
          placeholder={text('Search Settings')}
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
          {visible.map(([id, label, icon]) => (
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
              {text(label)}
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
              <div style={{ fontSize: sp(18), fontWeight: 500 }}>{text(title)}</div>
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

import { useEffect, useState, useSyncExternalStore } from 'react';
import type { Methods } from '../../backend/protocol';
import { getAppStore } from '../../store';
import { useTheme } from '../../theme';
import { english, t, type TranslationKey } from '../../i18n';
import { listActions, subscribeActions } from '../actions';
import { SettingsCard, SettingsRow } from './index';

export function HelpPage({ cwd }: { cwd: string }) {
  const { colors, sp } = useTheme();
  const [commands, setCommands] = useState<Methods['commands.list']['result']>([]);
  const [error, setError] = useState('');
  useSyncExternalStore(subscribeActions, () =>
    listActions()
      .map((action) => action.id)
      .join('\n'),
  );
  const shortcuts = listActions().filter((action) => action.shortcut);
  useEffect(() => {
    let active = true;
    void getAppStore()
      .backend.call('commands.list', { cwd })
      .then((result) => {
        if (active) setCommands(result);
      })
      .catch((error) => {
        if (active) setError(String(error));
      });
    return () => {
      active = false;
    };
  }, [cwd]);
  return (
    <>
      <SettingsCard>
        <SettingsRow
          title={t('settings.keyboard_shortcuts')}
          description={t('settings.keyboard_shortcuts_description')}
        />
        {shortcuts.map((action, index) => (
          <SettingsRow
            key={action.id}
            title={
              Object.hasOwn(english, action.label)
                ? t(action.label as TranslationKey)
                : action.label
            }
            last={index === shortcuts.length - 1}
          >
            <div style={{ fontSize: sp(12.5), color: colors.text_secondary }}>
              {action.shortcut}
            </div>
          </SettingsRow>
        ))}
      </SettingsCard>
      <SettingsCard>
        <SettingsRow
          title={t('settings.composer_shortcuts')}
          description={t('settings.composer_shortcuts_description')}
        />
        <SettingsRow
          title={t('settings.steer_turn')}
          description={t('settings.steer_turn_description')}
        />
        <SettingsRow
          title={t('settings.suggestions_editing')}
          description={t('settings.suggestions_editing_description')}
        />
        <SettingsRow
          title={t('settings.attachments')}
          description={t('settings.attachments_description')}
        />
        <SettingsRow
          title={t('settings.slash_commands')}
          description={t('settings.slash_commands_description')}
        />
        {commands.map((command, index) => (
          <SettingsRow
            key={`${command.scope}:${command.name}`}
            title={`/${command.name}`}
            description={command.description}
            last={index === commands.length - 1}
          />
        ))}
        {error && (
          <SettingsRow title={t('settings.load_commands_failed')} description={error} last />
        )}
      </SettingsCard>
    </>
  );
}

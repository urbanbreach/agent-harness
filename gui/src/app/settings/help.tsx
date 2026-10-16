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
          title="Keyboard shortcuts"
          description="Application commands and their current shortcuts."
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
          title="Composer shortcuts"
          description="Enter sends, or queues a follow-up while a turn is running. Shift+Enter inserts a newline."
        />
        <SettingsRow
          title="Steer a running turn"
          description="Ctrl+Enter sends the current draft as steering input. With an empty draft, it steers the first queued follow-up."
        />
        <SettingsRow
          title="Suggestions and editing"
          description="Up/Down selects a suggestion; Tab or Enter accepts it. Tab or Right accepts an inline completion. Escape dismisses suggestions. Shift+F10 opens the edit menu."
        />
        <SettingsRow
          title="Attachments"
          description="Ctrl+V pastes clipboard images or files. Backspace in an empty composer removes the last attachment."
        />
        <SettingsRow
          title="Slash commands"
          description="Type / in the composer to find a command. Project and user commands expand before sending."
        />
        {commands.map((command, index) => (
          <SettingsRow
            key={`${command.scope}:${command.name}`}
            title={`/${command.name}`}
            description={command.description}
            last={index === commands.length - 1}
          />
        ))}
        {error && <SettingsRow title="Could not load slash commands" description={error} last />}
      </SettingsCard>
    </>
  );
}

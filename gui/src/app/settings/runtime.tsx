import { useEffect, useState } from 'react';
import type { Json, Setting } from '../../backend/protocol';
import { getAppStore } from '../../store';
import { Button, Dropdown, Switch, TextField } from '../../ui';
import { useTheme } from '../../theme';
import { SettingsCard, SettingsRow } from './index';
import { t } from '../../i18n';

export function RuntimePage({ cwd }: { cwd: string }) {
  const { colors, sp } = useTheme();
  const backend = getAppStore().backend;
  const [settings, setSettings] = useState<Setting[]>([]);
  const [error, setError] = useState('');
  const [query, setQuery] = useState('');
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  async function load() {
    setSettings(await backend.call('settings.list', { cwd }));
  }
  useEffect(() => {
    void load().catch((error) => setError(String(error)));
  }, [cwd]);
  async function write(setting: Setting, value: Json) {
    try {
      await backend.call('settings.write', { cwd, id: setting.id, value });
      await load();
      setError('');
    } catch (error) {
      setError(String(error));
    }
  }
  const filtered = settings.filter((setting) =>
    `${setting.label} ${setting.description} ${setting.surface}`
      .toLowerCase()
      .includes(query.toLowerCase()),
  );
  return (
    <>
      <TextField
        icon="search"
        placeholder={t('settings.search_runtime')}
        value={query}
        onChange={setQuery}
        style={{ marginTop: 15 }}
      />
      {error && (
        <div style={{ color: colors.danger, marginTop: 12, fontSize: sp(12.5) }}>{error}</div>
      )}
      {[...new Set(filtered.map((setting) => setting.surface))].map((surface) => (
        <SettingsCard key={surface}>
          <SettingsRow title={surface} last />
          {filtered
            .filter((setting) => setting.surface === surface)
            .map((setting, index, group) => (
              <SettingsRow
                key={setting.id}
                title={setting.label}
                description={setting.description}
                last={index === group.length - 1}
              >
                <div
                  style={{ display: 'flex', flexDirection: 'row', gap: 8, alignItems: 'center' }}
                >
                  {typeof setting.value === 'boolean' ? (
                    <Switch
                      label={setting.label}
                      checked={setting.value}
                      disabled={!setting.editable}
                      onChange={(value) => void write(setting, value)}
                    />
                  ) : setting.choices?.length ? (
                    <Dropdown
                      outlined
                      menuAlign="end"
                      disabled={!setting.editable}
                      label={String(setting.value)}
                      items={setting.choices.map((value) => ({
                        id: value,
                        label: value,
                        selected: value === setting.value,
                        onSelect: () => void write(setting, value),
                      }))}
                    />
                  ) : (
                    <TextField
                      readOnly={!setting.editable}
                      value={
                        drafts[setting.id] ??
                        (typeof setting.value === 'string'
                          ? setting.value
                          : JSON.stringify(setting.value))
                      }
                      style={{ width: 180, opacity: setting.editable ? 1 : 0.55 }}
                      onChange={(value) =>
                        setDrafts((previous) => ({ ...previous, [setting.id]: value }))
                      }
                      onKeyDown={(event) => {
                        if (event.key !== 'enter' || !setting.editable) return;
                        const text = drafts[setting.id];
                        if (text === undefined) return;
                        try {
                          const value =
                            typeof setting.value === 'string' ? text : (JSON.parse(text) as Json);
                          void write(setting, value);
                        } catch (error) {
                          setError(String(error));
                        }
                      }}
                    />
                  )}
                  {setting.editable && (
                    <Button
                      onClick={() => {
                        void backend
                          .call('settings.reset', { cwd, id: setting.id })
                          .then(load)
                          .catch((error) => setError(String(error)));
                      }}
                      style={{ padding: 6, fontSize: sp(12.5) }}
                    >
                      {t('settings.reset')}
                    </Button>
                  )}
                </div>
              </SettingsRow>
            ))}
        </SettingsCard>
      ))}
    </>
  );
}

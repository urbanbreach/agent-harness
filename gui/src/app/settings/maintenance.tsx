import { useState } from 'react';
import { getAppStore } from '../../store';
import { Button, TextField } from '../../ui';
import { useTheme } from '../../theme';
import { SettingsCard, SettingsRow } from './index';
import { t } from '../../i18n';

export function MaintenanceActions({ cwd }: { cwd: string }) {
  const backend = getAppStore().backend;
  const { colors, sp } = useTheme();
  const [busy, setBusy] = useState(false);
  const [output, setOutput] = useState('');
  const [path, setPath] = useState('');
  async function run(action: string) {
    setBusy(true);
    setOutput('');
    try {
      let result: unknown;
      switch (action) {
        case 'index':
          result = await backend.call('diagnostics.rebuildSessionIndex', { cwd });
          break;
        case 'crash':
          result = await backend.call('diagnostics.crashScan', { cwd });
          break;
        case 'models':
          result = await backend.call('diagnostics.refreshModelCatalog', { cwd });
          break;
        case 'protocols':
          result = await backend.call('providers.protocols', { cwd });
          break;
        case 'validate':
          result = await backend.call('config.validate', { cwd });
          break;
        case 'explain':
          result = await backend.call('config.explain', { cwd, path: path.trim() });
          break;
      }
      setOutput(JSON.stringify(result, null, 2));
    } catch (error) {
      setOutput(String(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <SettingsCard>
        {[
          ['index', t('settings.rebuild_index'), t('settings.rebuild_index_description')],
          ['crash', t('settings.scan_crashes'), t('settings.scan_crashes_description')],
          ['models', t('settings.refresh_models'), t('settings.refresh_models_description')],
          [
            'protocols',
            t('settings.provider_protocols'),
            t('settings.provider_protocols_description'),
          ],
          ['validate', t('settings.validate_config'), t('settings.validate_config_description')],
        ].map(([id, title, description]) => (
          <SettingsRow key={id} title={title!} description={description}>
            <Button disabled={busy} onClick={() => void run(id!)}>
              {t('settings.run')}
            </Button>
          </SettingsRow>
        ))}
        <SettingsRow
          title={t('settings.explain_config')}
          description={t('settings.explain_config_description')}
          last
        >
          <TextField
            placeholder={t('settings.config_key')}
            value={path}
            onChange={setPath}
            style={{ width: 160 }}
          />
          <Button disabled={busy || !path.trim()} onClick={() => void run('explain')}>
            {t('settings.explain')}
          </Button>
        </SettingsRow>
      </SettingsCard>
      {(busy || output) && (
        <div
          style={{
            marginTop: 18,
            fontSize: sp(12),
            lineHeight: sp(18),
            color: colors.text_secondary,
          }}
        >
          {busy ? t('settings.running') : output}
        </div>
      )}
    </>
  );
}

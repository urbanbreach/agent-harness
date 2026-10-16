import { useState } from 'react';
import { getAppStore } from '../../store';
import { Button, TextField } from '../../ui';
import { useTheme } from '../../theme';
import { SettingsCard, SettingsRow } from './index';

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
          [
            'index',
            'Rebuild session index',
            'Recreate the searchable session index from durable session histories.',
          ],
          [
            'crash',
            'Scan crash reports',
            'Inspect locally recorded crashes without contacting a provider.',
          ],
          [
            'models',
            'Refresh model catalog',
            'Refresh provider model metadata. This does not generate an agent response.',
          ],
          [
            'protocols',
            'Provider protocols',
            'Inspect supported provider protocols and configuration.',
          ],
          [
            'validate',
            'Validate configuration',
            'Check the effective Harness configuration for errors.',
          ],
        ].map(([id, title, description]) => (
          <SettingsRow key={id} title={title!} description={description}>
            <Button disabled={busy} onClick={() => void run(id!)}>
              Run
            </Button>
          </SettingsRow>
        ))}
        <SettingsRow
          title="Explain configuration"
          description="Enter a configuration key to inspect its effective value and source."
          last
        >
          <TextField
            placeholder="Config key"
            value={path}
            onChange={setPath}
            style={{ width: 160 }}
          />
          <Button disabled={busy || !path.trim()} onClick={() => void run('explain')}>
            Explain
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
          {busy ? 'Running…' : output}
        </div>
      )}
    </>
  );
}

import { useEffect, useState } from 'react';
import type { MemoryEntry, Methods } from '../../backend/protocol';
import { getAppStore } from '../../store';
import { useTheme } from '../../theme';
import { Button, Switch, TextField, useToast } from '../../ui';
import { Markdown } from '../../markdown';
import { SettingsCard, SettingsRow } from './index';
import { MaintenanceActions } from './maintenance';

export function DataPage({ page, cwd }: { page: string; cwd: string }) {
  const { colors, sp } = useTheme();
  const { showToast } = useToast();
  const backend = getAppStore().backend;
  const [memory, setMemory] = useState<MemoryEntry[]>([]);
  const [selected, setSelected] = useState<string>();
  const [query, setQuery] = useState('');
  const [armed, setArmed] = useState<string>();
  const [toggles, setToggles] = useState<Methods['toggles.get']['result']['entries']>([]);
  const [doctor, setDoctor] = useState<Methods['app.doctor']['result']>();
  const [sources, setSources] = useState<Methods['app.configSources']['result']>();
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    let active = true;
    setLoading(true);
    const load = async () => {
      if (page === 'memory') {
        const result = await backend.call('memory.list', { cwd });
        if (active) setMemory(result.entries);
      } else if (page === 'tools') {
        const result = await backend.call('toggles.get', { cwd });
        if (active) setToggles(result.entries);
      } else {
        const [report, config] = await Promise.all([
          backend.call('app.doctor', { cwd }),
          backend.call('app.configSources', { cwd }),
        ]);
        if (active) {
          setDoctor(report);
          setSources(config);
        }
      }
    };
    void load()
      .catch((error) => {
        if (active) showToast(String(error));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [page, cwd]);
  if (loading)
    return (
      <div style={{ marginTop: 18, fontSize: sp(12.5), color: colors.text_tertiary }}>Loading…</div>
    );
  if (page === 'tools')
    return (
      <>
        <SettingsCard>
          <SettingsRow
            title="Tools & MCP"
            description="Changes apply to newly started sessions."
            last
          />
        </SettingsCard>
        {[...new Set(toggles.map((entry) => entry.kind))].map((kind) => (
          <SettingsCard key={kind}>
            {toggles
              .filter((entry) => entry.kind === kind)
              .map((entry, index, group) => (
                <SettingsRow
                  key={entry.id}
                  title={entry.name}
                  description={entry.agent ?? kind}
                  last={index === group.length - 1}
                >
                  <Switch
                    checked={entry.enabled}
                    label={entry.name}
                    onChange={(enabled) => {
                      void backend
                        .call('toggles.set', { cwd, id: entry.id, enabled })
                        .then((result) => setToggles(result.entries))
                        .catch((error) => showToast(String(error)));
                    }}
                  />
                </SettingsRow>
              ))}
          </SettingsCard>
        ))}
      </>
    );
  if (page === 'memory') {
    const filtered = memory.filter((entry) =>
      `${entry.key} ${entry.value}`.toLowerCase().includes(query.toLowerCase()),
    );
    const current = filtered.find((entry) => entry.key === selected) ?? filtered[0];
    return (
      <>
        <TextField
          icon="search"
          placeholder="Search memory…"
          value={query}
          onChange={setQuery}
          style={{ marginTop: 15 }}
        />
        <SettingsCard>
          {filtered.length ? (
            filtered.map((entry) => (
              <SettingsRow key={entry.key} title={entry.key} description={entry.scope}>
                <Button
                  onClick={() => {
                    setSelected(entry.key);
                    setArmed(undefined);
                  }}
                  style={{ padding: 8 }}
                >
                  View
                </Button>
                <Button
                  variant={armed === entry.key ? 'danger' : 'ghost'}
                  onClick={() => {
                    if (armed !== entry.key) {
                      setArmed(entry.key);
                      return;
                    }
                    void backend
                      .call('memory.delete', { cwd, key: entry.key })
                      .then(() => {
                        setMemory((previous) => previous.filter((item) => item.key !== entry.key));
                        setArmed(undefined);
                      })
                      .catch((error) => showToast(String(error)));
                  }}
                  style={{ padding: 8 }}
                >
                  {armed === entry.key ? 'Confirm delete' : 'Delete'}
                </Button>
              </SettingsRow>
            ))
          ) : (
            <SettingsRow title="No memory entries" last />
          )}
        </SettingsCard>
        {current && (
          <div style={{ marginTop: 18 }}>
            <Markdown source={current.value} />
          </div>
        )}
      </>
    );
  }
  return (
    <>
      <SettingsCard>
        <SettingsRow
          title="Local configuration diagnostics"
          description="No network probes were run. Configuration readiness does not prove provider execution or sandbox enforcement."
          last
        />
        {doctor?.checks.map((check, index, group) => (
          <SettingsRow
            key={check.name}
            title={check.name}
            description={check.message}
            last={index === group.length - 1}
          >
            <div
              style={{
                fontSize: sp(12.5),
                color: check.status === 'ok' ? colors.success : colors.warning,
              }}
            >
              {check.status}
            </div>
          </SettingsRow>
        ))}
      </SettingsCard>
      <SettingsCard>
        <SettingsRow title="Config sources" description={sources?.primaryPath ?? undefined} />
        {sources?.layers.map((layer, index, group) => (
          <SettingsRow
            key={`${layer.kind}:${layer.path}`}
            title={layer.kind}
            description={layer.path}
            last={index === group.length - 1}
          />
        ))}
      </SettingsCard>
      <MaintenanceActions cwd={cwd} />
    </>
  );
}

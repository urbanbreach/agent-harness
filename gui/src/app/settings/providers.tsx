import { useEffect, useRef, useState } from 'react';
import type { Notification, Provider } from '../../backend/protocol';
import { getAppStore } from '../../store';
import { Button, Icon, TextField } from '../../ui';
import { useTheme } from '../../theme';
import { openUrl, writeClipboard } from '../../workspace/os';
import { SettingsCard, SettingsRow } from './index';

export function ProvidersPage() {
  const { colors, sp } = useTheme();
  const backend = getAppStore().backend;
  const [providers, setProviders] = useState<Provider[]>([]);
  const [expanded, setExpanded] = useState<string>();
  const [checking, setChecking] = useState(false);
  const [attempt, setAttempt] = useState<number>();
  const [message, setMessage] = useState('');
  const [input, setInput] = useState('');
  const [error, setError] = useState('');
  const [apiKeyProvider, setApiKeyProvider] = useState<string>();
  const [accountId, setAccountId] = useState('');
  const [accountName, setAccountName] = useState('');
  const activeAttempt = useRef<number | undefined>(undefined);
  const starting = useRef(false);
  const earlyUpdates = useRef<Extract<Notification, { method: 'auth.update' }>['params'][]>([]);
  function report(error: unknown) {
    setError(String(error));
  }
  async function refresh() {
    setChecking(true);
    try {
      setProviders(await backend.call('providers.list', {}));
      setError('');
    } catch (error) {
      report(error);
    } finally {
      setChecking(false);
    }
  }
  useEffect(() => {
    void refresh();
  }, []);
  function applyUpdate(update: Extract<Notification, { method: 'auth.update' }>['params']) {
    if (update.attempt !== activeAttempt.current) return;
    setMessage(update.message);
    if (update.kind === 'result') {
      activeAttempt.current = undefined;
      setAttempt(undefined);
      setInput('');
      if (update.success) void refresh();
    }
  }
  useEffect(
    () =>
      backend.subscribe((notification) => {
        if (notification.method !== 'auth.update') return;
        if (starting.current) earlyUpdates.current.push(notification.params);
        else applyUpdate(notification.params);
      }),
    [backend],
  );
  async function start(args: string[], stdin?: string) {
    starting.current = true;
    earlyUpdates.current = [];
    try {
      setMessage('Starting sign-in…');
      const result = await backend.call('auth.start', { args, stdin });
      activeAttempt.current = result.attempt;
      setAttempt(result.attempt);
      for (const update of earlyUpdates.current) applyUpdate(update);
    } catch (error) {
      report(error);
    } finally {
      starting.current = false;
      earlyUpdates.current = [];
    }
  }
  const url = message.match(/https?:\/\/[^\s<>]+/)?.[0];
  return (
    <SettingsCard>
      <SettingsRow title="Providers" description="Connect Harness to the providers you use.">
        <Button
          onClick={() => void refresh()}
          style={{ padding: 8, borderRadius: 7, borderColor: colors.border_strong, gap: 6 }}
        >
          <Icon name="rotate-cw" size={11} />
          {checking ? 'Checking…' : 'Refresh'}
        </Button>
      </SettingsRow>
      {error && (
        <div style={{ padding: 20, color: colors.danger, fontSize: sp(12.5) }}>{error}</div>
      )}
      {providers.map((provider) => (
        <div key={provider.id}>
          <SettingsRow
            title={provider.label}
            description={`${provider.connected ? 'Connected' : 'Not connected'} · ${provider.description}`}
          >
            <Button
              aria-label={`Configure ${provider.label}`}
              onClick={() => {
                setExpanded(expanded === provider.id ? undefined : provider.id);
                setMessage('');
                setInput('');
                setApiKeyProvider(undefined);
              }}
            >
              <Icon name={expanded === provider.id ? 'chevron-down' : 'chevron-right'} />
            </Button>
          </SettingsRow>
          {expanded === provider.id && (
            <div
              style={{
                marginLeft: 62,
                marginRight: 20,
                paddingTop: 10,
                paddingBottom: 16,
                gap: 8,
                fontSize: sp(12.5),
              }}
            >
              <div style={{ display: 'flex', flexDirection: 'row', gap: 8, flexWrap: 'wrap' }}>
                {provider.methods.map((method) => (
                  <Button
                    key={method.id}
                    disabled={attempt !== undefined}
                    onClick={() => {
                      if (method.id === 'api-key') {
                        setApiKeyProvider(provider.id);
                        setInput('');
                      } else void start(['login', provider.id, '--method', method.id]);
                    }}
                    style={{ padding: 8, borderRadius: 7, borderColor: colors.border_strong }}
                  >
                    {method.label}
                  </Button>
                ))}
                {provider.connected && (
                  <Button
                    onClick={() => {
                      void backend
                        .call('auth.logout', { provider: provider.id })
                        .then(refresh)
                        .catch(report);
                    }}
                    style={{ padding: 8, borderRadius: 7, borderColor: colors.border_strong }}
                  >
                    Disconnect
                  </Button>
                )}
              </div>
              {provider.id === 'anthropic-subscription' && (
                <>
                  <div style={{ display: 'flex', flexDirection: 'row', gap: 8 }}>
                    <Button
                      onClick={() => void start(['claude-account', 'list'])}
                      style={{ padding: 8 }}
                    >
                      List accounts
                    </Button>
                    <Button
                      onClick={() => void start(['claude-account', 'add'])}
                      style={{ padding: 8 }}
                    >
                      Add account
                    </Button>
                  </div>
                  <TextField placeholder="Account ID" value={accountId} onChange={setAccountId} />
                  <TextField
                    placeholder="Display name"
                    value={accountName}
                    onChange={setAccountName}
                  />
                  <div style={{ display: 'flex', flexDirection: 'row', gap: 8 }}>
                    {['pin', 'rename', 'remove'].map((action) => (
                      <Button
                        key={action}
                        disabled={!accountId.trim()}
                        onClick={() =>
                          void start([
                            'claude-account',
                            action,
                            accountId.trim(),
                            ...(action === 'rename' ? accountName.trim().split(/\s+/) : []),
                          ])
                        }
                        style={{ padding: 8 }}
                      >
                        {action === 'pin' ? 'Pin' : action === 'rename' ? 'Rename' : 'Remove'}
                      </Button>
                    ))}
                    <Button
                      onClick={() => void start(['claude-account', 'unpin'])}
                      style={{ padding: 8 }}
                    >
                      Unpin
                    </Button>
                  </div>
                </>
              )}
              {apiKeyProvider === provider.id && (
                <>
                  <TextField placeholder="API key" secure value={input} onChange={setInput} />
                  <Button
                    disabled={!input}
                    onClick={() => {
                      const secret = input;
                      setInput('');
                      setApiKeyProvider(undefined);
                      void start(
                        ['login', provider.id, '--method', 'api-key', '--api-key-stdin'],
                        secret,
                      );
                    }}
                    style={{ padding: 8 }}
                  >
                    Save API key
                  </Button>
                </>
              )}
              {message && (
                <div style={{ color: colors.text_secondary, lineHeight: sp(18) }}>{message}</div>
              )}
              {url && (
                <div style={{ display: 'flex', flexDirection: 'row', gap: 8 }}>
                  <Button onClick={() => void openUrl(url).catch(report)} style={{ padding: 8 }}>
                    Open sign-in page
                  </Button>
                  <Button
                    onClick={() => void writeClipboard(url).catch(report)}
                    style={{ padding: 8 }}
                  >
                    Copy URL
                  </Button>
                </div>
              )}
              {attempt !== undefined && (
                <>
                  <TextField
                    placeholder="Device code or authorization response…"
                    value={input}
                    onChange={setInput}
                    onKeyDown={(event) => {
                      if (event.key === 'enter') {
                        void backend
                          .call('auth.input', { attempt, line: input })
                          .then(() => setInput(''))
                          .catch(report);
                      }
                    }}
                  />
                  <div style={{ display: 'flex', flexDirection: 'row', gap: 8 }}>
                    <Button
                      onClick={() => {
                        void backend
                          .call('auth.input', { attempt, line: input })
                          .then(() => setInput(''))
                          .catch(report);
                      }}
                      style={{ padding: 8 }}
                    >
                      Continue
                    </Button>
                    <Button
                      onClick={() => {
                        void backend.call('auth.input', { attempt, line: null }).catch(report);
                        activeAttempt.current = undefined;
                        setAttempt(undefined);
                      }}
                      style={{ padding: 8 }}
                    >
                      Cancel
                    </Button>
                  </div>
                </>
              )}
            </div>
          )}
        </div>
      ))}
    </SettingsCard>
  );
}

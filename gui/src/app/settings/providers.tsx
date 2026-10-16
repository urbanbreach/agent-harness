import { useEffect, useRef, useState } from 'react';
import type { Notification, Provider } from '../../backend/protocol';
import { getAppStore } from '../../store';
import { Button, Icon, TextField } from '../../ui';
import { useTheme } from '../../theme';
import { providerColor } from '../../theme/colors';
import { openUrl, writeClipboard } from '../../workspace/os';
import { SettingsCard, SettingsRow } from './index';
import { t } from '../../i18n';

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
      setMessage(t('settings.starting_sign_in'));
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
      <SettingsRow
        title={t('settings.providers')}
        description={t('settings.providers_description')}
        last
        style={{ paddingTop: 14, paddingBottom: 4 }}
      >
        <Button
          onClick={() => void refresh()}
          style={{
            height: 28,
            minWidth: 87,
            flexShrink: 0,
            alignSelf: 'flex-start',
            paddingLeft: 11,
            paddingRight: 11,
            borderRadius: 7,
            fontSize: sp(12.5),
            borderColor: colors.border_strong,
            gap: 6,
          }}
        >
          <Icon name="rotate-cw" size={11} />
          {checking ? t('settings.checking') : t('settings.refresh')}
        </Button>
      </SettingsRow>
      {error && (
        <div style={{ padding: 20, color: colors.danger, fontSize: sp(12.5) }}>{error}</div>
      )}
      {providers.map((provider) => (
        <div key={provider.id}>
          <SettingsRow
            title={provider.label}
            description={`${provider.connected ? t('settings.connected') : t('settings.not_connected')} · ${provider.description}`}
            style={{ paddingTop: 10, paddingBottom: 10 }}
            leading={
              <div
                style={{
                  position: 'relative',
                  display: 'flex',
                  width: 30,
                  height: 30,
                  flexShrink: 0,
                  alignItems: 'center',
                  justifyContent: 'center',
                  borderRadius: 6,
                  backgroundColor: colors.overlay,
                }}
              >
                <Icon
                  name={
                    provider.id === 'openai'
                      ? 'provider-openai'
                      : provider.id.startsWith('anthropic')
                        ? 'provider-claude'
                        : 'bot'
                  }
                  size={20}
                  color={providerColor(provider.id, colors)}
                />
                <div
                  style={{
                    position: 'absolute',
                    right: -2,
                    bottom: -2,
                    width: 10,
                    height: 10,
                    borderRadius: 5,
                    borderWidth: 2,
                    borderColor: colors.raised,
                    backgroundColor: provider.connected ? colors.success : colors.text_ghost,
                  }}
                />
              </div>
            }
          >
            <Button
              aria-label={t('settings.configure_provider', { provider: provider.label })}
              style={{ width: 22, height: 22 }}
              onClick={() => {
                setExpanded(expanded === provider.id ? undefined : provider.id);
                setMessage('');
                setInput('');
                setApiKeyProvider(undefined);
              }}
            >
              <Icon name={expanded === provider.id ? 'chevron-down' : 'chevron-right'} size={14} />
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
                    {t('settings.disconnect')}
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
                      {t('settings.list_accounts')}
                    </Button>
                    <Button
                      onClick={() => void start(['claude-account', 'add'])}
                      style={{ padding: 8 }}
                    >
                      {t('settings.add_account')}
                    </Button>
                  </div>
                  <TextField
                    placeholder={t('settings.account_id')}
                    value={accountId}
                    onChange={setAccountId}
                  />
                  <TextField
                    placeholder={t('settings.display_name')}
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
                        {action === 'pin'
                          ? t('settings.pin')
                          : action === 'rename'
                            ? t('settings.rename')
                            : t('settings.remove')}
                      </Button>
                    ))}
                    <Button
                      onClick={() => void start(['claude-account', 'unpin'])}
                      style={{ padding: 8 }}
                    >
                      {t('settings.unpin')}
                    </Button>
                  </div>
                </>
              )}
              {apiKeyProvider === provider.id && (
                <>
                  <TextField
                    placeholder={t('settings.api_key')}
                    secure
                    value={input}
                    onChange={setInput}
                  />
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
                    {t('settings.save_api_key')}
                  </Button>
                </>
              )}
              {message && (
                <div style={{ color: colors.text_secondary, lineHeight: sp(18) }}>{message}</div>
              )}
              {url && (
                <div style={{ display: 'flex', flexDirection: 'row', gap: 8 }}>
                  <Button onClick={() => void openUrl(url).catch(report)} style={{ padding: 8 }}>
                    {t('settings.open_sign_in')}
                  </Button>
                  <Button
                    onClick={() => void writeClipboard(url).catch(report)}
                    style={{ padding: 8 }}
                  >
                    {t('settings.copy_url')}
                  </Button>
                </div>
              )}
              {attempt !== undefined && (
                <>
                  <TextField
                    placeholder={t('settings.auth_response')}
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
                      {t('settings.continue')}
                    </Button>
                    <Button
                      onClick={() => {
                        void backend.call('auth.input', { attempt, line: null }).catch(report);
                        activeAttempt.current = undefined;
                        setAttempt(undefined);
                      }}
                      style={{ padding: 8 }}
                    >
                      {t('settings.cancel')}
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

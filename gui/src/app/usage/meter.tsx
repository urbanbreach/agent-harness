import { useEffect, useRef, useState } from 'react';
import { DismissableLayer, useGpuix, type PublicInstance } from '@gpuix/react';
import type { Methods } from '../../backend/protocol';
import { getAppStore, useSessionView, useStore } from '../../store';
import { useTheme } from '../../theme';
import { Button, Icon, Tooltip } from '../../ui';
import { row, ellipsis } from '../../ui/controls';
import { openUrl } from '../../workspace/os';
import { registerAction, runAction } from '../actions';
import { Pulse } from './pulse';

type Plan = Methods['usage.subscription']['result'];
const compact = (n: number) =>
  Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 }).format(n);
const bounded = (n: number) => Math.max(0, Math.min(100, n));

export function UsageMeter({ sessionId, cwd }: { sessionId: string | null; cwd: string | null }) {
  const { colors, sp } = useTheme();
  const { renderer } = useGpuix();
  const view = useSessionView(sessionId ?? undefined);
  const model = useStore((s) => (cwd ? s.models[cwd]?.current : undefined));
  const provider = model?.provider ?? view?.meta?.provider_model?.split('/')[0] ?? '';
  const [open, setOpen] = useState(false);
  const [plan, setPlan] = useState<Plan>();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string>();
  const trigger = useRef<PublicInstance>(null);
  const card = useRef<PublicInstance>(null);
  const lastAttempt = useRef(0);
  const inflight = useRef(false);
  const generation = useRef(0);
  useEffect(() => {
    generation.current++;
    setPlan(undefined);
    setError(undefined);
    setPending(false);
    setOpen(false);
    inflight.current = false;
    lastAttempt.current = 0;
  }, [sessionId, provider, cwd]);
  useEffect(() => {
    if (!sessionId) return;
    return registerAction({
      id: 'usage.toggle',
      label: 'action.usagePanel',
      shortcut: 'Ctrl+U',
      run: () => setOpen((v) => !v),
    });
  }, [sessionId]);
  useEffect(() => {
    if (!open || !provider || inflight.current || Date.now() - lastAttempt.current < 30_000) return;
    const current = generation.current;
    lastAttempt.current = Date.now();
    inflight.current = true;
    setPending(true);
    void getAppStore()
      .backend.call('usage.subscription', { provider, cwd: cwd ?? undefined })
      .then((result) => {
        if (current === generation.current) {
          if (result.status === 'error') {
            setError(result.message ?? 'Usage request failed');
          } else {
            setPlan(result);
            setError(undefined);
          }
        }
      })
      .catch((reason) => {
        if (current === generation.current) setError(String(reason));
      })
      .finally(() => {
        if (current === generation.current) {
          inflight.current = false;
          setPending(false);
        }
      });
  }, [open, provider, cwd]);
  if (!sessionId) return null;
  const used = Math.max(0, view?.context?.occupiedTokens ?? 0);
  const capacity = view?.context?.windowTokens;
  const budget = view?.context?.budget;
  const threshold =
    budget &&
    typeof budget === 'object' &&
    'compaction_threshold_tokens' in budget &&
    typeof budget.compaction_threshold_tokens === 'number'
      ? budget.compaction_threshold_tokens
      : undefined;
  const percent = capacity && capacity > 0 ? bounded((used / capacity) * 100) : undefined;
  const color = (value: number) =>
    value >= 95 ? colors.danger : value >= 80 ? colors.warning : colors.gauge;
  const close = () => {
    setOpen(false);
    runAction('composer.focus');
  };
  const tooltip = error
    ? `Usage refresh failed: ${error}`
    : percent === undefined
      ? 'Usage (Ctrl+U)'
      : `Context window ${Math.round(percent)}% used (Ctrl+U)`;
  const circumference = Math.PI * 10.5;
  const ring = `<svg xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 13 13"><circle cx="6.5" cy="6.5" r="5.25" fill="none" stroke="${colors.border_strong}" stroke-width="2.5"/>${percent === undefined ? '' : `<circle cx="6.5" cy="6.5" r="5.25" fill="none" stroke="${color(percent)}" stroke-width="2.5" stroke-dasharray="${(circumference * Math.max(5, percent)) / 100} ${circumference}" transform="rotate(-90 6.5 6.5)"/>`}</svg>`;
  const accountUrl = /claude|anthropic/i.test(provider)
    ? 'https://claude.ai/settings/usage'
    : /codex|openai/i.test(provider)
      ? 'https://chatgpt.com/codex/settings/usage'
      : undefined;
  const windows = plan?.windows.filter((w) => !/spark/i.test(w.label)) ?? [];
  function bar(value: number) {
    return (
      <div
        style={{
          height: 3,
          backgroundColor: colors.overlay_strong,
          borderRadius: 3,
          overflow: 'hidden',
        }}
      >
        <div
          style={{
            height: 3,
            width: `${value > 0 ? Math.max(1.5, bounded(value)) : 0}%`,
            backgroundColor: color(value),
            borderRadius: 3,
          }}
        />
      </div>
    );
  }
  return (
    <div style={{ position: 'relative', ...row }}>
      <Tooltip content={tooltip}>
        <Button
          ref={trigger}
          aria-label={tooltip}
          aria-expanded={open}
          onClick={() => setOpen((v) => !v)}
          style={{
            height: 20,
            paddingLeft: 5,
            paddingRight: 5,
            borderRadius: 5,
            backgroundColor: open ? colors.overlay_strong : undefined,
          }}
        >
          <svg source={ring} style={{ width: 13, height: 13 }} />
        </Button>
      </Tooltip>
      {open && (
        <DismissableLayer
          initialFocus={() => card.current?.id ?? null}
          finalFocus={(previous) => {
            runAction('composer.focus');
            return renderer?.getFocusedElementId?.() ?? previous;
          }}
          onEscapeKeyDown={close}
        >
          <anchored
            side="top"
            align="end"
            gap={8}
            fit="switch"
            snapMargin={8}
            deferred
            priority={3}
            occlude
          >
            <div
              ref={card}
              tabIndex={-1}
              role="dialog"
              aria-label="Usage"
              onMouseDownOutside={(e) => {
                const b = trigger.current && renderer?.getElementBounds?.(trigger.current.id);
                if (
                  b &&
                  e.x !== undefined &&
                  e.y !== undefined &&
                  e.x >= b.x &&
                  e.x <= b.x + b.width &&
                  e.y >= b.y &&
                  e.y <= b.y + b.height
                )
                  return;
                close();
              }}
              style={{
                width: 320,
                padding: 14,
                display: 'flex',
                flexDirection: 'column',
                gap: 12,
                borderRadius: 10,
                borderWidth: 1,
                borderColor: colors.border_strong,
                backgroundColor: colors.raised,
                color: colors.text,
                fontSize: sp(12.5),
                boxShadow: {
                  offsetX: 0,
                  offsetY: 12,
                  blurRadius: 24,
                  spreadRadius: -4,
                  color: '#00000033',
                },
              }}
            >
              <div style={{ display: 'flex', flexDirection: 'column', gap: 7 }}>
                <div style={{ ...row, gap: 8 }}>
                  <text style={{ flexGrow: 1 }}>Context window</text>
                  <text style={{ color: colors.text_tertiary }}>
                    {capacity && capacity > 0
                      ? `${compact(Math.min(used, capacity))} / ${compact(capacity)} (${Math.round(percent!)}%)`
                      : compact(used)}
                  </text>
                </div>
                {bar(percent ?? 0)}
                {threshold != null && threshold > 0 && (
                  <text style={{ color: colors.text_tertiary }}>
                    {`Auto-compaction threshold · ${compact(threshold)} tokens`}
                  </text>
                )}
              </div>
              {(pending || error || plan?.status === 'available') && (
                <>
                  <div style={{ height: 1, backgroundColor: colors.border }} />
                  <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
                    {pending && plan?.status !== 'available' ? (
                      <Pulse style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
                        {[132, 210, 190].map((width, i) => (
                          <div
                            key={i}
                            style={{
                              width,
                              height: 9,
                              backgroundColor: colors.overlay_strong,
                              borderRadius: 5,
                            }}
                          />
                        ))}
                        {bar(0)}
                      </Pulse>
                    ) : (
                      <>
                        <div style={{ ...row, gap: 6 }}>
                          <text style={{ ...ellipsis, flexGrow: 1, color: colors.text_tertiary }}>
                            {`Plan usage limits${plan?.plan ? ` · ${plan.plan}` : ''}`}
                          </text>
                          {accountUrl && (
                            <Tooltip content="Open account usage settings">
                              <Button
                                aria-label="Open account usage settings"
                                onClick={() =>
                                  void openUrl(accountUrl).catch((reason) =>
                                    setError(String(reason)),
                                  )
                                }
                                style={{ width: 20, height: 20 }}
                              >
                                <Icon name="arrow-right" size={10} />
                              </Button>
                            </Tooltip>
                          )}
                        </div>
                        {plan?.status !== 'available' && error ? (
                          <text style={{ color: colors.text_secondary, whiteSpace: 'normal' }}>
                            {`Unavailable — ${error}`}
                          </text>
                        ) : (
                          windows.map((window, i) => (
                            <div
                              key={`${window.label}-${i}`}
                              style={{ display: 'flex', flexDirection: 'column', gap: 7 }}
                            >
                              <div style={{ ...row, gap: 8 }}>
                                <text style={{ ...ellipsis, flexGrow: 1 }}>{window.label}</text>
                                {window.resetsAt && (
                                  <Tooltip content={new Date(window.resetsAt).toLocaleString()}>
                                    <text style={{ color: colors.text_tertiary }}>
                                      {resetCaption(window.resetsAt)}
                                    </text>
                                  </Tooltip>
                                )}
                                <text style={{ color: colors.text_secondary }}>
                                  {`${Math.round(bounded(window.usedPercent))}%`}
                                </text>
                              </div>
                              {bar(window.usedPercent)}
                            </div>
                          ))
                        )}
                      </>
                    )}
                  </div>
                </>
              )}
            </div>
          </anchored>
        </DismissableLayer>
      )}
    </div>
  );
}
function resetCaption(value: string) {
  const remaining = Math.max(0, new Date(value).getTime() - Date.now());
  if (remaining < 60_000) return 'Resets now';
  if (remaining < 3_600_000) return `Resets in ${Math.ceil(remaining / 60_000)}m`;
  if (remaining < 86_400_000) return `Resets in ${Math.ceil(remaining / 3_600_000)}h`;
  return `Resets ${new Date(value).toLocaleDateString('en', { month: 'short', day: 'numeric' })}`;
}

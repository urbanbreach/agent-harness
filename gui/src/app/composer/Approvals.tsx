import { useEffect, useRef, useState } from 'react';
import { useGpuix, type KeyEvent, type PublicInstance } from '@gpuix/react';
import type { Intent } from '../../backend/protocol';
import type { PermissionView, QuestionRequest } from '../../model/session';
import { getAppStore, useSessionView } from '../../store';
import { useTheme } from '../../theme';
import { Button, Icon, TextField, useToast } from '../../ui';
import { bare, row } from '../../ui/controls';
import { t, useLocale } from '../../i18n';

type Resolution = Extract<Intent, { kind: 'resolvePermission' }>;
type Question = {
  question: string;
  header: string;
  options: { label: string; description: string }[];
  multiple: boolean;
  custom: boolean;
};

function record(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object' ? (value as Record<string, unknown>) : {};
}
function parseQuestion(value: unknown): Question {
  const item = record(value);
  return {
    question: typeof item.question === 'string' ? item.question : '',
    header: typeof item.header === 'string' ? item.header : '',
    options: Array.isArray(item.options)
      ? item.options.flatMap((value) => {
          const option = record(value);
          return typeof option.label === 'string'
            ? [
                {
                  label: option.label,
                  description: typeof option.description === 'string' ? option.description : '',
                },
              ]
            : [];
        })
      : [],
    multiple: (item.multiSelect ?? item.multi_select ?? item.multiple) === true,
    custom: item.custom !== false,
  };
}

export function Approvals({ sessionId }: { sessionId: string }) {
  const view = useSessionView(sessionId);
  const question = view?.questionRequests[0];
  const permission = view?.permissions.find((item) => !item.decision);
  if (!question && !permission) return null;
  return (
    <ApprovalCard
      key={`${sessionId}:${question?.permissionId ?? permission!.permission_id}`}
      sessionId={sessionId}
      request={question}
      permission={permission}
      toolName={permission?.tool_call_id ? view?.tools[permission.tool_call_id]?.toolId : undefined}
    />
  );
}

function ApprovalCard({
  sessionId,
  request,
  permission,
  toolName,
}: {
  sessionId: string;
  request?: QuestionRequest;
  permission?: PermissionView;
  toolName?: string;
}) {
  const { colors, sp } = useTheme();
  useLocale();
  const { renderer } = useGpuix();
  const { showToast } = useToast();
  const questions = request?.questions.map(parseQuestion) ?? [];
  const [page, setPage] = useState(0);
  const [selections, setSelections] = useState<string[][]>(() => questions.map(() => []));
  const [custom, setCustom] = useState<string[]>(() => questions.map(() => ''));
  const [pending, setPending] = useState(false);
  const locked = useRef(false);
  const controls = useRef<(PublicInstance | null)[]>([]);
  const customInput = useRef<PublicInstance>(null);
  const continueButton = useRef<PublicInstance>(null);
  const activeControl = useRef(0);
  const editing = useRef(false);
  const question = questions[page];
  const answers = questions.map((item, index) => {
    const text = custom[index]?.trim();
    const chosen = selections[index] ?? [];
    return text && item.custom
      ? item.multiple
        ? [...new Set([...chosen, text])]
        : [text]
      : chosen;
  });
  const valid = answers.every(
    (values) =>
      values.length > 0 &&
      values.length <= 32 &&
      values.every((value) => new TextEncoder().encode(value).length <= 4096),
  );
  const currentValid =
    !!answers[page]?.length &&
    answers[page].length <= 32 &&
    answers[page].every((value) => new TextEncoder().encode(value).length <= 4096);

  useEffect(() => {
    const previous = renderer?.getFocusedElementId?.();
    const target = controls.current[0] ?? customInput.current;
    if (target) renderer?.focusElement?.(target.id);
    return () => {
      if (previous !== undefined && previous !== null) renderer?.focusElement?.(previous);
    };
  }, [renderer]);
  useEffect(() => {
    activeControl.current = 0;
    editing.current = false;
    const target = controls.current[0] ?? customInput.current;
    if (target) renderer?.focusElement?.(target.id);
  }, [page, renderer]);

  async function resolve(response: Omit<Resolution, 'kind' | 'permissionId'>) {
    if (locked.current) return;
    locked.current = true;
    setPending(true);
    try {
      await getAppStore().sendIntent(sessionId, {
        kind: 'resolvePermission',
        permissionId: request?.permissionId ?? permission!.permission_id,
        ...response,
      });
      // Keep the card disabled until the authoritative resolution removes it.
    } catch (error) {
      locked.current = false;
      setPending(false);
      showToast(
        t('composer.response_error', {
          error: error instanceof Error ? error.message : String(error),
        }),
      );
    }
  }
  function select(index: number) {
    if (pending || !question?.options[index]) return;
    const label = question.options[index].label;
    setSelections((values) =>
      values.map((value, i) =>
        i !== page
          ? value
          : question.multiple
            ? value.includes(label)
              ? value.filter((item) => item !== label)
              : [...value, label]
            : [label],
      ),
    );
    if (!question.multiple)
      setCustom((values) => values.map((value, i) => (i === page ? '' : value)));
  }
  function next() {
    if (pending || !currentValid) return;
    if (page < questions.length - 1) setPage(page + 1);
    else if (valid) void resolve({ decision: 'allow', reason: JSON.stringify(answers) });
    else setPage(answers.findIndex((values) => !values.length));
  }
  function keys(event: KeyEvent) {
    const key = event.key ?? '';
    if ((event.modifiers?.ctrl || event.modifiers?.cmd) && key === 'enter') {
      event.preventDefault();
      event.stopPropagation();
      next();
      return;
    }
    if (!bare(event)) return;
    // Escape must never fall through to the running-turn stop shortcut.
    if (key === 'escape') {
      event.preventDefault();
      event.stopPropagation();
      if (!pending && question) {
        setSelections((values) => values.map((value, i) => (i === page ? [] : value)));
        setCustom((values) => values.map((value, i) => (i === page ? '' : value)));
      }
      return;
    }
    if (editing.current || pending) return;
    const count = question ? question.options.length : 3;
    if (['up', 'down', 'home', 'end'].includes(key) && count) {
      event.preventDefault();
      event.stopPropagation();
      const index =
        key === 'home'
          ? 0
          : key === 'end'
            ? count - 1
            : (activeControl.current + (key === 'up' ? -1 : 1) + count) % count;
      activeControl.current = index;
      const target = controls.current[index];
      if (target) renderer?.focusElement?.(target.id);
    }
  }
  const buttonStyle = {
    height: 28,
    paddingLeft: 13,
    paddingRight: 13,
    borderRadius: 7,
    fontSize: sp(12.5),
    fontWeight: 600,
    active: { opacity: 0.8 },
  } as const;
  return (
    <div
      style={{
        paddingLeft: 20,
        paddingRight: 20,
        paddingBottom: 8,
        flexShrink: 0,
        alignItems: 'center',
      }}
    >
      <div
        role="group"
        aria-label={t(request ? 'composer.answer_questions' : 'composer.permission_request')}
        aria-busy={pending}
        onKeyDown={keys}
        style={{
          width: '100%',
          maxWidth: 720,
          minWidth: 0,
          flexDirection: 'column',
          padding: request ? 14 : 12,
          paddingTop: 12,
          paddingBottom: request ? 10 : 12,
          borderRadius: request ? 13 : 12,
          borderWidth: 1,
          borderColor: request ? colors.border : colors.border_strong,
          backgroundColor: request ? colors.composer : colors.raised,
        }}
      >
        {question ? (
          <>
            <div style={{ ...row, gap: 8 }}>
              <text style={{ fontSize: sp(12.5), fontWeight: 600, color: colors.text_tertiary }}>
                {question.header || t('composer.question')}
              </text>
              {questions.length > 1 && (
                <text
                  style={{
                    height: 18,
                    paddingLeft: 6,
                    paddingRight: 6,
                    borderRadius: 5,
                    backgroundColor: colors.overlay,
                    color: colors.text_tertiary,
                    fontSize: sp(12.5),
                  }}
                >
                  {t('user_input.progress', { current: page + 1, total: questions.length })}
                </text>
              )}
            </div>
            <text
              style={{
                marginTop: 5,
                fontSize: sp(13),
                lineHeight: sp(18),
                fontWeight: 500,
                color: colors.text,
                whiteSpace: 'normal',
              }}
            >
              {question.question}
            </text>
            <div
              style={{
                marginTop: 9,
                gap: 4,
                maxHeight: 260,
                overflowY: 'scroll',
                flexDirection: 'column',
              }}
            >
              {question.options.map((option, index) => {
                const selected = selections[page]?.includes(option.label) ?? false;
                return (
                  <Button
                    key={`${page}:${index}`}
                    ref={(node) => {
                      controls.current[index] = node;
                    }}
                    role={question.multiple ? 'checkbox' : 'radio'}
                    aria-checked={selected}
                    disabled={pending}
                    onFocus={() => {
                      activeControl.current = index;
                    }}
                    onClick={() => select(index)}
                    style={{
                      minHeight: 36,
                      flexShrink: 0,
                      paddingLeft: 10,
                      paddingRight: 10,
                      paddingTop: 5,
                      paddingBottom: 5,
                      borderRadius: 8,
                      gap: 8,
                      justifyContent: 'flex-start',
                      backgroundColor: selected ? `${colors.accent}14` : colors.overlay,
                      borderColor: selected ? `${colors.accent}57` : 'transparent',
                      hover: {
                        backgroundColor: colors.overlay_strong,
                        borderColor: colors.border_strong,
                      },
                      active: { opacity: 0.85 },
                    }}
                  >
                    <div
                      style={{
                        flexGrow: 1,
                        flexBasis: 0,
                        minWidth: 0,
                        flexDirection: 'column',
                        gap: 1,
                      }}
                    >
                      <text
                        style={{
                          fontSize: sp(12.5),
                          fontWeight: 500,
                          color: colors.text,
                          whiteSpace: 'normal',
                        }}
                      >
                        {option.label}
                      </text>
                      {!!option.description && (
                        <text
                          style={{
                            fontSize: sp(12.5),
                            lineHeight: sp(15),
                            color: colors.text_secondary,
                            whiteSpace: 'normal',
                          }}
                        >
                          {option.description}
                        </text>
                      )}
                    </div>
                    {selected && <Icon name="check" size={12} color={colors.accent} />}
                  </Button>
                );
              })}
            </div>
            <TextField
              key={page}
              ref={customInput}
              value={custom[page] ?? ''}
              icon="pencil"
              iconSize={11}
              aria-label={t(
                question.custom ? 'user_input.other_placeholder' : 'composer.custom_not_allowed',
              )}
              placeholder={t('user_input.other_placeholder')}
              readOnly={pending || !question.custom}
              tabIndex={pending || !question.custom ? -1 : 0}
              onFocus={() => {
                editing.current = true;
              }}
              onBlur={() => {
                editing.current = false;
              }}
              onChange={(value) => {
                if (pending || !question.custom) return;
                setCustom((values) => values.map((entry, i) => (i === page ? value : entry)));
                if (!question.multiple && value.trim())
                  setSelections((values) => values.map((entry, i) => (i === page ? [] : entry)));
              }}
              onKeyDown={(event) => {
                if (bare(event) && event.key === 'enter') {
                  event.preventDefault();
                  event.stopPropagation();
                  next();
                }
              }}
              style={{
                height: 34,
                marginTop: question.options.length ? 4 : 9,
                paddingLeft: 10,
                paddingRight: 10,
                borderRadius: 8,
                gap: 7,
                backgroundColor: custom[page]?.trim() ? `${colors.accent}0F` : colors.overlay,
                borderColor: custom[page]?.trim() ? `${colors.accent}57` : 'transparent',
              }}
            />
            <div style={{ ...row, marginTop: 8 }}>
              {page > 0 && (
                <Button
                  disabled={pending}
                  onClick={() => setPage(page - 1)}
                  style={{ ...buttonStyle, height: 26, paddingLeft: 8, paddingRight: 8 }}
                >
                  {t('user_input.back')}
                </Button>
              )}
              <div style={{ flexGrow: 1 }} />
              <Button
                ref={continueButton}
                variant="primary"
                disabled={pending || !currentValid}
                onClick={next}
                style={{ ...buttonStyle, height: 26, paddingLeft: 10, paddingRight: 10 }}
              >
                {t(
                  pending
                    ? 'composer.sending'
                    : page === questions.length - 1
                      ? 'user_input.submit'
                      : 'user_input.next',
                )}
              </Button>
            </div>
          </>
        ) : (
          <>
            <div style={{ ...row, gap: 8 }}>
              <Icon name="alert" size={13} color={colors.warning} />
              <text
                style={{
                  fontSize: sp(12.5),
                  fontWeight: 500,
                  color: colors.text,
                  whiteSpace: 'normal',
                }}
              >
                {toolName || t('composer.permission_requested')} · {permission?.kind}
              </text>
            </div>
            <div
              style={{
                marginTop: 8,
                maxHeight: 92,
                overflowY: 'scroll',
                padding: 8,
                borderRadius: 7,
                backgroundColor: colors.inset,
              }}
            >
              <text
                style={{
                  fontFamily: 'JetBrains Mono',
                  fontSize: sp(12.5),
                  lineHeight: sp(16),
                  color: colors.text_secondary,
                  whiteSpace: 'normal',
                }}
              >
                {permission?.summary}
              </text>
            </div>
            <div style={{ ...row, gap: 8, marginTop: 10, flexWrap: 'wrap' }}>
              {(
                [
                  t('permission.allow_once'),
                  t('permission.allow_for_session'),
                  t('composer.deny'),
                ] as const
              ).map((label, index) => (
                <Button
                  key={label}
                  ref={(node) => {
                    controls.current[index] = node;
                  }}
                  disabled={pending}
                  variant={index < 2 ? 'primary' : 'secondary'}
                  onFocus={() => {
                    activeControl.current = index;
                  }}
                  onClick={() =>
                    void resolve(
                      index === 2
                        ? { decision: 'deny' }
                        : { decision: 'allow', grantScope: index === 0 ? 'once' : 'session' },
                    )
                  }
                  style={{
                    ...buttonStyle,
                    borderColor: index === 2 ? colors.border_strong : 'transparent',
                  }}
                >
                  {label}
                </Button>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

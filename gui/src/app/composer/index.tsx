import { useEffect, useRef, useState } from 'react';
import { flushSync, useGpuix, type KeyEvent, type PublicInstance } from '@gpuix/react';
import {
  getAppStore,
  useCommands,
  useDraft,
  useFiles,
  useModels,
  useSessionView,
  useStore,
} from '../../store';
import type { Draft, DraftAttachment, Selection } from '../../store/store';
import type { AccessMode, ModelSelection, Skill } from '../../backend/protocol';
import { useNativeRenderer, type NativeEvent, type InputCommand } from '../../native';
import { useTheme } from '../../theme';
import { Button, Dropdown, Icon, Menu, Spinner, Tooltip, useToast } from '../../ui';
import { registerAction, runAction } from '../actions';
import { ProjectSelector, BranchSelector } from '../pickers';
import { UsageMeter } from '../usage';
import { AttachmentTiles } from './AttachmentTiles';
import { Approvals } from './Approvals';
import { ModelControls } from './ModelControls';
import { Queue, type FollowUp } from './Queue';
import { Suggestions } from './Suggestions';
import {
  appCommands,
  terminalCommands,
  suggestions,
  triggerAt,
  type Suggestion,
} from './completion';
import { importPath, maxAttachmentBytes } from './attachments';
import { canSteerDraft, prepareDraft, setQueuedDrafts, startQueueDelivery } from './queue-store';
import { recordPromptDisplay } from './prompt-display';

export type ComposerTarget =
  | { kind: 'task'; sessionId: string; cwd: string }
  | { kind: 'new'; projectId: string | null; cwd: string | null };
function selectionFor(target: ComposerTarget): Selection {
  const state = getAppStore().getSnapshot();
  const row =
    target.kind === 'task'
      ? Object.values(state.sessions)
          .flat()
          .find((entry) => entry.runId === target.sessionId)
      : undefined;
  const projectId =
    target.kind === 'new'
      ? target.projectId
      : state.projects.find((project) => project.cwd === (row?.projectCwd ?? target.cwd))?.id;
  return target.kind === 'task'
    ? { kind: 'session', sessionId: target.sessionId, projectId: projectId ?? '' }
    : projectId
      ? { kind: 'project', projectId }
      : { kind: 'provisional' };
}
const emptyDraft: Draft = { text: '' };
export function Composer({ target }: { target: ComposerTarget }) {
  const store = getAppStore();
  const selection = selectionFor(target);
  const key = JSON.stringify(selection);
  const draft = useDraft(selection) ?? emptyDraft;
  const cwd = target.cwd ?? '';
  const sessionId = target.kind === 'task' ? target.sessionId : undefined;
  const view = useSessionView(sessionId);
  const live = useStore((state) => (sessionId ? state.live[sessionId] : undefined));
  const [runtimeAccess, setRuntimeAccess] = useState<{ sessionId: string; mode: AccessMode }>();
  const catalog = useModels(cwd);
  const commands = useCommands(cwd);
  const files = useFiles(cwd);
  const preferences = useStore((state) => state.preferences);
  const stopArmed = useStore((state) => state.layout.stopArmed === true);
  const { colors, sp } = useTheme();
  const { renderer } = useGpuix();
  const native = useNativeRenderer();
  const { showToast } = useToast();
  const editor = useRef<PublicInstance>(null);
  const owner = useRef(key);
  owner.current = key;
  const [caret, setCaret] = useState(0);
  const selectionRange = useRef({ start: 0, end: 0 });
  const [focused, setFocused] = useState(false);
  const [preparing, setPreparing] = useState(false);
  const [draggingFiles, setDraggingFiles] = useState(false);
  const [width, setWidth] = useState(720);
  const [highlight, setHighlight] = useState(0);
  const [dismissed, setDismissed] = useState<string>();
  const [editMenu, setEditMenu] = useState<{ x: number; y: number }>();
  const [skillCatalog, setSkillCatalog] = useState<{ cwd: string; entries: Skill[] }>();
  const [mentions, setMentions] = useState<{
    cwd: string;
    agents: { name: string }[];
    resources: { name: string; uri: string; mime: string }[];
  }>();
  const [promptHistory, setPromptHistory] = useState<{ cwd: string; prompts: string[] }>();
  const [ghost, setGhost] = useState<{ text: string; suffix: string }>();
  const busy =
    view?.status === 'working' || view?.status === 'waiting' || view?.status === 'background';
  const overlay = colors.overlay_strong.match(/[\d.]+/g)!.map(Number);
  const channels = colors.composer
    .slice(1)
    .match(/../g)!
    .map((channel) => parseInt(channel, 16));
  const dropFill = `rgb(${channels.map((channel, index) => Math.round(channel * (1 - overlay[3]!) + overlay[index]! * overlay[3]!)).join(',')})`;
  const composerFill = draggingFiles ? dropFill : colors.composer;
  const attachments = draft.attachments ?? [];
  const trigger = focused ? triggerAt(draft.text, caret) : undefined;
  const tokenKey = trigger ? `${trigger.start}:${trigger.query}:${trigger.kind}` : '';
  const rows = trigger
    ? suggestions(
        trigger,
        commands ?? [],
        files ?? [],
        skillCatalog?.cwd === cwd ? skillCatalog.entries : [],
        mentions?.cwd === cwd ? mentions.agents : [],
        mentions?.cwd === cwd ? mentions.resources : [],
      )
    : [];
  const loading = !!trigger && (trigger.kind === '/' ? !commands : !files);
  const popup = !!trigger && dismissed !== tokenKey && (!!rows.length || loading);
  const queues = (preferences.composerQueues ?? {}) as unknown as Record<string, FollowUp[]>;
  const queue = queues[key] ?? [];
  const sendable = !!draft.text.trim() || attachments.length > 0;
  const providerAvailable = !!catalog?.models.length;
  const canSubmit = providerAvailable || draft.text.trim().startsWith('/');
  const accessMode =
    draft.accessMode ??
    (sessionId
      ? runtimeAccess?.sessionId === sessionId
        ? runtimeAccess.mode
        : view?.yolo
          ? 'full'
          : (catalog?.defaultAccessMode ?? 'supervised')
      : ((preferences.lastAccessMode as AccessMode | undefined) ??
        catalog?.defaultAccessMode ??
        'supervised'));
  function update(patch: Partial<Draft>) {
    store.setDraft(selection, { ...draft, ...patch });
  }
  function focus() {
    if (editor.current) renderer?.focusElement?.(editor.current.id);
  }
  function saveQueue(items: FollowUp[]) {
    setQueuedDrafts(store, key, items);
  }
  function fail(error: unknown) {
    showToast(error instanceof Error ? error.message : String(error));
  }
  useEffect(() => {
    setCaret(draft.text.length);
    setDismissed(undefined);
    if (cwd)
      void Promise.all([
        store.loadModels(cwd),
        store.loadCommands(cwd),
        store.loadFiles(cwd),
      ]).catch(fail);
  }, [key, cwd]);
  useEffect(() => {
    let cancelled = false;
    if (cwd)
      void store.backend
        .call('skills.list', { cwd })
        .then((entries) => {
          if (!cancelled) setSkillCatalog({ cwd, entries: entries.skills });
        })
        .catch(fail);
    return () => {
      cancelled = true;
    };
  }, [cwd]);
  useEffect(() => {
    let cancelled = false;
    if (cwd)
      void Promise.all([
        store.backend.call('agents.list', { cwd }),
        store.backend.call('mcp.resources', { cwd }),
        store.backend.call('history.prompts', { cwd }),
      ])
        .then(([agents, resources, history]) => {
          if (!cancelled) {
            setMentions({ cwd, agents, resources });
            setPromptHistory({ cwd, prompts: history.prompts });
          }
        })
        .catch(fail);
    return () => {
      cancelled = true;
    };
  }, [cwd]);
  useEffect(() => {
    setGhost(undefined);
    if (
      !focused ||
      !draft.text ||
      draft.text.startsWith('!') ||
      popup ||
      caret !== draft.text.length
    )
      return;
    const timer = setTimeout(() => {
      const history = [
        ...(promptHistory?.cwd === cwd ? promptHistory.prompts : []),
        ...(view?.entries.flatMap((entry) => (entry.kind === 'user' ? [entry.text] : [])) ?? []),
      ];
      for (let index = history.length - 1; index >= 0; index--) {
        const prediction = history[index]!;
        if (prediction !== draft.text && prediction.startsWith(draft.text)) {
          setGhost({ text: draft.text, suffix: prediction.slice(draft.text.length) });
          break;
        }
      }
    }, 100);
    return () => clearTimeout(timer);
  }, [draft.text, focused, caret, popup, promptHistory, cwd, view?.entries]);
  useEffect(() => {
    let cancelled = false;
    if (sessionId && live)
      void store.backend
        .call('session.info', { sessionId })
        .then((info) => {
          if (!cancelled) setRuntimeAccess({ sessionId, mode: info.accessMode });
        })
        .catch(fail);
    return () => {
      cancelled = true;
    };
  }, [sessionId, live]);
  useEffect(() => {
    setHighlight(0);
  }, [tokenKey]);
  useEffect(() => {
    if (!editor.current) return;
    const id = editor.current.id;
    setFocused(renderer?.getFocusedElementId?.() === id);
    native.setInputSelectionListener(id, (error, event) => {
      if (!error && event.selectionStart !== undefined && event.selectionEnd !== undefined) {
        selectionRange.current = { start: event.selectionStart, end: event.selectionEnd };
        setCaret(event.selectionEnd);
      }
    });
    return () => native.setInputSelectionListener(id, null);
  }, [key, native]);
  useEffect(
    () => registerAction({ id: 'composer.focus', label: 'composer.message', run: focus }),
    [key],
  );
  useEffect(
    () =>
      registerAction({
        id: 'composer.cancelEdit',
        label: 'action.close',
        enabled: () => popup || !!editMenu,
        run: () => {
          setDismissed(tokenKey);
          setEditMenu(undefined);
          focus();
        },
      }),
    [popup, editMenu, tokenKey],
  );
  useEffect(() => startQueueDelivery(store), [store]);
  const maintenanceArguments = useRef<string | undefined>(undefined);
  useEffect(() => {
    const unregister = (['compact', 'handoff', 'shake'] as const).map((kind) =>
      registerAction({
        id: `session.${kind}`,
        label:
          kind === 'compact'
            ? 'Compact conversation'
            : kind === 'handoff'
              ? 'Prepare handoff'
              : 'Prune conversation context',
        enabled: () => !!sessionId,
        run: () => {
          if (!sessionId) return;
          const argument = maintenanceArguments.current;
          void store
            .sendIntent(
              sessionId,
              kind === 'compact'
                ? { kind, instructions: argument }
                : kind === 'handoff'
                  ? { kind, focus: argument }
                  : { kind, args: argument },
            )
            .catch(fail);
        },
      }),
    );
    return () => unregister.forEach((remove) => remove());
  }, [sessionId]);
  async function localCommand(text: string): Promise<boolean> {
    const match = /^\/(\S+)(?:\s+([\s\S]*))?$/.exec(text);
    if (!match) return false;
    const name = match[1]!;
    const args = match[2]?.trim();
    const command = commands?.find((entry) => entry.name.replace(/^\//, '') === name);
    if (command && (command.scope !== 'builtin' || command.template !== undefined)) return false;
    if (terminalCommands[name]) throw new Error(`/${name} is only available in the terminal`);
    if (!appCommands.includes(name) && command?.scope !== 'builtin') return false;
    if (name === 'rename' && args) {
      if (!sessionId) throw new Error('Open a task before renaming it');
      await store.sendIntent(sessionId, { kind: 'updateTitle', title: args });
      return true;
    }
    if (['model', 'models', 'mo'].includes(name) && args) {
      const entry = catalog?.models.find((entry) =>
        [entry.model, entry.label, `${entry.provider}/${entry.model}`, entry.profile].some(
          (value) => value.toLowerCase() === args.toLowerCase(),
        ),
      );
      if (!entry) throw new Error(`Unknown model: ${args}`);
      const value = { profile: entry.profile, model: entry.model, variant: entry.variant };
      update({ selection: value });
      if (sessionId) await store.sendIntent(sessionId, { kind: 'switchModel', selection: value });
      return true;
    }
    if (name === 'yolo') {
      const mode = accessMode === 'full' ? 'supervised' : 'full';
      update({ accessMode: mode });
      store.setPreferences({ lastAccessMode: mode });
      if (sessionId) {
        await store.sendIntent(sessionId, { kind: 'setAccessMode', mode });
        setRuntimeAccess({ sessionId, mode });
      }
      return true;
    }
    if (name === 'timestamps' || name === 'thinking') {
      const preference = name === 'timestamps' ? 'showMessageTimes' : 'showReasoning';
      store.setPreferences({ [preference]: preferences[preference] === false });
      return true;
    }
    if (name === 'copy') {
      const markdown =
        view?.entries
          .flatMap((entry) =>
            entry.kind === 'user'
              ? [`## You\n\n${entry.text}`]
              : entry.kind === 'text'
                ? [`## Agent\n\n${entry.text}`]
                : [],
          )
          .join('\n\n') ?? '';
      native.writeClipboardText(markdown);
      showToast('Transcript copied', 'success');
      return true;
    }
    if (name === 'claude-account' && args) {
      await store.backend.call('auth.command', {
        cwd,
        args: ['claude-account', ...args.split(/\s+/)],
      });
    }
    if (name === 'new' || name === 'clear') {
      update({ text: args ?? '' });
      if (!runAction('task.new')) throw new Error('New task is unavailable here');
      return true;
    }
    const actions: Record<string, string> = {
      new: 'task.new',
      clear: 'task.new',
      sessions: 'session.resume',
      resume: 'session.resume',
      continue: 'session.resume',
      import: 'session.resume',
      tree: 'session.tree',
      models: 'composer.modelPicker',
      model: 'composer.modelPicker',
      mo: 'composer.modelPicker',
      agents: 'composer.profilePicker',
      mcps: 'settings.tools',
      toggles: 'settings.tools',
      login: 'settings.providers',
      'claude-account': 'settings.providers',
      help: 'settings.help',
      feedback: 'settings.help',
      follow: 'transcript.follow',
      undo: 'session.rewind',
      summarize: 'session.compact',
      exit: 'app.quit',
      quit: 'app.quit',
      q: 'app.quit',
      settings: 'settings.general',
      'view-plan': 'panel.plan',
      worktree: 'worktree.open',
      dashboard: 'panel.session',
      status: 'panel.session',
      usage: 'usage.toggle',
    };
    const commandDraft = store.getDraft(selection);
    if (commandDraft) store.setDraft(selection, { ...commandDraft, text: '' });
    maintenanceArguments.current = args;
    try {
      if (!runAction(actions[name] ?? `session.${name}`)) {
        if (commandDraft) store.setDraft(selection, commandDraft);
        throw new Error(`The ${name} command is unavailable here`);
      }
    } finally {
      maintenanceArguments.current = undefined;
    }
    return true;
  }
  async function submit(steer = false, current = store.getDraft(selection) ?? emptyDraft) {
    if (preparing) return;
    if (!current.text.trim() && !current.attachments?.length) {
      if (steer && busy && queue.length) await steerItem(queue[0]!);
      return;
    }
    if (!providerAvailable && !current.text.trim().startsWith('/')) return;
    setPreparing(true);
    try {
      if (await localCommand(current.text.trim())) {
        if (!/^\/(?:new|clear)(?:\s|$)/.test(current.text.trim()))
          store.setDraft(selection, { ...(store.getDraft(selection) ?? emptyDraft), text: '' });
        return;
      }
      const invocation = /^\/(\S+)(?:\s+([\s\S]*))?$/.exec(current.text.trim());
      if (
        invocation &&
        commands?.some(
          (entry) =>
            entry.name.replace(/^\//, '') === invocation[1] &&
            (entry.scope !== 'builtin' || entry.template !== undefined),
        )
      ) {
        const expanded = await store.backend.call('commands.expand', {
          cwd,
          name: invocation[1]!,
          args: invocation[2] ?? '',
        });
        current = { ...current, text: expanded.text, invocation: current.text.trim() };
      }
      if (!providerAvailable) throw new Error('Choose an available model before sending a prompt');
      const shouldSteer = (steer || view?.status === 'background') && canSteerDraft(current);
      if ((busy && !shouldSteer) || (!steer && queue.length > 0)) {
        saveQueue([...queue, { id: crypto.randomUUID(), draft: current }]);
        store.consumeDraft(selection);
        return;
      }
      if (current.text.trim().startsWith('!')) {
        let id = sessionId;
        if (!id)
          id = (await store.createSession({ cwd, selection: current.selection, accessMode }))
            .sessionId;
        await store.sendIntent(id, {
          kind: 'runShell',
          command: current.text.trim().slice(1).trim(),
        });
        store.consumeDraft(selection);
      } else
        await store.submit(selection, {
          ...prepareDraft(current, cwd),
          accessMode,
          mode: busy ? 'steer' : 'submit',
          runBeforeSubmit: current.invocation
            ? ({ sessionId }) =>
                recordPromptDisplay(sessionId, prepareDraft(current, cwd).text, current.invocation!)
            : undefined,
        });
    } catch (error) {
      fail(error);
    } finally {
      setPreparing(false);
      focus();
    }
  }
  async function steerItem(item: FollowUp) {
    if (!sessionId || !busy || !canSteerDraft(item.draft)) return;
    try {
      await store.submit(selection, {
        ...prepareDraft(item.draft, cwd),
        mode: 'steer',
        consumeDraft: false,
      });
      saveQueue(queue.filter((entry) => entry.id !== item.id));
    } catch (error) {
      fail(error);
    }
  }
  async function addAttachments(imported: DraftAttachment[], importOwner: string) {
    try {
      const staged = await Promise.all(
        imported.map(async (item) =>
          item.dataBase64
            ? {
                ...item,
                ...(await store.backend.call('blobs.put', {
                  mime: item.mime,
                  dataBase64: item.dataBase64,
                })),
              }
            : item,
        ),
      );
      if (owner.current !== importOwner) return;
      const latest = store.getDraft(selection) ?? emptyDraft;
      const existing = latest.attachments ?? [];
      store.setDraft(selection, {
        ...latest,
        attachments: [
          ...existing,
          ...staged.filter(
            (item) =>
              !existing.some((prior) =>
                item.path ? prior.path === item.path : prior.digest === item.digest,
              ),
          ),
        ],
      });
      focus();
    } catch (error) {
      fail(error);
    }
  }
  async function attachPaths(paths: string[]) {
    const importOwner = key;
    try {
      await addAttachments(await Promise.all([...new Set(paths)].map(importPath)), importOwner);
    } catch (error) {
      fail(error);
    }
  }
  function pasteImage(): boolean {
    const image = native.readClipboardImage();
    if (!image) return false;
    if (image.data.length > maxAttachmentBytes) {
      showToast('Attachments must be smaller than 32 MB');
      return true;
    }
    const dataBase64 = image.data.toString('base64');
    if (!attachments.some((item) => item.dataBase64 === dataBase64))
      void addAttachments(
        [{ name: `image.${image.mime.split('/')[1] ?? 'png'}`, mime: image.mime, dataBase64 }],
        key,
      );
    return true;
  }
  function accept(row: Suggestion) {
    if (!trigger) return;
    const value = `${row.value} `;
    const text = draft.text.slice(0, trigger.start) + value + draft.text.slice(trigger.end);
    flushSync(() => update({ text }));
    const end = trigger.start + value.length;
    setCaret(end);
    setDismissed(undefined);
    if (editor.current) {
      native.inputSetSelection(editor.current.id, end, end);
      focus();
    }
    if (row.agent || row.resource) {
      const source = { start: trigger.start, end: end - 1, value: row.value };
      if (row.agent)
        update({ text, agentTags: [...(draft.agentTags ?? []), { ...row.agent, source }] });
      if (row.resource)
        update({
          text,
          resourceTags: [...(draft.resourceTags ?? []), { ...row.resource, source }],
        });
    }
  }
  function keys(event: KeyEvent) {
    const keyName = event.key;
    const primary = event.modifiers?.ctrl || event.modifiers?.cmd;
    if (
      ghost?.text === draft.text &&
      !popup &&
      !primary &&
      !event.modifiers?.alt &&
      !event.modifiers?.shift &&
      ['tab', 'right'].includes(keyName ?? '') &&
      selectionRange.current.start === selectionRange.current.end
    ) {
      event.preventDefault();
      event.stopPropagation();
      const text = draft.text + ghost.suffix;
      flushSync(() => update({ text }));
      setCaret(text.length);
      setGhost(undefined);
      if (editor.current) native.inputSetSelection(editor.current.id, text.length, text.length);
      return;
    }
    if (primary && keyName === 'v') {
      const image = pasteImage();
      const paths = image ? undefined : native.readClipboardPaths();
      if (paths?.length) void attachPaths(paths);
      if (!image && !paths?.length && editor.current)
        native.inputCommand(editor.current.id, 'paste');
      return;
    }
    if (popup && keyName === 'escape') {
      event.preventDefault();
      event.stopPropagation();
      setDismissed(tokenKey);
      return;
    }
    if (
      popup &&
      rows.length &&
      !primary &&
      !event.modifiers?.alt &&
      !event.modifiers?.shift &&
      ['up', 'down', 'tab'].includes(keyName ?? '')
    ) {
      event.preventDefault();
      event.stopPropagation();
      if (keyName === 'up' || keyName === 'down')
        setHighlight((highlight + (keyName === 'up' ? -1 : 1) + rows.length) % rows.length);
      else accept(rows[highlight] ?? rows[0]!);
      return;
    }
    if (keyName === 'backspace' && !draft.text && attachments.length) {
      event.preventDefault();
      update({ attachments: attachments.slice(0, -1) });
      return;
    }
    if (keyName === 'f10' && event.modifiers?.shift) {
      event.preventDefault();
      openEditMenu();
    }
  }
  function command(name: InputCommand) {
    if (editor.current) {
      focus();
      native.inputCommand(editor.current.id, name);
    }
    setEditMenu(undefined);
  }
  function openEditMenu(event?: NativeEvent) {
    const bounds = editor.current ? renderer?.getElementBounds?.(editor.current.id) : undefined;
    setEditMenu({
      x: event?.x ?? bounds?.x ?? 0,
      y: event?.y ?? (bounds?.y ?? 0) + (bounds?.height ?? 0),
    });
  }
  if (target.kind === 'new' && !target.cwd) return null;
  if (!live && view?.meta && !view.meta.is_resumable)
    return (
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          width: '100%',
          paddingLeft: 20,
          paddingRight: 20,
          alignItems: 'center',
        }}
      >
        <div
          style={{
            maxWidth: 720,
            width: '100%',
            padding: 14,
            borderWidth: 1,
            borderColor: colors.border,
            borderRadius: 12,
            backgroundColor: colors.composer,
          }}
        >
          <text style={{ fontSize: sp(12.5), color: colors.text_secondary }}>
            {view.meta.resume_disabled_reason ?? 'This session is read-only and cannot be resumed.'}
          </text>
        </div>
      </div>
    );
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: '100%',
        paddingLeft: 20,
        paddingRight: 20,
        alignItems: 'center',
        flexShrink: 0,
      }}
    >
      <div style={{ width: '100%', maxWidth: 720, display: 'flex', flexDirection: 'column' }}>
        {sessionId && <Approvals sessionId={sessionId} />}
        <Queue
          items={queue}
          onEdit={(item) => {
            saveQueue(queue.filter((entry) => entry.id !== item.id));
            flushSync(() => store.setDraft(selection, item.draft));
            if (editor.current)
              native.inputSetSelection(
                editor.current.id,
                item.draft.text.length,
                item.draft.text.length,
              );
            focus();
          }}
          onRemove={(id) => saveQueue(queue.filter((entry) => entry.id !== id))}
          onSteer={(item) => void steerItem(item)}
        />
        <div
          onLayout={(event) => {
            if (event.width) setWidth(event.width);
          }}
          onFileDragEnter={() => setDraggingFiles(true)}
          onFileDragLeave={() => setDraggingFiles(false)}
          onFileDrop={(event) => {
            focus();
            setDraggingFiles(false);
            void attachPaths(event.paths ?? []);
          }}
          style={{
            position: 'relative',
            width: '100%',
            paddingTop: 10,
            paddingBottom: 10,
            borderWidth: 1,
            borderColor: draggingFiles ? `${colors.accent}b3` : colors.border,
            borderRadius: 13,
            backgroundColor: composerFill,
            display: 'flex',
            flexDirection: 'column',
            flexShrink: 0,
          }}
        >
          {popup && (
            <Suggestions
              rows={rows}
              selected={highlight}
              loading={loading}
              width={width}
              query={trigger?.query ?? ''}
              onAccept={accept}
              onDismiss={() => setDismissed(tokenKey)}
            />
          )}
          {!!attachments.length && (
            <AttachmentTiles
              attachments={attachments}
              onRemove={(index) =>
                update({ attachments: attachments.filter((_, at) => at !== index) })
              }
            />
          )}
          <textarea
            ref={editor}
            aria-label="Message"
            autoFocus
            value={draft.text}
            placeholder="Do anything…"
            ghostText={ghost?.text === draft.text ? ghost.suffix : undefined}
            minRows={1}
            maxRows={Math.ceil(300 / sp(22))}
            onFocus={() => setFocused(true)}
            onBlur={() => setFocused(false)}
            onKeyDown={keys}
            interceptedKeys={[
              'ctrl-v',
              'cmd-v',
              ...(popup && rows.length ? ['up', 'down', 'tab'] : []),
              ...(ghost?.text === draft.text && !popup ? ['tab', 'right'] : []),
              ...(!draft.text && attachments.length ? ['backspace'] : []),
            ]}
            onSubmit={(event) => {
              const primary = event.modifiers?.ctrl || event.modifiers?.cmd;
              if (popup && rows.length && !primary) accept(rows[highlight] ?? rows[0]!);
              else {
                const current = {
                  ...(store.getDraft(selection) ?? emptyDraft),
                  text: event.value ?? draft.text,
                };
                if (current.text.trim() || primary) void submit(!!primary, current);
              }
            }}
            onMouseDown={(event) => {
              if (event.button === 2) openEditMenu(event as NativeEvent);
            }}
            onChange={(event) => {
              const input = event as NativeEvent;
              update({ text: input.value ?? '' });
              const end = input.selectionEnd ?? (input.value ?? '').length;
              selectionRange.current = { start: input.selectionStart ?? end, end };
              setCaret(end);
            }}
            theme={{
              appearance: colors.is_dark ? 'dark' : 'light',
              text: colors.text,
              textMuted: colors.text_ghost,
              caret: colors.accent,
              bg: composerFill,
              accent: colors.inverse,
              ...{
                placeholder: colors.text_ghost,
                selectionWash: colors.overlay_strong,
                caretWidth: 1.5,
              },
            }}
            style={{
              minHeight: 26,
              maxHeight: 302,
              paddingLeft: 14,
              paddingRight: 14,
              paddingTop: 3,
              width: '100%',
              fontSize: sp(13.5),
              lineHeight: sp(22),
              color: colors.text,
              backgroundColor: composerFill,
            }}
          />
          <div
            style={{
              display: 'flex',
              flexDirection: 'row',
              alignItems: 'center',
              gap: 4,
              paddingLeft: 10,
              paddingRight: 10,
              marginTop: 8,
            }}
          >
            <ModelControls
              cwd={cwd}
              sessionId={sessionId}
              selection={draft.selection}
              accessMode={accessMode}
              onSelection={(value: ModelSelection) => {
                update({ selection: value });
                if (sessionId)
                  void store
                    .sendIntent(sessionId, { kind: 'switchModel', selection: value })
                    .catch(fail);
                focus();
              }}
              onAccessMode={(mode) => {
                update({ accessMode: mode });
                store.setPreferences({ lastAccessMode: mode });
                if (sessionId)
                  void store
                    .sendIntent(sessionId, { kind: 'setAccessMode', mode })
                    .then(() => setRuntimeAccess({ sessionId, mode }))
                    .catch(fail);
                focus();
              }}
            />
            <div style={{ flexGrow: 1 }} />
            {preparing ? (
              <Tooltip content="Preparing task…">
                <div
                  style={{
                    width: 26,
                    height: 26,
                    borderRadius: 26,
                    backgroundColor: colors.overlay_strong,
                    alignItems: 'center',
                    justifyContent: 'center',
                  }}
                >
                  <Spinner size={15} />
                </div>
              </Tooltip>
            ) : (
              <>
                {busy && (
                  <Tooltip content={stopArmed ? 'Press Escape again to stop' : 'Stop'}>
                    <Button
                      aria-label="Stop"
                      onClick={() => {
                        if (sessionId) void store.interruptSession(sessionId).catch(fail);
                      }}
                      style={{
                        width: 26,
                        height: 26,
                        borderRadius: 26,
                        backgroundColor: colors.overlay_strong,
                        hover: { backgroundColor: colors.danger_soft },
                      }}
                    >
                      {stopArmed ? (
                        <text style={{ fontSize: sp(12.5), fontWeight: 600, color: colors.text }}>
                          Esc
                        </text>
                      ) : (
                        <Icon name="stop" size={18} color={colors.text} />
                      )}
                    </Button>
                  </Tooltip>
                )}
                {(!busy || sendable) && (
                  <Tooltip
                    content={
                      !canSubmit
                        ? 'Install or switch on a provider to send'
                        : busy
                          ? 'Queue as follow-up (⏎)'
                          : 'Send'
                    }
                  >
                    <Button
                      aria-label={busy ? 'Queue as follow-up' : 'Send'}
                      disabled={!sendable || !canSubmit}
                      onClick={() => void submit()}
                      style={{
                        marginLeft: busy ? 2 : 0,
                        width: 26,
                        height: 26,
                        borderRadius: 26,
                        backgroundColor:
                          sendable && canSubmit ? colors.inverse : colors.overlay_strong,
                        opacity: 1,
                        hover: { opacity: 0.9 },
                        active: { opacity: 0.8 },
                      }}
                    >
                      <Icon
                        name="arrow-up"
                        size={16}
                        color={sendable && canSubmit ? colors.on_inverse : colors.text_ghost}
                      />
                    </Button>
                  </Tooltip>
                )}
              </>
            )}
          </div>
          {editMenu && (
            <Menu
              position={editMenu}
              restoreFocusId={editor.current?.id}
              onClose={() => setEditMenu(undefined)}
              items={[
                {
                  id: 'cut',
                  label: 'Cut',
                  disabled: selectionRange.current.start === selectionRange.current.end,
                  onSelect: () => command('cut'),
                },
                {
                  id: 'copy',
                  label: 'Copy',
                  disabled: selectionRange.current.start === selectionRange.current.end,
                  onSelect: () => command('copy'),
                },
                {
                  id: 'paste',
                  label: 'Paste',
                  disabled: !native.readClipboardText(),
                  onSelect: () => command('paste'),
                },
                { id: 'divider', type: 'separator' },
                {
                  id: 'all',
                  label: 'Select All',
                  disabled:
                    !draft.text ||
                    (selectionRange.current.start === 0 &&
                      selectionRange.current.end === draft.text.length),
                  onSelect: () => command('selectAll'),
                },
              ]}
            />
          )}
        </div>
      </div>
    </div>
  );
}
export function WorkspaceFooter({ target }: { target: ComposerTarget }) {
  const store = getAppStore();
  const projects = useStore((state) => state.projects);
  const selection = selectionFor(target);
  const draft = useDraft(selection) ?? emptyDraft;
  const { colors } = useTheme();
  const { showToast } = useToast();
  const sessionId = target.kind === 'task' ? target.sessionId : undefined;
  const view = useSessionView(sessionId);
  const current = projects.find((project) => project.id === selection.projectId);
  const [worktreeOpen, setWorktreeOpen] = useState(false);
  useEffect(
    () =>
      registerAction({
        id: 'worktree.open',
        label: 'Choose worktree',
        run: () => {
          if (sessionId) runAction('task.new');
          setWorktreeOpen(true);
        },
      }),
    [sessionId],
  );
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: '100%',
        paddingLeft: 20,
        paddingRight: 20,
        alignItems: 'center',
        flexShrink: 0,
      }}
    >
      <div
        style={{
          height: 40,
          width: '100%',
          maxWidth: 720,
          paddingLeft: 10,
          paddingRight: 10,
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'center',
          gap: 4,
          color: colors.text_secondary,
        }}
      >
        <ProjectSelector
          projects={projects}
          currentProjectId={current?.id}
          site="composer"
          onSelect={(id) => {
            if (id) {
              const next = { kind: 'project' as const, projectId: id };
              store.moveDraft(selection, next);
              store.selectProject(id);
            }
          }}
          onAddProject={(path) => {
            const id = crypto.randomUUID();
            store.setProjects([
              ...projects,
              { id, cwd: path, name: path.split('/').filter(Boolean).pop() ?? path },
            ]);
            store.setDraft({ kind: 'project', projectId: id }, draft);
            store.selectProject(id);
          }}
          onError={showToast}
        />
        <Dropdown
          open={worktreeOpen}
          onOpenChange={setWorktreeOpen}
          label={draft.worktree ? 'New worktree' : 'Local'}
          icon="monitor"
          chevron={false}
          disabled={!!sessionId}
          menuSide="top"
          items={[
            {
              id: 'local',
              label: 'Local',
              selected: !draft.worktree,
              onSelect: () => store.setDraft(selection, { ...draft, worktree: undefined }),
            },
            {
              id: 'worktree',
              label: 'New worktree',
              selected: !!draft.worktree,
              onSelect: () => store.setDraft(selection, { ...draft, worktree: {} }),
            },
          ]}
        />
        <BranchSelector
          workspacePath={target.cwd ?? undefined}
          projectName={current?.name}
          busy={view?.status === 'working'}
          planned={!!draft.worktree}
          branch={draft.worktree?.baseBranch}
          onBranchChange={(_snapshot, branch) => {
            if (draft.worktree) {
              store.setDraft(selection, {
                ...draft,
                worktree: { ...draft.worktree, baseBranch: branch },
              });
            } else {
              if (target.cwd) store.invalidate(target.cwd);
              store.setLayout({ branchRevision: Date.now() });
            }
          }}
          onError={showToast}
        />
        <div style={{ flexGrow: 1 }} />
        <UsageMeter sessionId={sessionId ?? null} cwd={target.cwd} />
      </div>
    </div>
  );
}

import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useGpuix, useWindowSize, type EventPayload, type KeyEvent } from '@gpuix/react';
import { ThemeProvider, useTheme } from '../theme';
import { Button, Icon, ToastHost, ToastProvider, Tooltip, useToast } from '../ui';
import { memoryPersist, type Persist } from '../state/persist';
import { fitPanels, usePanelLayout } from '../state/layout';
import { Panel } from './Panel';
import { Welcome } from './Welcome';
import { t } from '../i18n';

// Slots contain panel bodies, not outer layout; composer/footer render only after project selection.
export interface AppProps {
  sidebar?: ReactNode;
  main?: ReactNode;
  rightPanel?: ReactNode;
  composer?: ReactNode;
  footer?: ReactNode;
  settings?: ReactNode;
  selectedProject?: { name: string; projectless?: boolean } | null;
  title?: string;
  sessionKey?: string;
  persist?: Persist;
  onNewTask?(): void;
  onOpenProject?(): void;
  onProjectless?(): void;
  onCommandPalette?(): void;
  onSettings?(): void;
  onBack?(): void;
  onForward?(): void;
  canBack?: boolean;
  canForward?: boolean;
  onProjectSelector?(): void;
}

export function App(props: AppProps = {}) {
  return (
    <ToastProvider>
      <ThemedApp {...props} />
    </ToastProvider>
  );
}

function ThemedApp(props: AppProps) {
  const { showToast } = useToast();
  return (
    <ThemeProvider
      persist={props.persist}
      onError={(error) => showToast(t('notice.saveFailed', { error: String(error) }), 'alert')}
    >
      <Workspace {...props} />
    </ThemeProvider>
  );
}

export function Workspace(props: AppProps) {
  const { colors, sp } = useTheme();
  const { renderer } = useGpuix();
  const size = useWindowSize();
  const viewport = size.width || 1380;
  const { showToast, clearToast } = useToast();
  const previousSession = useRef(props.sessionKey);
  useEffect(() => {
    if (previousSession.current === props.sessionKey) return;
    previousSession.current = props.sessionKey;
    clearToast();
  }, [props.sessionKey, clearToast]);
  const [panelDragging, setPanelDragging] = useState(false);
  const [layout, setLayout] = usePanelLayout(
    props.persist ?? memoryPersist,
    (error) => showToast(t('notice.saveFailed', { error: String(error) }), 'alert'),
    panelDragging,
  );
  const dragArmed = useRef(false);
  const titlebar = {
    onMouseDown: (event: EventPayload) => {
      if (event.button !== 0) return;
      if ((event.clickCount ?? 0) >= 2) {
        renderer?.zoomWindow?.();
        dragArmed.current = false;
      } else dragArmed.current = true;
    },
    onMouseMove: () => {
      if (!dragArmed.current) return;
      dragArmed.current = false;
      const native = renderer as typeof renderer & { startWindowMove?: () => void };
      native?.startWindowMove?.();
    },
    onMouseUp: () => {
      dragArmed.current = false;
    },
    onMouseDownOutside: () => {
      dragArmed.current = false;
    },
  };
  const [localProject, setLocalProject] = useState<{ name: string; projectless?: boolean } | null>(
    null,
  );
  const selectedProject =
    props.selectedProject === undefined ? localProject : props.selectedProject;
  const fitted = fitPanels(viewport, layout);
  const toggleSidebar = () => setLayout((old) => ({ ...old, sidebarVisible: !old.sidebarVisible }));
  const toggleRight = () => setLayout((old) => ({ ...old, rightVisible: !old.rightVisible }));
  const openProject =
    props.onOpenProject ??
    (() => {
      void renderer
        ?.promptForPaths?.({ directories: true, files: false, multiple: false })
        .then((paths) => {
          const path = paths?.[0];
          if (path) setLocalProject({ name: path.split('/').filter(Boolean).at(-1) ?? path });
        });
    });
  const projectless =
    props.onProjectless ??
    (() => setLocalProject({ name: t('welcome.withoutProject'), projectless: true }));
  const newTask = props.onNewTask ?? (() => {});
  const keyDown = (event: KeyEvent) => {
    if (!event.modifiers?.ctrl || event.modifiers.alt || event.modifiers.cmd) return;
    const key = event.key?.toLowerCase();
    if (key === 'b') {
      event.modifiers.shift ? toggleRight() : toggleSidebar();
    } else if (!event.modifiers.shift && key === 'k') props.onCommandPalette?.();
    else if (!event.modifiers.shift && key === 'n') newTask();
    else if (!event.modifiers.shift && key === 'o') openProject();
    else if (!event.modifiers.shift && key === ',') props.onSettings?.();
    else if (!event.modifiers.shift && key === '[') props.onBack?.();
    else if (!event.modifiers.shift && key === ']') props.onForward?.();
    else return;
    event.preventDefault();
    event.stopPropagation();
  };
  const chromeButton = (
    icon: string,
    label: string,
    action?: () => void,
    disabled = false,
    width = 26,
    height = 26,
  ) => (
    <Tooltip content={label}>
      <Button
        aria-label={label}
        disabled={disabled}
        onClick={action}
        style={{
          width,
          height,
          flexShrink: 0,
          borderRadius: 6,
          cursor: 'default',
          opacity: disabled ? 0.35 : 1,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          hover: { backgroundColor: colors.overlay },
          active: { backgroundColor: colors.overlay_strong },
        }}
      >
        <Icon name={icon} size={14} color={colors.text_tertiary} />
      </Button>
    </Tooltip>
  );
  const navigation = (
    <div style={{ display: 'flex', alignItems: 'center' }}>
      {chromeButton('panel-left', t('action.sidebar'), toggleSidebar)}
      <div style={{ marginLeft: 6, display: 'flex', gap: 2 }}>
        {chromeButton('arrow-left', t('navigation.back'), props.onBack, !props.canBack)}
        {chromeButton('arrow-right', t('navigation.forward'), props.onForward, !props.canForward)}
      </div>
    </div>
  );
  const sidebarRow = (icon: string, label: string, action?: () => void) => (
    <Button
      onClick={action}
      style={{
        height: 32,
        width: '100%',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'flex-start',
        paddingLeft: 4,
        paddingRight: 4,
        borderRadius: 7,
        gap: 10,
        cursor: 'default',
        hover: { backgroundColor: colors.sidebar_item_background },
        active: { backgroundColor: colors.overlay_strong },
      }}
    >
      <div style={{ width: 20, display: 'flex', justifyContent: 'center' }}>
        <Icon name={icon} size={14} color={colors.text_secondary} />
      </div>
      <text style={{ color: colors.text_secondary, fontSize: sp(13), whiteSpace: 'nowrap' }}>
        {label}
      </text>
    </Button>
  );
  if (props.settings)
    return (
      <div
        onKeyDown={keyDown}
        style={{
          width: '100%',
          height: '100%',
          fontFamily: '.SystemUIFont',
          fontSize: sp(14),
          backgroundColor: colors.canvas,
          position: 'relative',
        }}
      >
        {props.settings}
        <ToastHost />
      </div>
    );
  return (
    <div
      autoFocus
      tabIndex={-1}
      onKeyDown={keyDown}
      style={{
        display: 'flex',
        width: '100%',
        height: '100%',
        backgroundColor: colors.canvas,
        fontFamily: '.SystemUIFont',
        fontSize: sp(14),
        overflow: 'hidden',
      }}
    >
      <Panel
        side="left"
        width={layout.sidebarVisible ? fitted.sidebar : layout.sidebarWidth}
        visible={layout.sidebarVisible}
        onDraggingChange={setPanelDragging}
        onResize={(width) =>
          setLayout((old) => ({
            ...old,
            sidebarWidth: Math.max(180, Math.min(420, viewport - fitted.right - 360, width)),
          }))
        }
      >
        <div
          style={{
            width: '100%',
            height: '100%',
            display: 'flex',
            flexDirection: 'column',
            backgroundColor: colors.sidebar,
            borderRightWidth: 1,
            borderColor: colors.sidebar_border,
          }}
        >
          {props.sidebar ?? (
            <>
              <div
                style={{
                  height: 48,
                  flexShrink: 0,
                  display: 'flex',
                  alignItems: 'center',
                  paddingLeft: 8,
                }}
              >
                {navigation}
                <div {...titlebar} style={{ flexGrow: 1, height: '100%' }} />
              </div>
              <div style={{ marginLeft: 10, marginRight: 10 }}>
                {sidebarRow('compose', t('action.newTask'), newTask)}
              </div>
              <div style={{ flexGrow: 1, minHeight: 0, marginLeft: 10, marginRight: 10 }}>
                <div style={{ marginBottom: 10 }}>
                  {sidebarRow('search', t('navigation.search'), props.onCommandPalette)}
                </div>
                <div
                  style={{
                    height: 28,
                    display: 'flex',
                    alignItems: 'center',
                    paddingLeft: 8,
                    paddingRight: 8,
                  }}
                >
                  <text
                    style={{
                      flexGrow: 1,
                      color: colors.text_secondary,
                      fontSize: sp(13),
                      fontWeight: 500,
                    }}
                  >
                    {t('navigation.today')}
                  </text>
                  {chromeButton('list-filter', t('navigation.options'), undefined, false, 20, 20)}
                  <div style={{ width: 2 }} />
                  {chromeButton(
                    'folder-new',
                    t('navigation.newProject'),
                    openProject,
                    false,
                    20,
                    22,
                  )}
                </div>
              </div>
              <div
                style={{
                  height: 40,
                  flexShrink: 0,
                  paddingLeft: 10,
                  paddingRight: 10,
                  display: 'flex',
                  alignItems: 'center',
                }}
              >
                {chromeButton('settings', t('navigation.settings'), props.onSettings)}
              </div>
            </>
          )}
        </div>
      </Panel>
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          flexGrow: 1,
          minWidth: 0,
          height: '100%',
          backgroundColor: colors.surface,
          position: 'relative',
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            height: 48,
            flexShrink: 0,
            gap: 8,
            paddingLeft: layout.sidebarVisible ? 14 : 8,
            paddingRight: 14,
          }}
        >
          {!layout.sidebarVisible && navigation}
          <text
            {...titlebar}
            style={{
              color: colors.text,
              fontSize: sp(13),
              fontWeight: 500,
              whiteSpace: 'nowrap',
              textOverflow: 'ellipsis',
              minWidth: 0,
            }}
          >
            {props.title ?? t('task.untitled')}
          </text>
          <div {...titlebar} style={{ flexGrow: 1, height: '100%' }} />
          {chromeButton('info', t('navigation.usage'), undefined)}
          {chromeButton('panel-right', t('action.rightPanel'), toggleRight)}
        </div>
        {props.main ?? (
          <Welcome
            selectedProject={selectedProject}
            onOpenProject={openProject}
            onProjectless={projectless}
            onProjectSelector={props.onProjectSelector}
          />
        )}
        {selectedProject && props.composer}
        {selectedProject && props.footer}
        <ToastHost />
      </div>
      <Panel
        side="right"
        width={layout.rightVisible ? fitted.right : layout.rightWidth}
        visible={layout.rightVisible}
        onDraggingChange={setPanelDragging}
        onResize={(width) =>
          setLayout((old) => ({
            ...old,
            rightWidth: Math.max(280, Math.min(1000, viewport - fitted.sidebar - 360, width)),
          }))
        }
      >
        <div
          style={{
            width: '100%',
            height: '100%',
            backgroundColor: colors.surface,
            borderLeftWidth: 1,
            borderColor: colors.border,
          }}
        >
          {props.rightPanel}
        </div>
      </Panel>
    </div>
  );
}

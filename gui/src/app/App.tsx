import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import {
  useGpuix,
  useWindowSize,
  type EventPayload,
  type KeyEvent,
  type StyleDesc,
} from '@gpuix/react';
import { ThemeProvider, useTheme } from '../theme';
import { Button, Icon, ToastHost, ToastProvider, Tooltip, useToast } from '../ui';
import { memoryPersist, type Persist } from '../state/persist';
import { fitPanels, usePanelLayout } from '../state/layout';
import { Panel } from './PanelFrame';
import { Welcome } from './Welcome';
import { t } from '../i18n';
import { registerAction, runAction } from './actions';
import { nativeRenderer, type ResizeEdge, type WindowDecorations } from '../native';

// Slots contain panel bodies, not outer layout; composer/footer render only after project selection.
export interface AppProps {
  sidebar?: ReactNode;
  renderSidebar?(header: ReactNode): ReactNode;
  overlay?: ReactNode;
  headerActions?: ReactNode;
  fps?: ReactNode;
  onKeyDown?(event: KeyEvent): void;
  onKeyUp?(event: KeyEvent): void;
  main?: ReactNode;
  rightPanel?: ReactNode;
  renderRightPanel?(controls: ReactNode): ReactNode;
  composer?: ReactNode;
  footer?: ReactNode;
  settings?: ReactNode;
  selectedProject?: { name: string; projectless?: boolean } | null;
  title?: string;
  requestedRightWidth?: number;
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
  projectSelector?: ReactNode;
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
      <WindowFrame>
        <Workspace {...props} />
      </WindowFrame>
    </ThemeProvider>
  );
}

const WindowChrome = createContext<ReactNode>(null);
function WindowFrame({ children }: { children: ReactNode }) {
  const { renderer } = useGpuix();
  const { colors, preference } = useTheme();
  const [decorations, setDecorations] = useState<WindowDecorations>({
    kind: 'server',
    tiling: { top: false, bottom: false, left: false, right: false },
  });
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    if (!renderer) return;
    const sample = () => {
      const native = nativeRenderer(renderer);
      const next = native.windowDecorations();
      setDecorations((old) => (JSON.stringify(old) === JSON.stringify(next) ? old : next));
      setMaximized(native.isWindowMaximized());
    };
    sample();
    const timer = setInterval(sample, 500);
    return () => clearInterval(timer);
  }, [renderer]);
  if (decorations.kind === 'server' || !renderer) return children;
  const native = nativeRenderer(renderer);
  const controls = native.windowControls();
  const buttons = [
    {
      icon: 'window-minimize',
      label: t('frame.minimize'),
      enabled: controls.minimize,
      run: () => renderer.minimizeWindow?.(),
    },
    {
      icon: maximized ? 'window-restore' : 'window-maximize',
      label: t(maximized ? 'frame.restore' : 'frame.maximize'),
      enabled: controls.maximize,
      run: () => renderer.zoomWindow?.(),
    },
    {
      icon: 'x',
      label: t('action.closeWindow'),
      enabled: true,
      run: () => runAction('shell.quit'),
    },
  ];
  const chrome = (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        paddingLeft: 10,
        paddingRight: 10,
        gap: 8,
        height: 48,
        flexShrink: 0,
      }}
    >
      {buttons.map((button) => (
        <Tooltip key={button.icon} content={button.label}>
          <Button
            disabled={!button.enabled}
            aria-label={button.label}
            onClick={button.run}
            style={{
              width: 26,
              height: 26,
              borderRadius: 13,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              opacity: button.enabled ? 1 : 0.45,
              hover: { backgroundColor: button.icon === 'x' ? colors.danger_soft : colors.overlay },
              active: { backgroundColor: colors.overlay_strong },
            }}
          >
            <Icon
              name={button.icon}
              size={14}
              color={button.enabled ? colors.text_secondary : colors.text_ghost}
            />
          </Button>
        </Tooltip>
      ))}
    </div>
  );
  const tiling = decorations.tiling;
  const tiled = tiling.top || tiling.bottom || tiling.left || tiling.right;
  const edges: { edge: ResizeEdge; enabled: boolean; style: StyleDesc }[] = [
    {
      edge: 'n',
      enabled: !tiling.top,
      style: { top: 0, left: 10, right: 10, height: 10, cursor: 'ns-resize' },
    },
    {
      edge: 's',
      enabled: !tiling.bottom,
      style: { bottom: 0, left: 10, right: 10, height: 10, cursor: 'ns-resize' },
    },
    {
      edge: 'w',
      enabled: !tiling.left,
      style: { left: 0, top: 10, bottom: 10, width: 10, cursor: 'ew-resize' },
    },
    {
      edge: 'e',
      enabled: !tiling.right,
      style: { right: 0, top: 10, bottom: 10, width: 10, cursor: 'ew-resize' },
    },
    {
      edge: 'nw',
      enabled: !tiling.top && !tiling.left,
      style: { top: 0, left: 0, width: 10, height: 10, cursor: 'nwse-resize' },
    },
    {
      edge: 'ne',
      enabled: !tiling.top && !tiling.right,
      style: { top: 0, right: 0, width: 10, height: 10, cursor: 'nesw-resize' },
    },
    {
      edge: 'sw',
      enabled: !tiling.bottom && !tiling.left,
      style: { bottom: 0, left: 0, width: 10, height: 10, cursor: 'nesw-resize' },
    },
    {
      edge: 'se',
      enabled: !tiling.bottom && !tiling.right,
      style: { bottom: 0, right: 0, width: 10, height: 10, cursor: 'nwse-resize' },
    },
  ];
  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        position: 'relative',
        paddingTop: tiling.top ? 0 : 10,
        paddingBottom: tiling.bottom ? 0 : 10,
        paddingLeft: tiling.left ? 0 : 10,
        paddingRight: tiling.right ? 0 : 10,
      }}
    >
      <div
        style={{
          width: '100%',
          height: '100%',
          overflow: 'hidden',
          borderTopWidth: tiling.top ? 0 : 1,
          borderBottomWidth: tiling.bottom ? 0 : 1,
          borderLeftWidth: tiling.left ? 0 : 1,
          borderRightWidth: tiling.right ? 0 : 1,
          borderColor: colors.border_strong,
          borderTopLeftRadius: tiling.top || tiling.left ? 0 : 10,
          borderTopRightRadius: tiling.top || tiling.right ? 0 : 10,
          borderBottomLeftRadius: tiling.bottom || tiling.left ? 0 : 10,
          borderBottomRightRadius: tiling.bottom || tiling.right ? 0 : 10,
          boxShadow: tiled
            ? undefined
            : {
                offsetX: 0,
                offsetY: 0,
                blurRadius: 5,
                spreadRadius: 0,
                color: preference === 'light' ? '#00000040' : '#00000080',
              },
        }}
      >
        <WindowChrome.Provider value={chrome}>{children}</WindowChrome.Provider>
      </div>
      {edges
        .filter((item) => item.enabled)
        .map(({ edge, style }) => (
          <div
            key={edge}
            style={{ position: 'absolute', ...style }}
            onMouseDown={(event) => {
              if (event.button === 0) native.startWindowResize(edge);
            }}
          />
        ))}
    </div>
  );
}

export function Workspace(props: AppProps) {
  const { colors, sp } = useTheme();
  const { renderer } = useGpuix();
  const windowControls = useContext(WindowChrome);
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
  useEffect(() => {
    if (props.requestedRightWidth === undefined) return;
    const width = Math.max(280, Math.min(1000, props.requestedRightWidth));
    setLayout((old) => ({ ...old, rightWidth: width, rightVisible: true }));
  }, [props.requestedRightWidth, setLayout]);
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
  useEffect(() => {
    const actions = [
      registerAction({
        id: 'shell.toggleSidebar',
        label: 'action.sidebar',
        shortcut: 'Ctrl+B',
        run: toggleSidebar,
      }),
      registerAction({
        id: 'shell.toggleRightPanel',
        label: 'action.rightPanel',
        shortcut: 'Ctrl+Shift+B',
        run: toggleRight,
      }),
      registerAction({
        id: 'shell.showRightPanel',
        label: 'action.rightPanel',
        run: () => setLayout((old) => ({ ...old, rightVisible: true })),
      }),
      registerAction({
        id: 'shell.hideRightPanel',
        label: 'action.rightPanel',
        run: () => setLayout((old) => ({ ...old, rightVisible: false })),
      }),
    ];
    return () => actions.forEach((remove) => remove());
  }, [setLayout]);
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
    props.onKeyDown?.(event);
    if (event.defaultPrevented) return;
    if (!event.modifiers?.ctrl || event.modifiers.alt || event.modifiers.cmd) return;
    const key = event.key?.toLowerCase();
    if (key === 'b') {
      runAction(event.modifiers.shift ? 'shell.toggleRightPanel' : 'shell.toggleSidebar');
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
  return (
    <div
      onKeyDown={keyDown}
      onKeyUp={props.onKeyUp}
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: '100%',
        height: '100%',
        position: 'relative',
      }}
    >
      <div
        autoFocus
        tabIndex={-1}
        onMouseDown={(event) => {
          if (event.button === 3) props.onBack?.();
          if (event.button === 4) props.onForward?.();
        }}
        style={{
          display: props.settings ? 'none' : 'flex',
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
          width={(layout.sidebarVisible ? fitted.sidebar : layout.sidebarWidth) + 1}
          visible={layout.sidebarVisible}
          onDraggingChange={setPanelDragging}
          onResize={(width) =>
            setLayout((old) => ({
              ...old,
              sidebarWidth: Math.max(180, Math.min(420, viewport - fitted.right - 361, width - 1)),
            }))
          }
        >
          <div
            style={{
              width: '100%',
              paddingRight: 1,
              height: '100%',
              display: 'flex',
              flexDirection: 'column',
              backgroundColor: colors.sidebar_border,
            }}
          >
            <div
              style={{
                width: '100%',
                height: '100%',
                backgroundColor: colors.sidebar,
                display: 'flex',
                flexDirection: 'column',
              }}
            >
              {props.renderSidebar?.(
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
                </div>,
              ) ?? props.sidebar}
            </div>
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
            {props.headerActions}
            {!layout.rightVisible && props.fps}
            {chromeButton('info', t('navigation.usage'), () => runAction('usage.toggle'))}
            {chromeButton('panel-right', t('action.rightPanel'), toggleRight)}
            {!layout.rightVisible && windowControls}
          </div>
          {props.main ?? (
            <Welcome
              selectedProject={selectedProject}
              onOpenProject={openProject}
              onProjectless={projectless}
              onProjectSelector={props.onProjectSelector}
              projectSelector={props.projectSelector}
            />
          )}
          {selectedProject && props.composer}
          {selectedProject && props.footer}
          <ToastHost />
        </div>
        <Panel
          side="right"
          keepMounted
          resizing={!!props.settings}
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
            {props.renderRightPanel?.(
              <>
                {props.fps}
                {windowControls}
              </>,
            ) ?? props.rightPanel}
          </div>
        </Panel>
      </div>
      {props.settings && (
        <div
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
      )}
      {props.overlay}
    </div>
  );
}

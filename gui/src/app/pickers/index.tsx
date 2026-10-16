import { useEffect, useRef, useState, type ReactNode } from 'react';
import { DismissableLayer, useGpuix, type KeyEvent, type PublicInstance } from '@gpuix/react';
import { useTheme } from '../../theme';
import { Button, Icon, MenuChip, TextField, ellipsis, row } from '../../ui';
import type { IconName } from '../../ui/icon-names';
import { Scrollbar } from '../../ui/scrollbar';
export { BranchSelector, type BranchSelectorProps } from './branches';

export interface PickerProject {
  id: string;
  name: string;
  cwd: string;
  projectless?: boolean;
}
export type ProjectPickerSite = 'composer' | 'empty';
export interface ProjectSelectorProps {
  projects: readonly PickerProject[];
  currentProjectId?: string | null;
  site: ProjectPickerSite;
  onSelect: (id: string | null, site: ProjectPickerSite) => void;
  onAddProject: (path: string, site: ProjectPickerSite) => void | Promise<void>;
  onError?: (message: string) => void;
  restoreFocusId?: number;
  label?: string;
  onRefresh?: () => void;
}
const normalize = (value: string) =>
  value.normalize('NFKD').replace(/\p{M}/gu, '').toLocaleLowerCase();
export function projectMatchScore(name: string, query: string): number | null {
  const text = normalize(name),
    needle = normalize(query.trim());
  if (!needle) return 0;
  let at = 0,
    score = 0,
    previous = -2;
  for (const character of needle) {
    const index = text.indexOf(character, at);
    if (index < 0) return null;
    score += index === previous + 1 ? 8 : 0;
    score += index === 0 || /[\s_./-]/.test(text[index - 1] ?? '') ? 12 : 0;
    score -= index - at;
    previous = index;
    at = index + 1;
  }
  return score;
}
export function filterProjects(
  projects: readonly PickerProject[],
  query: string,
  current?: string | null,
) {
  return projects
    .map((project, order) => ({ project, order, score: projectMatchScore(project.name, query) }))
    .filter((item) => !item.project.projectless && item.score !== null)
    .sort((a, b) =>
      query.trim()
        ? b.score! - a.score! || a.project.name.length - b.project.name.length || a.order - b.order
        : Number(b.project.id === current) - Number(a.project.id === current) || a.order - b.order,
    )
    .map(({ project }) => project);
}
export function PickerRow({
  label,
  icon,
  checked,
  highlighted,
  disabled,
  onSelect,
  onHover,
  rowRef,
}: {
  label: string;
  icon?: IconName;
  checked?: boolean;
  highlighted?: boolean;
  disabled?: boolean;
  onSelect: () => void;
  onHover: () => void;
  rowRef?: (instance: PublicInstance | null) => void;
}) {
  const { colors, sp } = useTheme();
  return (
    <div
      ref={rowRef}
      role="option"
      aria-label={label}
      aria-selected={checked}
      aria-disabled={disabled}
      onMouseEnter={() => {
        if (!disabled) onHover();
      }}
      onMouseDown={(event) => {
        if (event.button === 0 && !disabled) onSelect();
      }}
      style={{
        ...row,
        height: 26,
        flexShrink: 0,
        marginLeft: 4,
        marginRight: 4,
        paddingLeft: 8,
        paddingRight: 8,
        gap: 8,
        borderRadius: 6,
        backgroundColor: highlighted ? colors.overlay_strong : undefined,
        hover: disabled
          ? undefined
          : { backgroundColor: highlighted ? colors.overlay_strong : colors.overlay },
        active: disabled ? undefined : { opacity: 0.85 },
      }}
    >
      {icon && (
        <Icon name={icon} size={12} color={disabled ? colors.text_ghost : colors.text_secondary} />
      )}
      <text
        style={{
          ...ellipsis,
          flexGrow: 1,
          fontSize: sp(12.5),
          lineHeight: sp(15),
          fontWeight: 400,
          color: disabled ? colors.text_ghost : colors.text,
        }}
      >
        {label}
      </text>
      {checked && <Icon name="check" size={11} color={colors.text_secondary} />}
    </div>
  );
}
export function PickerSeparator() {
  const { colors } = useTheme();
  return (
    <div
      style={{
        height: 1,
        flexShrink: 0,
        marginLeft: 6,
        marginRight: 6,
        marginTop: 4,
        marginBottom: 4,
        backgroundColor: colors.border,
      }}
    />
  );
}
export function PickerEmpty({ children }: { children: ReactNode }) {
  const { colors, sp } = useTheme();
  return (
    <div style={{ ...row, height: 64, justifyContent: 'center' }}>
      <text style={{ fontSize: sp(12.5), color: colors.text_ghost }}>{children}</text>
    </div>
  );
}
export function ProjectPicker(
  props: ProjectSelectorProps & { onClose: () => void; triggerId?: number },
) {
  const { colors } = useTheme();
  const { renderer } = useGpuix();
  const input = useRef<PublicInstance>(null);
  const rows = useRef(new Map<number, PublicInstance>());
  const list = useRef<PublicInstance>(null);
  const [offset, setOffset] = useState(0);
  const [query, setQuery] = useState('');
  const [cursor, setCursor] = useState(-1);
  const projects = filterProjects(props.projects, query, props.currentProjectId);
  useEffect(() => {
    props.onRefresh?.();
  }, []);
  useEffect(() => {
    rows.current.get(cursor)?.scrollIntoView?.();
  }, [cursor, query]);
  function choose(index: number) {
    const project = projects[index];
    props.onClose();
    if (project) {
      if (project.id !== props.currentProjectId) props.onSelect(project.id, props.site);
    } else if (index === projects.length) {
      void (async () => {
        try {
          if (!renderer?.promptForPaths) throw new Error('The native folder picker is unavailable');
          const paths = await renderer.promptForPaths({
            directories: true,
            files: false,
            multiple: false,
          });
          if (paths?.[0]) await props.onAddProject(paths[0], props.site);
        } catch (error) {
          props.onError?.(String(error));
        }
      })();
    } else if (
      props.currentProjectId !== null &&
      !props.projects.find((p) => p.id === props.currentProjectId)?.projectless
    ) {
      props.onSelect(null, props.site);
    }
  }
  function keys(event: KeyEvent) {
    const key = event.key;
    if (event.modifiers?.alt || event.modifiers?.cmd || event.modifiers?.ctrl) return;
    if (key === 'escape') {
      event.preventDefault();
      event.stopPropagation();
      if (query) {
        setQuery('');
        setCursor(-1);
      } else props.onClose();
    } else if (key === 'down' || key === 'up' || key === 'tab') {
      event.preventDefault();
      event.stopPropagation();
      const count = projects.length + 2,
        backwards = key === 'up' || (key === 'tab' && event.modifiers?.shift);
      setCursor((at) =>
        at < 0 ? (backwards ? count - 1 : 0) : (at + (backwards ? -1 : 1) + count) % count,
      );
    } else if (key === 'enter') {
      event.preventDefault();
      event.stopPropagation();
      choose(cursor < 0 ? 0 : cursor);
    }
  }
  return (
    <DismissableLayer
      initialFocus={() => input.current?.id ?? null}
      finalFocus={(previous) => props.restoreFocusId ?? previous}
      onEscapeKeyDown={() => {
        if (query) {
          setQuery('');
          setCursor(-1);
        } else props.onClose();
      }}
    >
      <anchored
        side={props.site === 'composer' ? 'top' : 'bottom'}
        align="start"
        gap={4}
        fit="switch"
        snapMargin={8}
        deferred
        priority={3}
        occlude
      >
        <div
          role="listbox"
          aria-label="Projects"
          onKeyDown={keys}
          onMouseDownOutside={(event) => {
            if (event.button === 0) props.onClose();
          }}
          style={{
            width: 300,
            display: 'flex',
            flexDirection: 'column',
            backgroundColor: colors.raised,
            borderRadius: 13,
            borderWidth: 1,
            borderColor: colors.border_strong,
            overflow: 'hidden',
            paddingBottom: 4,
            boxShadow: {
              offsetX: 0,
              offsetY: 10,
              blurRadius: 20,
              spreadRadius: 0,
              color: '#00000033',
            },
          }}
        >
          <div style={{ paddingTop: 10, paddingLeft: 12, paddingRight: 12, paddingBottom: 8 }}>
            <TextField
              neutralFocus
              ref={input}
              value={query}
              onSubmit={() => choose(cursor < 0 ? 0 : cursor)}
              placeholder="Search projects"
              icon="search"
              iconSize={12}
              onChange={(value) => {
                setQuery(value);
                setCursor(value.trim() ? 0 : -1);
              }}
              style={{
                height: 28,
                borderRadius: 9,
                backgroundColor: colors.surface,
                paddingLeft: 10,
                paddingRight: 10,
                gap: 8,
              }}
            />
          </div>
          <div style={{ position: 'relative' }}>
            <div
              ref={list}
              onScrollChange={(event) => setOffset(-(event.scrollY ?? 0))}
              style={{
                height: projects.length ? Math.min(286, projects.length * 26) : 64,
                overflowY: 'scroll',
                display: 'flex',
                flexDirection: 'column',
              }}
            >
              {projects.length ? (
                projects.map((project, index) => (
                  <PickerRow
                    key={project.id}
                    rowRef={(instance) => {
                      if (instance) rows.current.set(index, instance);
                      else rows.current.delete(index);
                    }}
                    label={project.name}
                    checked={project.id === props.currentProjectId}
                    highlighted={cursor === index}
                    onSelect={() => choose(index)}
                    onHover={() => setCursor(index)}
                  />
                ))
              ) : (
                <PickerEmpty>No projects found</PickerEmpty>
              )}
            </div>
            <Scrollbar
              viewport={Math.min(286, projects.length * 26)}
              content={projects.length * 26}
              offset={offset}
              surface={colors.raised}
              edgeFades
              onScroll={(value) => {
                if (list.current) renderer?.scrollTo?.(list.current.id, 0, -value);
              }}
            />
          </div>
          <PickerSeparator />
          <PickerRow
            label="New project…"
            icon="folder-new"
            highlighted={cursor === projects.length}
            onSelect={() => choose(projects.length)}
            onHover={() => setCursor(projects.length)}
          />
          <PickerRow
            label="Don't work in a project"
            icon="x"
            checked={
              props.currentProjectId === null ||
              props.projects.find((p) => p.id === props.currentProjectId)?.projectless
            }
            highlighted={cursor === projects.length + 1}
            onSelect={() => choose(projects.length + 1)}
            onHover={() => setCursor(projects.length + 1)}
          />
        </div>
      </anchored>
    </DismissableLayer>
  );
}
export function ProjectSelector(props: ProjectSelectorProps) {
  const { colors, sp } = useTheme();
  const [open, setOpen] = useState(false);
  const trigger = useRef<PublicInstance>(null);
  const [triggerWidth, setTriggerWidth] = useState(0);
  const project = props.projects.find((item) => item.id === props.currentProjectId);
  const label =
    props.label ??
    (project?.projectless || props.currentProjectId === null
      ? 'No project'
      : (project?.name ?? 'choose a project'));
  return (
    <div style={{ position: 'relative' }}>
      {props.site === 'empty' ? (
        <Button
          ref={trigger}
          onLayout={(event) => setTriggerWidth(event.width ?? 0)}
          aria-expanded={open}
          onClick={() => setOpen(!open)}
          style={{
            borderRadius: 6,
            paddingLeft: 0,
            paddingRight: 0,
            position: 'relative',
            backgroundColor: open ? colors.overlay : undefined,
          }}
        >
          <text
            style={{
              ...ellipsis,
              fontSize: sp(20),
              lineHeight: sp(28),
              fontWeight: 500,
              color: colors.text,
            }}
          >
            {label}
          </text>
          {triggerWidth > 0 && (
            <svg
              source={`<svg xmlns="http://www.w3.org/2000/svg" width="${triggerWidth}" height="1"><path d="M0 .5H${triggerWidth}" stroke="black" stroke-dasharray="1 2"/></svg>`}
              style={{
                position: 'absolute',
                bottom: -3,
                width: triggerWidth,
                height: 1,
                color: colors.text_tertiary,
              }}
            />
          )}
        </Button>
      ) : (
        <MenuChip
          ref={trigger}
          label={label}
          icon="folder"
          chevron={false}
          open={open}
          onClick={() => setOpen(!open)}
        />
      )}
      {open && (
        <ProjectPicker {...props} triggerId={trigger.current?.id} onClose={() => setOpen(false)} />
      )}
    </div>
  );
}

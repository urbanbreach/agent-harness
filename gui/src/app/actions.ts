// App-wide commands shared by the command palette, keyboard shortcuts and menus.
// Surfaces register handlers for the commands they own; callers run them by id.
export type ActionId = string;

export type ActionInfo = {
  id: ActionId;
  /** i18n key of the visible label. */
  label: string;
  /** Linux shortcut as displayed, e.g. "Ctrl+Shift+B". */
  shortcut?: string;
  icon?: string;
  /** Hidden from the palette when false. */
  enabled?: () => boolean;
  run: (value?: string) => void;
};

const actions = new Map<ActionId, ActionInfo>();
const listeners = new Set<() => void>();

export function registerAction(info: ActionInfo): () => void {
  actions.set(info.id, info);
  listeners.forEach((listener) => listener());
  return () => {
    if (actions.get(info.id) === info) actions.delete(info.id);
    listeners.forEach((listener) => listener());
  };
}

export function runAction(id: ActionId, value?: string): boolean {
  const action = actions.get(id);
  if (!action || action.enabled?.() === false) return false;
  action.run(value);
  return true;
}

export function listActions(): ActionInfo[] {
  return [...actions.values()];
}

export function subscribeActions(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

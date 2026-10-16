export interface TaskSwitcherSnapshot {
  open: boolean;
  ids: readonly string[];
  highlightedId?: string;
}
export class TaskSwitcherController {
  private visited: string[] = [];
  private valid = new Set<string>();
  private selected?: string;
  private selectionChanged = false;
  private snapshot: TaskSwitcherSnapshot = { open: false, ids: [] };
  private listeners = new Set<() => void>();
  getSnapshot = () => this.snapshot;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private publish(snapshot: TaskSwitcherSnapshot) {
    this.snapshot = snapshot;
    for (const listener of this.listeners) listener();
  }
  /** Supply only started, surviving tasks. Restored history is not automatically visited. */
  reconcile(ids: readonly string[], selected?: string) {
    this.valid = new Set(ids);
    this.visited = this.visited.filter((id) => this.valid.has(id));
    if (this.snapshot.open && selected !== this.selected) this.selectionChanged = true;
    this.selected = selected;
    if (selected && this.valid.has(selected)) this.visit(selected);
    if (!this.snapshot.open) return;
    const previous = this.snapshot.ids;
    const remaining = previous.filter((id) => this.valid.has(id));
    if (!remaining.length) {
      this.cancel();
      return;
    }
    if (remaining.length !== previous.length) {
      const index = Math.max(0, previous.indexOf(this.snapshot.highlightedId ?? ''));
      this.publish({
        open: true,
        ids: remaining,
        highlightedId: remaining.includes(this.snapshot.highlightedId ?? '')
          ? this.snapshot.highlightedId
          : remaining[Math.min(index, remaining.length - 1)],
      });
    }
  }
  visit(id: string) {
    if (!this.valid.has(id)) return;
    this.visited = [id, ...this.visited.filter((other) => other !== id)].slice(0, 10);
  }
  open(reverse = false) {
    if (this.snapshot.open) {
      this.cycle(reverse ? -1 : 1);
      return;
    }
    const ids = [
      ...new Set([
        ...(this.selected && this.valid.has(this.selected) ? [this.selected] : []),
        ...this.visited,
      ]),
    ].slice(0, 10);
    if (!ids.length) return;
    this.selectionChanged = false;
    this.publish({
      open: true,
      ids,
      highlightedId: ids[reverse ? ids.length - 1 : ids.length > 1 ? 1 : 0],
    });
  }
  cycle(direction: number) {
    if (!this.snapshot.open) return;
    const { ids, highlightedId } = this.snapshot;
    const index = ids.indexOf(highlightedId ?? '');
    this.highlight(ids[(index + (direction % ids.length) + ids.length) % ids.length]!);
  }
  home() {
    if (this.snapshot.open) this.highlight(this.snapshot.ids[0]!);
  }
  end() {
    if (this.snapshot.open) this.highlight(this.snapshot.ids.at(-1)!);
  }
  highlight(id: string) {
    if (this.snapshot.open && this.snapshot.ids.includes(id) && id !== this.snapshot.highlightedId)
      this.publish({ ...this.snapshot, highlightedId: id });
  }
  /** Returns an activation id exactly once. Explicit clicks may override external navigation. */
  commit(explicitId?: string): string | undefined {
    if (!this.snapshot.open) return;
    const id = explicitId ?? this.snapshot.highlightedId;
    const allowed =
      id &&
      this.valid.has(id) &&
      this.snapshot.ids.includes(id) &&
      (explicitId !== undefined || !this.selectionChanged);
    this.cancel();
    return allowed ? id : undefined;
  }
  cancel() {
    if (this.snapshot.open) this.publish({ open: false, ids: [] });
  }
}
export const createTaskSwitcherController = () => new TaskSwitcherController();

export interface WindowGeometry {
  x: number;
  y: number;
  width: number;
  height: number;
  maximized: boolean;
  displayId?: string | null;
}

export function restoreWindowGeometry(saved: WindowGeometry | undefined) {
  if (!saved || ![saved.x, saved.y, saved.width, saved.height].every(Number.isFinite))
    return { width: 1380, height: 880 };
  return {
    x: saved.x,
    y: saved.y,
    width: Math.max(980, saved.width),
    height: Math.max(680, saved.height),
    maximized: saved.maximized === true,
    displayId: typeof saved.displayId === 'string' ? saved.displayId : undefined,
  };
}

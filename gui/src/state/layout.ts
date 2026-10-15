import { useEffect, useRef, useState } from 'react';
import { memoryPersist, type Persist } from './persist';

export interface PanelLayout {
  sidebarWidth: number;
  rightWidth: number;
  sidebarVisible: boolean;
  rightVisible: boolean;
}
export const defaultLayout: PanelLayout = {
  sidebarWidth: 252,
  rightWidth: 460,
  sidebarVisible: true,
  rightVisible: false,
};

export function fitPanels(viewport: number, layout: PanelLayout) {
  let sidebar = layout.sidebarVisible ? layout.sidebarWidth : 0;
  let right = layout.rightVisible ? layout.rightWidth : 0;
  let excess = Math.max(0, sidebar + right + 360 - viewport);
  const rightReduction = Math.min(excess, Math.max(0, right - 280));
  right -= rightReduction;
  excess -= rightReduction;
  const sidebarReduction = Math.min(excess, Math.max(0, sidebar - 180));
  sidebar -= sidebarReduction;
  excess -= sidebarReduction;
  const finalRightReduction = Math.min(excess, right);
  right -= finalRightReduction;
  excess -= finalRightReduction;
  sidebar = Math.max(0, sidebar - excess);
  return { sidebar, right };
}

export function usePanelLayout(
  persist: Persist = memoryPersist,
  onError?: (error: unknown) => void,
  dragging = false,
) {
  const errorHandler = useRef(onError);
  errorHandler.current = onError;
  const [layout, setLayout] = useState(defaultLayout);
  const [loaded, setLoaded] = useState(false);
  useEffect(() => {
    let active = true;
    void persist
      .read<PanelLayout>('window.panels')
      .then((saved) => {
        if (!active) return;
        if (saved)
          setLayout({
            sidebarWidth: Number.isFinite(saved.sidebarWidth)
              ? Math.min(420, Math.max(180, saved.sidebarWidth))
              : 252,
            rightWidth: Number.isFinite(saved.rightWidth)
              ? Math.min(1000, Math.max(280, saved.rightWidth))
              : 460,
            sidebarVisible: saved.sidebarVisible !== false,
            rightVisible: saved.rightVisible === true,
          });
        setLoaded(true);
      })
      .catch((error) => {
        if (active) {
          errorHandler.current?.(error);
          setLoaded(true);
        }
      });
    return () => {
      active = false;
    };
  }, [persist]);
  useEffect(() => {
    if (loaded && !dragging)
      void persist.write('window.panels', layout).catch((error) => errorHandler.current?.(error));
  }, [layout, loaded, persist, dragging]);
  return [layout, setLayout] as const;
}

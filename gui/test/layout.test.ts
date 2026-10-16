import { expect, test } from 'bun:test';
import { fitPanels, type PanelLayout } from '../src/state/layout';
import { restoreWindowGeometry } from '../src/state/window';

test('panel fitting sacrifices the right panel first while preserving preferred widths', () => {
  const preferred: PanelLayout = {
    sidebarWidth: 420,
    rightWidth: 720,
    sidebarVisible: true,
    rightVisible: true,
  };
  expect(fitPanels(980, preferred)).toEqual({ sidebar: 340, right: 280 });
  expect(fitPanels(600, preferred)).toEqual({ sidebar: 180, right: 60 });
  expect(fitPanels(400, preferred)).toEqual({ sidebar: 40, right: 0 });
  expect(fitPanels(300, preferred)).toEqual({ sidebar: 0, right: 0 });
  expect(preferred).toEqual({
    sidebarWidth: 420,
    rightWidth: 720,
    sidebarVisible: true,
    rightVisible: true,
  });
});

test('window restoration rejects corrupt rectangles and enforces the native minimum', () => {
  expect(
    restoreWindowGeometry({ x: NaN, y: 1, width: 1380, height: 880, maximized: false }),
  ).toEqual({ width: 1380, height: 880 });
  expect(
    restoreWindowGeometry({
      x: -20,
      y: 5,
      width: 400,
      height: 200,
      maximized: true,
      displayId: 'screen-2',
    }),
  ).toEqual({ x: -20, y: 5, width: 980, height: 680, maximized: true, displayId: 'screen-2' });
});

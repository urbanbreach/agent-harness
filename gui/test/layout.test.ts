import { expect, test } from 'bun:test';
import { fitPanels, type PanelLayout } from '../src/state/layout';

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

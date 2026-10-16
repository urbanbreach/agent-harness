import { useGpuixRequired, type EventPayload, type NativeRenderer, type Props } from '@gpuix/react';
import type { Buffer } from 'node:buffer';

export interface Run {
  /** UTF-16 offsets into the element's text. Later overlapping runs override earlier fields. */
  start: number;
  end: number;
  color?: string;
  fontFamily?: string;
  fontWeight?: string | number;
  italic?: boolean;
  underline?: { color?: string; thickness?: number; wavy?: boolean };
  strikethrough?: { color?: string; thickness?: number };
  background?: string;
  wash?: { color: string; radius?: number; padX?: number; insetY?: number };
}
export interface NativeEvent extends EventPayload {
  charIndex: number | null;
  selectionStart?: number;
  selectionEnd?: number;
}
export interface RichTextProps extends Omit<Props, 'onClick' | 'onMouseDown' | 'onMouseMove'> {
  text: string;
  runs?: Run[];
  onClick?: (event: NativeEvent) => void;
  onMouseDown?: (event: NativeEvent) => void;
  onMouseMove?: (event: NativeEvent) => void;
}
/** Additional native editor theme tokens. */
export interface InputTheme {
  selectionWash?: string;
  placeholder?: string;
  caretWidth?: number;
}
export interface InputHighlight {
  start: number;
  end: number;
  color: string;
  active?: boolean;
  activeColor?: string;
}
export interface InputGutter {
  color: string;
  activeColor?: string;
  width?: number;
  gap?: number;
  fontSize?: number;
}
export type ResizeEdge = 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw';
export type InputCommand = 'cut' | 'copy' | 'paste' | 'selectAll' | 'undo' | 'redo';
export interface WindowDecorations {
  kind: 'server' | 'client';
  tiling: { top: boolean; left: boolean; right: boolean; bottom: boolean };
}
export interface WindowControls {
  fullscreen: boolean;
  maximize: boolean;
  minimize: boolean;
  windowMenu: boolean;
}
export interface HarnessNativeRenderer extends NativeRenderer {
  inputSetSelection(elementId: number, start: number, end: number): void;
  inputReplaceSelection(elementId: number, text: string): void;
  inputCommand(elementId: number, name: InputCommand): void;
  inputRevealRange(elementId: number, start: number, end: number): void;
  inputScrollTo(elementId: number, x: number, y: number): void;
  writeClipboardText(text: string): void;
  readClipboardText(): string | null;
  readClipboardImage(): { mime: string; data: Buffer } | null;
  readClipboardPaths(): string[] | null;
  /** napi callbacks are error-first; null unregisters the listener. */
  setInputSelectionListener(
    elementId: number,
    callback: ((error: Error | null, event: NativeEvent) => void) | null,
  ): void;
  windowDecorations(): WindowDecorations;
  windowBounds(): {
    x: number;
    y: number;
    width: number;
    height: number;
    maximized: boolean;
    fullscreen: boolean;
    displayId?: string | null;
  };
  closeWindow(): void;
  requestClientDecorations(client: boolean): void;
  startWindowMove(): void;
  startWindowResize(edge: ResizeEdge): void;
  /** Poll getters to observe compositor changes without modifying the JS renderer. */
  isWindowMaximized(): boolean;
  windowControls(): WindowControls;
  isWindowActive(): boolean;
  windowAppearance(): 'light' | 'dark';
  registerFont(data: Uint8Array): void;
  /** Native chrome override is macOS-only in pinned GPUI; Linux follows its compositor. */
  setWindowAppearance(appearance: 'light' | 'dark' | 'system'): void;
  setRemSize(size: number): void;
}
/** useGpuixRequired() and createRenderer() expose the native napi instance; refs expose numeric .id. */
export function nativeRenderer(renderer: NativeRenderer): HarnessNativeRenderer {
  return renderer as HarnessNativeRenderer;
}
export function useNativeRenderer(): HarnessNativeRenderer {
  return nativeRenderer(useGpuixRequired());
}
export const inputSetSelection = (
  renderer: NativeRenderer,
  id: number,
  start: number,
  end: number,
): void => nativeRenderer(renderer).inputSetSelection(id, start, end);
export const inputReplaceSelection = (renderer: NativeRenderer, id: number, text: string): void =>
  nativeRenderer(renderer).inputReplaceSelection(id, text);
export const inputCommand = (renderer: NativeRenderer, id: number, name: InputCommand): void =>
  nativeRenderer(renderer).inputCommand(id, name);
export const inputRevealRange = (
  renderer: NativeRenderer,
  id: number,
  start: number,
  end: number,
): void => nativeRenderer(renderer).inputRevealRange(id, start, end);
export const inputScrollTo = (renderer: NativeRenderer, id: number, x: number, y: number): void =>
  nativeRenderer(renderer).inputScrollTo(id, x, y);
export const writeClipboardText = (renderer: NativeRenderer, text: string): void =>
  nativeRenderer(renderer).writeClipboardText(text);
export const readClipboardText = (renderer: NativeRenderer): string | null =>
  nativeRenderer(renderer).readClipboardText();
export const readClipboardImage = (
  renderer: NativeRenderer,
): { mime: string; data: Buffer } | null => nativeRenderer(renderer).readClipboardImage();
export const windowDecorations = (renderer: NativeRenderer): WindowDecorations =>
  nativeRenderer(renderer).windowDecorations();
export const requestClientDecorations = (renderer: NativeRenderer, client: boolean): void =>
  nativeRenderer(renderer).requestClientDecorations(client);
export const startWindowMove = (renderer: NativeRenderer): void =>
  nativeRenderer(renderer).startWindowMove();
export const setInputSelectionListener = (
  renderer: NativeRenderer,
  id: number,
  callback: ((error: Error | null, event: NativeEvent) => void) | null,
): void => nativeRenderer(renderer).setInputSelectionListener(id, callback);
export const startWindowResize = (renderer: NativeRenderer, edge: ResizeEdge): void =>
  nativeRenderer(renderer).startWindowResize(edge);
export const isWindowMaximized = (renderer: NativeRenderer): boolean =>
  nativeRenderer(renderer).isWindowMaximized();
export const windowControls = (renderer: NativeRenderer): WindowControls =>
  nativeRenderer(renderer).windowControls();
export const isWindowActive = (renderer: NativeRenderer): boolean =>
  nativeRenderer(renderer).isWindowActive();
export const windowAppearance = (renderer: NativeRenderer): 'light' | 'dark' =>
  nativeRenderer(renderer).windowAppearance();
export const registerFont = (renderer: NativeRenderer, bytes: Uint8Array): void =>
  nativeRenderer(renderer).registerFont(bytes);
export const setWindowAppearance = (
  renderer: NativeRenderer,
  appearance: 'light' | 'dark' | 'system',
): void => nativeRenderer(renderer).setWindowAppearance(appearance);
export const setRemSize = (renderer: NativeRenderer, size: number): void =>
  nativeRenderer(renderer).setRemSize(size);

declare module '@gpuix/react/jsx-runtime' {
  namespace JSX {
    interface IntrinsicElements {
      'rich-text': RichTextProps;
    }
  }
}

import React, { useEffect, useRef, useState } from 'react';
import {
  render,
  createRenderer,
  startFrameLoop,
  type PublicInstance,
  type GpuixTheme,
} from '@gpuix/react';
import { readFileSync } from 'node:fs';
import {
  nativeRenderer,
  useNativeRenderer,
  type NativeEvent,
  type Run,
  type InputCommand,
  type ResizeEdge,
} from '../../src/native';

const paragraph =
  'Bold text, italic text, and inline_code_with_a_rounded_wash that wraps naturally across visual lines. A link is underlined; obsolete text is struck through. Unicode 😀 remains UTF-16 indexed. Repeat inline_code_with_a_rounded_wash across another line.';
const span = (part: string, fields: Omit<Run, 'start' | 'end'>): Run => ({
  start: paragraph.indexOf(part),
  end: paragraph.indexOf(part) + part.length,
  ...fields,
});
const runs: Run[] = [
  span('Bold text', { fontWeight: 700 }),
  span('italic text', { italic: true }),
  span('inline_code_with_a_rounded_wash that wraps naturally across visual lines.', {
    fontFamily: 'JetBrains Mono',
    wash: { color: '#303643', padX: 3, insetY: 3, radius: 4 },
  }),
  span('A link', { color: '#8daeff', underline: { color: '#8daeff', thickness: 1 } }),
  span('obsolete text', { strikethrough: { thickness: 1 } }),
];
const buttonStyle = {
  padding: 9,
  backgroundColor: '#303643',
  borderRadius: 5,
  cursor: 'pointer' as const,
};
function Smoke() {
  const native = useNativeRenderer();
  const input = useRef<PublicInstance>(null);
  const [selection, setSelection] = useState('0–0');
  const [status, setStatus] = useState('Ready');
  const [linkHover, setLinkHover] = useState(false);
  function report(name: string, result: unknown) {
    console.log(name, JSON.stringify(result));
    setStatus(`${name}: ${JSON.stringify(result)}`);
  }
  function command(name: InputCommand) {
    if (input.current) {
      native.inputCommand(input.current.id, name);
      report(name, 'ok');
    }
  }
  useEffect(() => {
    report('window', {
      decorations: native.windowDecorations(),
      controls: native.windowControls(),
      active: native.isWindowActive(),
      appearance: native.windowAppearance(),
      maximized: native.isWindowMaximized(),
    });
    native.writeClipboardText('Harness native clipboard 😀');
    console.log('clipboardText', native.readClipboardText());
    const image = native.readClipboardImage();
    console.log('clipboardImage', image ? { mime: image.mime, bytes: image.data.length } : null);
    if (input.current) {
      native.setInputSelectionListener(input.current.id, (error, event) => {
        if (error) {
          console.error(error);
          return;
        }
        setSelection(`${event.selectionStart}–${event.selectionEnd}`);
        console.log('selectionChange', JSON.stringify(event));
      });
      native.inputSetSelection(input.current.id, 0, 7);
      native.inputReplaceSelection(input.current.id, 'Harness');
      native.inputSetSelection(input.current.id, 0, 7);
    }
    const timer = setInterval(
      () =>
        console.log('windowPoll', {
          active: native.isWindowActive(),
          appearance: native.windowAppearance(),
          maximized: native.isWindowMaximized(),
        }),
      5000,
    );
    return () => clearInterval(timer);
  }, [native]);
  const updateSelection = (event: NativeEvent) => {
    console.log('inputEvent', JSON.stringify(event));
  };
  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        padding: 32,
        backgroundColor: '#181b21',
        color: '#e8eaf0',
        fontFamily: 'Liberation Sans',
        fontSize: 15,
        display: 'flex',
        flexDirection: 'column',
        gap: 18,
      }}
    >
      <text style={{ fontSize: 24, fontWeight: 700 }}>Harness native capability smoke</text>
      <rich-text
        text={paragraph}
        runs={runs}
        highlight={{ query: 'Unicode', color: '#7b602a', radius: 3 }}
        style={{ width: 520, lineHeight: 27, fontSize: 17, cursor: linkHover ? 'pointer' : 'text' }}
        onClick={(event) => report('richClick UTF-16', event.charIndex)}
        onMouseDown={(event) => console.log('richDown', event.charIndex)}
        onMouseMove={(event) =>
          setLinkHover(
            event.charIndex != null &&
              event.charIndex >= paragraph.indexOf('A link') &&
              event.charIndex < paragraph.indexOf('A link') + 6,
          )
        }
      />
      <text>Editor selection (UTF-16): {selection}</text>
      <textarea
        ref={input}
        value={'Harness editor 😀\nSelect text or move the caret. Commands preserve native undo.'}
        onChange={updateSelection}
        theme={{ selectionWash: '#5d72bd80', placeholder: '#969baa', caretWidth: 2 } as GpuixTheme}
        style={{ width: 650, height: 125, fontSize: 16 }}
      />
      <div style={{ display: 'flex', gap: 8 }}>
        {(['cut', 'copy', 'paste', 'selectAll', 'undo', 'redo'] as const).map((name) => (
          <div key={name} style={buttonStyle} onClick={() => command(name)}>
            <text>{name}</text>
          </div>
        ))}
      </div>
      <div style={{ display: 'flex', gap: 8 }}>
        <div
          style={buttonStyle}
          onClick={() => report('clipboardText', native.readClipboardText())}
        >
          <text>Read clipboard</text>
        </div>
        <div
          style={buttonStyle}
          onClick={() => {
            const image = native.readClipboardImage();
            report('clipboardImage', image ? { mime: image.mime, bytes: image.data.length } : null);
          }}
        >
          <text>Read image</text>
        </div>
        <div
          style={buttonStyle}
          onClick={() => {
            native.requestClientDecorations(true);
            report('decorations', native.windowDecorations());
          }}
        >
          <text>Client chrome</text>
        </div>
        <div
          style={buttonStyle}
          onClick={() => {
            native.requestClientDecorations(false);
            report('decorations', native.windowDecorations());
          }}
        >
          <text>Server chrome</text>
        </div>
        <div
          style={buttonStyle}
          onMouseDown={() => {
            native.startWindowMove();
            report('startWindowMove', 'requested');
          }}
        >
          <text>Drag window</text>
        </div>
      </div>
      <div style={{ display: 'flex', gap: 8 }}>
        {(['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'] as ResizeEdge[]).map((edge) => (
          <div
            key={edge}
            style={buttonStyle}
            onMouseDown={() => {
              native.startWindowResize(edge);
              report('startWindowResize', edge);
            }}
          >
            <text>Resize {edge}</text>
          </div>
        ))}
      </div>
      <text>{status}</text>
    </div>
  );
}
const renderer = createRenderer();
renderer.init({ title: 'Harness native smoke', width: 1100, height: 800 });
for (const face of ['Regular', 'Bold', 'Italic', 'BoldItalic']) {
  nativeRenderer(renderer).registerFont(
    readFileSync(new URL(`../../assets/fonts/JetBrainsMono-${face}.ttf`, import.meta.url)),
  );
}
startFrameLoop(renderer, { onTerminated: () => process.exit(0) });
render(<Smoke />, { renderer });

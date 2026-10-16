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
  const [code, setCode] = useState(
    'const greeting = "Harness 😀";\nfunction welcome(name: string) {\n  return `${greeting} ${name}`; // A deliberately long logical line demonstrates wrapped rows without extra gutter numbers.\n}\nwelcome("native editor");',
  );
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
      bounds: native.windowBounds(),
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
      <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        <div copySeparator={'\n\n'}>
          <text>Copy separator: first markdown paragraph.</text>
        </div>
        <div copySeparator={'\n\n'}>
          <text>Second paragraph copies after a blank line.</text>
        </div>
      </div>
      <text>Editor selection (UTF-16): {selection}</text>
      <textarea
        ref={input}
        value={code}
        onChange={(event) => {
          setCode(event.value ?? '');
          updateSelection(event);
        }}
        runs={[
          { start: 0, end: 5, color: '#c792ea', fontWeight: 700 },
          {
            start: code.indexOf('"'),
            end: code.indexOf('"', code.indexOf('"') + 1) + 1,
            color: '#c3e88d',
          },
        ]}
        highlights={[
          {
            start: code.indexOf('greeting'),
            end: code.indexOf('greeting') + 8,
            color: '#79632380',
            active: true,
            activeColor: '#b68a3280',
          },
        ]}
        gutter={{ color: '#727b8e', activeColor: '#e8eaf0', width: 32, gap: 16, fontSize: 13 }}
        wrap
        minRows={6}
        maxRows={6}
        theme={{ selectionWash: '#5d72bd80', placeholder: '#969baa', caretWidth: 2 } as GpuixTheme}
        style={{ width: 650, fontSize: 14, fontFamily: 'JetBrains Mono', lineHeight: 22 }}
      />
      <input secure value="demo-secret-é😀" style={{ width: 300, fontSize: 16 }} />
      <div style={{ display: 'flex', gap: 8 }}>
        <div
          style={buttonStyle}
          onClick={() =>
            input.current &&
            native.inputRevealRange(input.current.id, code.length - 10, code.length)
          }
        >
          <text>Reveal last range</text>
        </div>
        <div
          style={buttonStyle}
          onClick={() => input.current && native.inputScrollTo(input.current.id, 0, 0)}
        >
          <text>Scroll to top</text>
        </div>
      </div>
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
function NestedScrollSmoke() {
  const native = useNativeRenderer();
  const list = useRef<PublicInstance>(null);
  const inner = useRef<PublicInstance>(null);
  const [revision, setRevision] = useState(0);
  const [offsets, setOffsets] = useState('');
  function reportOffsets() {
    const values = {
      inner: inner.current && native.getScrollOffset(inner.current.id),
      list: list.current && native.getScrollOffset(list.current.id),
      revision,
    };
    console.log('nestedScroll', JSON.stringify(values));
    setOffsets(JSON.stringify(values));
  }
  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        padding: 32,
        backgroundColor: '#181b21',
        color: '#e8eaf0',
        display: 'flex',
        flexDirection: 'column',
        gap: 16,
      }}
    >
      <text style={{ fontSize: 24 }}>Nested scroll in virtual rows</text>
      <div style={{ display: 'flex', gap: 12 }}>
        <div style={buttonStyle} onClick={() => setRevision((value) => value + 1)}>
          <text>Rebuild row</text>
        </div>
        <div style={buttonStyle} onClick={reportOffsets}>
          <text>Report offsets</text>
        </div>
      </div>
      <text>{offsets}</text>
      <virtual-list ref={list} style={{ width: 700, height: 440 }}>
        {Array.from({ length: 12 }, (_, index) => (
          <div
            key={index}
            style={{ width: '100%', padding: 12, display: 'flex', flexDirection: 'column', gap: 8 }}
          >
            <text>{`Virtual row ${index}, revision ${revision}`}</text>
            {index === 0 ? (
              <div
                ref={inner}
                style={{ height: 160, width: 620, overflow: 'scroll', backgroundColor: '#303643' }}
              >
                {Array.from({ length: 24 }, (_, line) => (
                  <text key={line} style={{ height: 24 }}>
                    {`Inner line ${line + 1}`}
                  </text>
                ))}
              </div>
            ) : (
              <text style={{ height: 70 }}>{`Outer content ${index}`}</text>
            )}
          </div>
        ))}
      </virtual-list>
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
render(process.env.HARNESS_NATIVE_SMOKE === 'nested-scroll' ? <NestedScrollSmoke /> : <Smoke />, {
  renderer,
});

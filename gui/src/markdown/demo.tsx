import React, { useEffect } from 'react';
import { render, useWindowSize } from '@gpuix/react';
import { readFileSync, existsSync } from 'node:fs';
import { Markdown } from './index';
import { ThemeProvider, useTheme } from '../theme';
import { TooltipProvider } from '@gpuix/react';
const reference =
  '/home/urbanbreach/Projects/agent-harness/gui-inspirations/captures/content/markdown-sample.txt';
const extended = `# Rich text specimen

Soft source break
and a hard break.  
Both retain their line boundaries.

**Semibold**, *italic*, ***combined***, ~~strike~~ and \`inline code\`.
Links: HTTPS://example.com/a_(balanced). [Guide](https://example.com).

> A quote with ordinary text.
>
> - A list inside a quote
>   - Nested content

7. Ordered start
8. Second item

- [x] Finished task
- [ ] Open task

| Left | Center | Right |
| :--- | :---: | ---: |
| short | **centered** | 120 |
| a longer column with wrapped prose | \`code\` | 42 |

\`\`\`typescript
// Syntax colors preserve geometry
interface Release { name: string; ready: boolean }
const release: Release = { name: "Atlas", ready: true };
console.log(release.name, 42);
\`\`\`

Inline math $x^2 + y^2$ and $\\frac{a}{b}$ stay distinct.

$$
\\int_0^1 x^2\\,dx = \\frac{1}{3}
$$

<img src="not-executed"> remains literal.

---

![Small local sample](data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIxNjAiIGhlaWdodD0iNDAiPjxyZWN0IHdpZHRoPSIxNjAiIGhlaWdodD0iNDAiIGZpbGw9IiNDODVGNDQiLz48L3N2Zz4=)
`;
const source = existsSync(reference) ? readFileSync(reference, 'utf8') : extended;
function Column({ appearance }: { appearance: 'light' | 'dark' }) {
  const theme = useTheme();
  useEffect(() => theme.setPreference(appearance), [appearance, theme.setPreference]);
  const referenceOnly = process.env.MD_REFERENCE === '1';
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: referenceOnly ? 718 : 640,
        flexShrink: 0,
        padding: referenceOnly ? 0 : 20,
        gap: 14,
        backgroundColor: theme.colors.surface,
        color: theme.colors.text,
      }}
    >
      {!referenceOnly && (
        <text
          style={{
            fontSize: 12.5,
            color: theme.colors.text_ghost,
            userSelect: 'none',
            whiteSpace: 'nowrap',
          }}
        >
          {appearance === 'light' ? 'Light' : 'Dark'}
        </text>
      )}
      <Markdown source={source} />
      {!referenceOnly && <Markdown source={extended} />}
    </div>
  );
}
function Demo() {
  const { height } = useWindowSize();
  const referenceOnly = process.env.MD_REFERENCE === '1';
  return (
    <TooltipProvider>
      <div
        style={{
          display: 'flex',
          flexDirection: 'row',
          width: '100%',
          height: height || 880,
          overflowY: 'scroll',
          alignItems: 'flex-start',
          paddingLeft: referenceOnly ? 458 : 0,
          backgroundColor: '#1A1A1A',
          gap: referenceOnly ? 0 : 20,
        }}
      >
        {(referenceOnly ? (['dark'] as const) : (['light', 'dark'] as const)).map((appearance) => (
          <ThemeProvider key={appearance}>
            <Column appearance={appearance} />
          </ThemeProvider>
        ))}
      </div>
    </TooltipProvider>
  );
}
render(<Demo />, { title: 'Harness rich text', width: 1380, height: 880 });

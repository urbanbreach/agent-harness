# Render and capture metadata

- Source: `grok-build-harness-parity-audit.md`
- Renderer: `marked@16.3.0` via `npx`
- Rendered artifact: `report.html`
- Browser: system Chromium `/usr/bin/chromium`
- Required viewport load: `1440x1000`, exit 0, dumped DOM size 31,479 bytes
- Full-page capture: `report.png`
- Final browser canvas: `1440x20000`
- Final content bounds measured from the composited PNG: 11,608 pixels high
- Fresh final review capture dimensions: `1440x12050` (full content plus bottom padding)
- Capture signature: PNG, 8-bit RGB, non-interlaced
- Browser capture command exited 0.
- Visual QA: independent dual review pending.

The capture uses the same 1440-pixel width as the required viewport and
contains the complete scrolling document. Excess browser-canvas background
below the document was removed after measuring the rendered content bounds.
The HTML was independently loaded at the required 1440x1000 viewport before
review. Reviewers launched against stale or oversized captures were cancelled;
only fresh reviewers of the current post-proofread capture may approve.

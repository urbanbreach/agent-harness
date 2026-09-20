# Settings and modal polish against Grok Build

This follow-up preserves Harness's themes, semantic colors, name, and artwork.
It builds on the [earlier UI pass](ui-polish-2026-09-15.md), using existing
editors, modal geometry, glyphs, and validation. No dependencies were added.

The comparison covered startup, composer, settings, dashboard, tool disclosure,
and activity animations. The remaining actionable gaps were concentrated in
settings and modal input ownership.

| Gap | Implemented behavior |
| --- | --- |
| Hidden choice options | All choices are visible; clicking selects a draft and Enter saves it. |
| Unclear text editing | Visible caret, horizontal scrolling, select-all highlighting, and readable validation errors. |
| Paste reached the composer behind a modal | Paste follows the active overlay; settings receive search or field input. Choice drafts ignore pasted text. |
| Hard-to-discover navigation | Clickable tabs/search, selected-tab color, Home/End and paging, and adaptive keyboard hints. |
| Empty search could act on a hidden row | No matches means no editable selection. |
| Compact layouts and Unicode | Readable setting names, grapheme-safe deletion/viewports, and a shared zero-height scrollbar guard. |

The official [Grok Build source](https://github.com/xai-org/grok-build) was
reviewed at commit `37949780c144e37df692e3d669051a21fec24f20`
(upstream `SOURCE_REV` `c4ea71cfdbcdb21e32e41bc25a0043d7d4836714`). The gallery's
recorded Grok settings image uses the earlier production renderer with
`SOURCE_REV` `eb4a894da8fb7bcd8d8f398a9d909a7868a4fcf1`; its provenance is included.
This is an interaction comparison, not a claim of whole-application pixel identity.
Service-dependent differences remain in the existing
[authority mapping](grok-build-parity-implementation.md#authority-mapping-and-comparison-limits).

## Verification

Open the xterm.js before/after gallery (`.omo/evidence/ui-refinement-20260915/index.html`).
It includes editable states, compact screens, the live journey, animation samples,
reference revisions, and source/binary receipts. Artifacts are local and ignored.

| Check | Result |
| --- | --- |
| Workspace nextest, all features | 4,618 passed; 11 configured skips |
| Workspace Clippy, all targets/features, warnings denied | Passed |
| Canonical simulation lane | All seven stages passed, including repeatability, replay, and secret scanning |
| Focused production-renderer capture tests | Eight passed |
| xterm.js production-renderer matrix | 405 final frames: settings, dashboard, tools, composer, chat, and startup |
| Reduced-motion transcript comparison | 42 scene/size cases remain cell-identical across 0/330/660 ms; elapsed-time status text continues updating |
| Live PTY in xterm.js | Six interaction checks and 11 screenshots; tab clicks, choice drafts/saves, validation, bracketed paste, Unicode, resizing, and composer preservation |
| Browser QA driver tests | 53 passed |
| Formatting, whitespace, static test-suite gates | Passed |

The full suite and live journey use a clean verification checkout with the shipped
example configuration. The user's existing `harness.jsonc` and untracked artifacts
were preserved. The live journey uses a deterministic HTTP provider fixture bound
to loopback and the actual Harness binary; it verifies saved configuration values
and cleans up the provider, PTY, and browser.

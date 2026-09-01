# Research brief: Harness visual parity with Grok Build

## Core question

What concrete visual, spatial, interaction, motion, architecture, and verification differences prevent Harness from feeling 1:1 with Grok Build, and what exact implementation sequence should close those gaps while preserving Harness branding and logos?

## Axes

1. Application shell and spatial hierarchy.
2. Sidebar, navigation, session switching, and wayfinding.
3. Transcript and message rendering.
4. Composer, input, autocomplete, and command surfaces.
5. Tool/task/activity/status presentation.
6. Overlays, dialogs, palettes, inspectors, and drawers.
7. Typography, color, borders, shadows, materials, and density.
8. Motion, transitions, feedback, scrolling, and perceived fluidity.
9. Responsive behavior, terminal geometry, and content stress.
10. Accessibility, focus, keyboard, mouse, and reduced-motion behavior.
11. Performance, rendering architecture, invalidation, and latency.
12. Component/primitives architecture and design-system governance.
13. Empty/loading/error/replay/offline states.
14. Branding, icons, logos, and asset treatment.
15. Testing, snapshots, PTY/browser evidence, and regression gates.
16. Migration sequencing, dependencies, effort, and implementation risk.

## Expected truths

- E1: Grok Build contains reusable visual/interaction patterns absent or materially weaker in Harness.
- E2: Harness has terminal-specific constraints that require faithful adaptation rather than direct web-code transplantation.
- E3: 1:1 parity can preserve Harness names, logos, and brand identity while adopting Grok Build's geometry, hierarchy, motion semantics, density, and finish.
- E4: The highest-leverage work can be ordered into implementable increments with objective visual and behavioral verification.

## Format answer

The user explicitly requested a Markdown file. Deliver one long-form implementation audit in English with an executive summary, prioritized findings, evidence tables, examples, detailed instructions, sequencing, and verification commands.

## Team roster

- shell-layout: application shell, panes, scroll ownership, responsive geometry.
- transcript: messages, code, markdown, tool/result rendering.
- composer-input: editor/composer, commands, autocomplete, input feedback.
- overlays-status: modal, palette, status, tasks, activity, notifications.
- theme-material: typography, palette, density, borders, surfaces, icons, branding.
- motion-fluidity: transitions, animation semantics, scrolling, perceived latency.
- verification-performance: architecture, rendering cost, tests, PTY/browser evidence.
- skeptic: attack evidence, framing, feasibility, and 1:1 claims.

## Scale and lifecycle

Sixteen axes across two substantial UI implementations. Use a 64-node opening collection DAG, at least two EXPAND waves, one cooperating debate team, parallel synthesis reducers, and final writing/visual gates.

# Harness Terminal Design System

## 1. Atmosphere & Identity

Harness is a quiet, terminal-native command center: dense enough for active agent work, calm enough for long sessions, and explicit about what the runtime is doing. Its signature is stateful chrome: the transcript remains primary while compact shell rows reveal focus, progress, safety state, and the next useful action. Harness keeps its own name, neutral surfaces, blue system accent, and event-sourced operator language. Grok Build is a reference for compact keyboard-first hierarchy and status legibility only; Harness does not adopt its name, logo, copy, or palette.

## 2. Color

The canonical implementation is `crates/harness-tui/src/theme_tokens.rs`; theme families resolve these roles rather than introducing screen-local colors.

| Role | Token | Usage |
|---|---|---|
| Canvas | `ColorRole::Canvas` | Transcript and footer background |
| Shell | `ColorRole::Shell` | Fixed shell chrome |
| Panel | `ColorRole::Panel` | Secondary surfaces |
| Elevated panel | `ColorRole::PanelElevated` | Composer and overlays |
| Primary text | `ColorRole::TextPrimary` | User content and active labels |
| Secondary text | `ColorRole::TextSecondary` | Metadata and ordinary status |
| Tertiary text | `ColorRole::TextTertiary` | Low-priority chrome |
| Accent | `ColorRole::TextAccent` | Focus, selected state, primary interaction |
| Success | `ColorRole::StatusSuccess` | Healthy execution and low context pressure |
| Warning | `ColorRole::StatusWarning` | Recoverable risk and high context pressure |
| Error | `ColorRole::StatusError` | Failure, destructive hover, critical context pressure |
| Info | `ColorRole::StatusInfo` | Background activity and neutral progress |

Rules:

- Accent color communicates interaction or focus, never decoration.
- Status colors always pair with text or a glyph; color alone never carries meaning.
- New colors must enter through a semantic `ColorRole`, not a renderer-local RGB value.
- Harness branding and theme-family contrast take precedence over external visual references.

## 3. Typography

Terminal font choice belongs to the operator. Harness creates hierarchy with semantic color, modifiers, spacing, and concise copy.

| Level | Treatment | Usage |
|---|---|---|
| Primary | normal or bold, primary text | Prompt, transcript, selected actions |
| Secondary | normal, secondary text | Status labels and metadata |
| Tertiary | dim, tertiary text | Optional hints and inactive chrome |
| Accent | bold, accent text | Focus marker and active choice |
| Error | error text, optional bold | Blocking failures and destructive action |

Rules:

- Sentence case is canonical; avoid title case in transient status copy.
- Numeric status uses compact, stable-width notation where practical.
- Labels describe the current activity directly: `Thinking…`, `Run bash`, `Waiting on subagent…`.
- Keep status fragments short enough to degrade cleanly at 80 columns.

## 4. Spacing & Layout

The base unit is one terminal cell. Geometry is owned by `layout.rs`, `responsive.rs`, `shell_geometry/`, and theme spacing tokens.

| Token / contract | Value | Usage |
|---|---:|---|
| Base unit | 1 cell | All terminal spacing |
| Inline item gap | 1 cell | Closely related status fragments |
| Section separator | `  │  ` | Independent footer actions |
| Composer horizontal padding | `SPACING.composer_padding_x` | Prompt dock |
| Footer height | `SPACING.footer_rows` | Contextual shortcuts/status |
| Prompt input height | `SPACING.prompt_input_rows` | Standard composer |

Shell contract:

- Header, status, composer, and footer remain fixed; transcript owns vertical scrolling.
- The live transcript spans the shell width; operator details stay in secondary surfaces.
- Width-dependent content degrades by priority: descriptive detail, compact metadata, then essential controls.
- At narrow widths, complete high-priority hints survive; never render partial key labels.
- Empty, long, unbroken, CJK, and resized content must not overflow or misalign borders.

## 5. Components

### Session shell

- **Structure**: header, transcript scroll body, optional live-turn status, composer, contextual footer.
- **Variants**: startup, live, replay, post-run; replay is read-only.
- **States**: ready, sending, streaming, recovering, disconnected, blocked, completed.
- **Accessibility**: keyboard-complete; focus remains visible; no status relies on color alone.
- **Layout**: scroll-body shell; transcript is the sole primary scroll owner.

### Live empty state

- **Structure**: compact centered Harness identity, one direct value statement, and up to three static prompt examples introduced by a tertiary label and the canonical prompt glyph above the composer.
- **Variants**: full examples when the transcript region has room; title and value statement only in compact geometry.
- **States**: visible only before the first activity and while the composer is empty; disappears as soon as work or drafting begins.
- **Interaction**: examples are inspiration, not controls; they never imply mouse-only actions or steal composer focus.
- **Layout**: chromeless and width-capped by the existing empty-state geometry so the composer remains the primary action.

### Composer

- **Structure**: focus rail/border, document input, model and mode metadata, contextual hints.
- **Variants**: focused, unfocused, shell mode, multiline, disabled, permission-blocked.
- **States**: empty, drafting, queued, submitting, clear-confirmation pending.
- **Interaction**: destructive clearing uses a two-step confirmation; while pending, the footer replaces ordinary hints with the exact next action.
- **Accessibility**: explicit key labels and visible mode/focus treatment; no hidden mouse-only action.
- **Empty guidance**: an enabled empty composer names the primary action in muted text; mode-specific guidance replaces it in shell mode, and typing removes it immediately.

### Live-turn status

- **Structure**: activity glyph, direct activity label, phase timing, context budget, optional controls.
- **Variants**: foreground, parked, background-only, recovering, reconnecting, cancelling.
- **States**: thinking, responding, running a tool, waiting on user/task, stopped.
- **Interaction**: `[stop]` and background controls remain the highest-priority right-side items.
- **Layout**: fixed one-row cluster; optional metadata yields before controls.

### Context budget segment

- **Structure**: `ctx used/limit` compact label and a six-cell fill meter with percentage when width permits.
- **Variants**: normal, warning, critical, compacted-pending-refresh, unknown.
- **States**: normal below 75%, warning from 75%, critical from 90%.
- **Accessibility**: numeric percentage accompanies the meter; semantic status color is supplementary.
- **Layout**: appears in live-turn chrome during work and in idle footer status when known; disappears before essential controls on narrow terminals.

### Contextual footer

- **Structure**: key/action hints on the left, compact runtime facts on the right.
- **Variants**: standard, reduced, minimal, confirmation takeover.
- **States**: idle, drafting, queued, replay, disabled, clear-confirmation pending.
- **Interaction**: pending confirmation replaces unrelated hints with `Esc:press again to clear` until completed or expired.
- **Accessibility**: only currently valid actions are advertised; complete help access survives compact modes.
- **Idle priority**: derive all labels from the active keymap and advertise send, mode, and shortcuts; compact modes preserve send and shortcuts first.

### Tool activity marker

- **Structure**: one fixed-width semantic glyph plus the existing title, path metadata, subtitle, and disclosure state.
- **States**: queued, running, waiting, succeeded, failed, and cancelled each use an explicit lifecycle glyph; color and motion remain supplementary.
- **Accessibility**: state must remain distinguishable in reduced-color terminals and ASCII glyph mode.

## 6. Motion & Interaction

- Motion is frame-based and meaningful: spinners indicate foreground work; pulse glyphs indicate monitored/background work.
- No decorative animation. Every changing glyph maps to activity, waiting, focus, or confirmation state.
- Confirmation and transient states must remain readable without relying on animation timing.
- Reduced-motion mode uses stable glyphs while preserving labels and state colors.
- Keyboard interaction is authoritative; mouse hover may enrich feedback but never reveal the only path.

## 7. Depth & Surface

Harness uses a mixed terminal strategy: tonal surface shifts establish shell layers, while borders and rails mark focus or containment. Shadows and raster effects are not part of the terminal surface.

- Canvas and shell may share a base tone when hierarchy is carried by spacing and text.
- Elevated composer and overlay surfaces use semantic panel roles.
- Focus uses `BorderRole::Focus`; ordinary separation uses subtle borders.
- Avoid boxing every transcript item. Borders exist only when they clarify hierarchy or interaction.

## 8. Accessibility Constraints & Accepted Debt

Constraints:

- Support full keyboard operation and visible focus across startup, live, replay, overlays, and composer states.
- Preserve high-contrast and terminal-native theme families.
- Provide ASCII glyph fallbacks for semantic icons.
- Treat wide characters as terminal cells, not bytes or scalar counts, in width-sensitive rendering.
- Never truncate the only available action label or hide stop/cancel controls behind optional metadata.
- Status language must remain understandable without color or animation.

Accepted debt:

| Item | Location | Why accepted | Owner / Exit |
|---|---|---|---|
| None | N/A | No new accessibility debt accepted for this design update. | N/A |

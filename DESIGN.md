# Terminal design

Keep the transcript readable and the next action visible. The shell shows the
active model, work status, permission requests, and controls without displacing
the conversation. Use Harness's name, theme roles, and event-derived state.

The [README recording](docs/assets/harness-demo.gif) shows the current TUI. The
[UI implementation records](docs/README.md#design-and-audit-records) document
comparisons with Grok Build and the revisions used.

## Color

[`theme_tokens.rs`](crates/harness-tui/src/theme_tokens.rs) defines the color roles.
Renderers use these roles; theme families resolve them to colors.

| Role | Token | Use |
| --- | --- | --- |
| Canvas | `ColorRole::Canvas` | Transcript and footer background |
| Shell | `ColorRole::Shell` | Fixed shell rows |
| Panel | `ColorRole::Panel` | Secondary panels |
| Elevated panel | `ColorRole::PanelElevated` | Composer and overlays |
| Primary text | `ColorRole::TextPrimary` | Content and active labels |
| Secondary text | `ColorRole::TextSecondary` | Metadata and ordinary status |
| Tertiary text | `ColorRole::TextTertiary` | Optional hints |
| Accent | `ColorRole::TextAccent` | Focus and selection |
| Success | `ColorRole::StatusSuccess` | Successful work and low context pressure |
| Warning | `ColorRole::StatusWarning` | Recoverable risk and high context pressure |
| Error | `ColorRole::StatusError` | Failures and destructive actions |
| Info | `ColorRole::StatusInfo` | Background work and progress |

Pair status colors with text or a glyph. Use accent for interaction and focus.
Add new colors through a semantic role, not a renderer-local RGB value. Check
contrast in high-contrast and terminal-native themes.

## Text and spacing

The user chooses the terminal font. Build hierarchy with spacing, semantic
colors, and normal, bold, or dim text. Use sentence case and direct status labels.
Preserve complete key labels at 80 columns.

The base unit is one terminal cell. `layout.rs`, `responsive.rs`,
`shell_geometry/`, and theme spacing tokens own geometry. Measure grapheme display
width; byte length and character count do not determine terminal width.

| Spacing | Contract |
| --- | --- |
| Related inline items | One cell |
| Independent footer actions | `  │  ` |
| Composer padding | `SPACING.composer_padding_x` |
| Footer height | `SPACING.footer_rows` |
| Prompt height | `SPACING.prompt_input_rows` |

## Shell layout

```text
┌──────────────────────────────────────────────────────────┐
│ Workspace and session context                            │
│                                                          │
│ Transcript                                               │
│ User messages, assistant replies, tools, and diffs        │
│ This region owns vertical scrolling.                     │
│                                                          │
│               ▼ Return to the latest output              │
│ Live activity and stop/background controls                │
│ Prompt composer                                          │
│ Keyboard hints                         Model and context │
└──────────────────────────────────────────────────────────┘
```

This diagram shows ownership, not exact dimensions. The header, live status,
composer, and footer stay fixed. The transcript uses the shell width. On narrow
terminals, drop optional descriptions and metadata before essential controls.
Test empty content, long unbroken text, CJK, combining characters, and resizing.

## Components

### Session shell

Startup, live, replay, and completed sessions share the shell. Replay is read-only.
Show ready, sending, streaming, recovering, disconnected, blocked, and completed
states explicitly. Every control needs a keyboard path and visible focus.

### Empty state

Show the Harness identity, a short explanation, and up to three static prompt
examples above the composer. Compact layouts show only the identity and
explanation. Hide the empty state as soon as drafting or work begins. Examples
are text, not clickable controls, and must not take composer focus.

### Composer

Show focus, the input document, model and mode metadata, and current keyboard
hints. Support focused, unfocused, shell, multiline, disabled, and permission-blocked
states. Empty guidance disappears when typing starts. Shell mode has its own guidance.

Clearing a draft requires two steps. While confirmation is pending, replace
ordinary footer hints with `Esc:press again to clear`. Keep queued, submitting,
and clear-confirmation states distinct.

### Question card

Use an accent rail, question text, scrollable choices, a fixed `z` freeform choice,
and an `[n/N]` footer for multiple questions. The focused choice expands its
description; other choices occupy one ellipsized row. Keep shell shortcuts on a
separate row. Dim an unfocused card without hiding it.

Choices use `1` through `9`, then `a` through `f`. Multi-select uses `[ ]` and
`[x]`; single-select uses `(○)` and `(●)`. These markers must carry selection
state without color.

| Input | Action |
| --- | --- |
| Arrows or `j` / `k` | Move between choices |
| Tab / Shift+Tab | Wrap through choices |
| Left / Right or `h` / `l` | Change question |
| Space | Toggle a choice |
| Enter | Select and advance, or submit the final question |
| `z` | Open the freeform choice |
| `y` | Copy the focused choice |
| Ctrl+F | Toggle fullscreen |
| Ctrl+C | Submit an existing selection or dismiss an unanswered card |
| Ctrl+Y or `X` | Dismiss the card |
| Esc | Clear the answer before moving focus to scrollback |

### Live status and context

Keep activity, phase timing, and optional context metadata on one row. Stop and
background controls take priority over metadata. Distinguish foreground, parked,
background-only, recovery, reconnection, and cancellation states.

The context segment shows `ctx used/limit`. When width permits, add a six-cell
meter and a percentage. Warn at 75% and use critical styling at 90%. Unknown and
compacted-pending-refresh states must not claim a known budget. Show context in
the live status during work and the footer while idle; hide it before controls
when space runs out.

### Footer

Derive hints from the active keymap and show only valid actions. Put actions on
the left and runtime facts on the right. Compact layouts preserve send and help
access. Confirmation temporarily replaces unrelated hints.

### Tools and diffs

Use distinct glyphs for queued, running, waiting, succeeded, failed, and cancelled
tools. Preserve the distinction in ASCII and reduced-color modes. Pulse only the
running marker, not the label, path, or output. Cached layouts repaint the marker
at the 33 ms active cadence. Waiting and queued markers stay still; replay and
reduced motion settle immediately.

Separate the header label, arguments or path, and summary. Grouped rows keep file
identity. Listings count entries. A supplied shell description becomes the header;
the command remains in expanded output.

Read and search calls can fold into groups. Shell calls stay separate until a
run exceeds eleven entries, then fold the older prefix and leave ten recent
commands visible. Choose preview rows after display-cell wrapping. Reads show
five head rows and three tail rows; shell output shows two and three. Expansion
shows stored output and must not imply that truncated data can be retrieved.

Collapsed edits show trusted diff counts. Expanded diffs separate header and body,
number the lines, apply syntax colors, and put change backgrounds on content
rather than the number gutter. Numbered permission choices and keyboard navigation
use the same coordinator decision. A rejection does not alter the composer draft;
always-approve still needs confirmation.

Open tool content can show a 400 ms completion rail without changing layout.
Schedule its expiry repaint. This state is temporary and never enters replay.

### Return to the latest output

When follow mode is detached and content remains below, paint `▼` in the
transcript's reserved final row at `x + width / 2`. Center a three-cell click target
on it. Use secondary text at rest and primary text on hover. Activation scrolls to
the bottom and restores follow mode; the keyboard equivalent remains available.
Hide the control while following, at the bottom, or without overflow. It must not
consume composer space or change transcript measurement.

## Motion, focus, and borders

Animate work and state changes only. Spinners, pulses, and completion feedback
must remain understandable with motion disabled. Do not rely on timing for
confirmation. A mouse hover can clarify a control, but cannot reveal its only
access path.

Use panel tones to distinguish layers and `BorderRole::Focus` for focus. Add
borders only where they clarify grouping or interaction. Do not box every
transcript item or add raster shadows to terminal output.

## Accessibility checks

- Every startup, live, replay, overlay, and composer action works from the keyboard.
- Focus and status remain readable without color or animation.
- Semantic glyphs have ASCII fallbacks.
- Wrapping and truncation preserve whole graphemes and display-cell alignment.
- Optional metadata never hides stop, cancel, or the only action hint.
- Overlays receive their own input; background components do not consume it.

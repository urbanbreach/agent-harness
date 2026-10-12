# Terminal design

Keep the transcript readable and the next action visible. The shell shows the
active model, work status, permission requests, and controls without displacing
the conversation. Use Harness's name, theme roles, and event-derived state.

The [README recording](docs/assets/harness-demo.gif) shows the current TUI. The
[UI implementation records](docs/README.md#design-and-audit-records) document
comparisons with the reference application and the revisions used.

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

`Theme::by_name` accepts `default`, `harness-dark`, and `dark` for the dark
theme; `harness-light` and `light` for the light theme; `high-contrast`; and
`terminal-native`. The named-theme picker lists `default`, `harness-light`,
and `high-contrast`. Theme-family choices are `dark`, `light`, and `auto`.
Color capability detection uses `NO_COLOR`, `COLORTERM`, and `TERM`; themes
fall back to 256 colors, ANSI 16 colors, or terminal-native monochrome.

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
| Composer padding | `DESIGN_TOKENS.spacing.composer_padding_x` (two cells) |
| Footer height | `DESIGN_TOKENS.spacing.footer_rows` (one row) |
| Prompt height | `DESIGN_TOKENS.spacing.prompt_input_rows` (three rows) |

## Shell layout

```text
┌──────────────────────────────────────────────────────────┐
│ Workspace, session, token speed, cost, and context       │
│                                                          │
│ Transcript                                               │
│ User messages, assistant replies, tools, and diffs       │
│ This region owns vertical scrolling.                     │
│                                                          │
│               ▼ Return to the latest output              │
│ Live activity and stop/background controls               │
│ Prompt composer                                          │
│ Keyboard hints                 Runtime and monitor state │
└──────────────────────────────────────────────────────────┘
```

This diagram shows ownership, not exact dimensions. The header, live status,
composer, and footer stay fixed. Live transcripts use the shell width; wide
replay layouts reserve a right sidebar. A permission or question dock replaces
the composer while awaiting an answer. Composer height depends on its content
and layout, not just the base spacing token. On narrow terminals, drop optional
descriptions and metadata before essential controls. Test empty content, long
unbroken text, CJK, combining characters, and resizing.

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

Clearing a nonempty idle draft requires two Esc presses within 800 ms. While
confirmation is pending, replace ordinary footer hints with
`Esc:press again to clear`. Keep queued, submitting, and clear-confirmation
states distinct.

### Question card

Use an accent rail, question text, scrollable choices, a fixed `z` freeform choice
when custom answers are allowed, and an `[n/N]` footer for multiple questions.
The focused choice expands its description; other choices occupy one ellipsized
row. Keep shell shortcuts on a separate row. Dim an unfocused card without
hiding it.

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
| Ctrl+C or Shift+X | Dismiss the card (deny the pending request) |
| Ctrl+Y | Submit the current answers when they are valid |
| Esc | Clear the answer before moving focus to scrollback |

### Live status and context

Keep activity, phase timing, and optional context metadata on one row. Stop and
background controls take priority over metadata. Distinguish foreground, parked,
background-only, recovery, reconnection, and cancellation states.

The top-right breadcrumb shows compact context token counts and a rounded
percentage, such as `96K / 128K 75%`. Its denominator is the compaction
threshold, not necessarily the model's full context window. The same row can
show generation speed (`42.1 tok/s`) and session cost (`$0.123`). Speed is live
while streaming and keeps the last reading between requests. Cost combines
priced usage across the session's models; `+` marks missing prices and `(sub)`
marks subscription authentication. With no priced usage, cost is hidden.
Speed drops first, then cost, when the row is too narrow. The footer and live
status do not repeat the context counter. Unknown and compacted-pending-refresh
states must not claim a known budget. Hide live status metadata before controls
when space runs out.

### Footer

Derive hints from the active keymap and show only valid actions. Put actions on
the left and runtime facts on the right. Compact layouts preserve send and help
access. Confirmation temporarily replaces unrelated hints.

The footer can show runtime summaries and counts of pending permissions, LSP
servers, and MCP servers with a `/status` hint. The disclosure row below the
composer has a right-aligned monitor segment when enough room remains for hints.
It shows `watching`, up to two titles, the monitor count, elapsed time, and mute
state. Keep these locations distinct; token speed and cost belong in the
breadcrumb.

Default shell bindings come from `keybindings.rs`; config overrides change
the hints. Question cards and other overlays handle their own navigation.

| Input | Action |
| --- | --- |
| Ctrl+p | Open the command palette |
| Ctrl+x | Open shortcuts and begin a leader sequence |
| Ctrl+x, then t / m / s | Open themes / models / status |
| Ctrl+x, then y / x / g | Copy a message / export the session / open lineage |
| Ctrl+x, then Down | Open the first child session |
| Ctrl+t | Toggle the todo pane |
| Ctrl+g | Toggle the tasks pane |
| Shift+Tab | Cycle the current model variant, not the agent mode |
| Ctrl+o | Toggle approval mode; a pending permission uses a confirmation step |
| F2 or Ctrl+, | Open status |
| Enter | Send a draft, or steer the running turn |
| Alt+i or Ctrl+Alt+Enter | Queue a follow-up after the running turn |
| Ctrl+Enter or Alt+r | Cancel the running turn and replace it with the draft |
| Shift+Enter, Ctrl+j, or Alt+Enter | Insert a newline in single-line mode |
| Alt+m | Toggle multiline mode (Enter inserts a newline; Alt+Enter sends) |

### Tools and diffs

Show tool input as it arrives, with syntax colors and the newest five wrapped
rows. Keep a visible writing label and allow expansion without waiting for
execution. Eval keeps the same code preview after execution; its result opens
with the details, while failures remain visible. Live argument previews never
enter replay history.

Use distinct glyphs for queued, running, waiting, succeeded, failed, and cancelled
tools. Preserve the distinction in ASCII and reduced-color modes. Pulse only the
running marker, not the label, path, or output. Cached layouts repaint the marker
at the runtime flush cadence (4 ms by default, adjustable with
`HARNESS_TUI_MIN_DRAW_MS`, clamped to 1 through 100 ms). Waiting and queued
markers stay still; replay and reduced motion settle immediately.

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
yolo still needs confirmation.

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

Live replies, reasoning, tool input, eval output, and command output reveal whole
graphemes at a 16 ms cadence after an 80 ms initial buffer. The reveal rate follows
incoming text and catches up during bursts. Durable completion, disconnection,
and reduced motion settle pending text immediately. History and runtime state
remain complete while only the displayed text is paced.

Set `HARNESS_TUI_REDUCED_MOTION` to `1`, `true`, `yes`, or `on` (case-insensitive,
with surrounding whitespace ignored), or set `HARNESS_DISABLE_ANIMATIONS` to
any value, including an empty value. Both disable animated motion and settle
pending streamed text. Monitor elapsed-time labels still refresh once a second.

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

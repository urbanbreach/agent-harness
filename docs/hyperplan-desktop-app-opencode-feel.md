# What “Feels Like OpenCode” Means for the Harness Desktop App

> Hyperplan artifact — UX/desktop product design definition.
> Reads the existing Harness TUI identity into native desktop surfaces:
> onboarding flow, system tray/menubar, launcher, config UI, session browser,
> first-run wizard, and update prompts.

## 1. The feel, in one sentence

The Harness desktop app should feel like a **local-first command center** that
happens to have a window frame. It borrows OpenCode’s keyboard-first honesty,
its prompt-as-center-of-gravity, and its refusal to hide state behind magic —
but re-expresses those values through native desktop conventions (menu bar,
window chrome, system tray, quick launcher) instead of terminal affordances.

It is **not** a GUI skin over the TUI. It is a translation: same spirit, different
language.

## 2. Core design principles

| Principle | OpenCode TUI expression | Harness desktop translation |
|-----------|--------------------------|----------------------------|
| **Compose-first** | Cursor is already in the prompt when the app opens | Launcher + new-session window both land directly in an active composer |
| **Keyboard sovereignty** | Leader-key chords (`ctrl+x m`) | Global leader chord (configurable, default `Ctrl+Shift+O`) + menu-bar mnemonics that mirror the same commands |
| **Palette discoverability** | `Ctrl+P` surfaces every action | `Ctrl+Shift+P` command palette; menu items echo palette titles exactly |
| **Honest state** | Footer shows cwd, pending permissions, LSP/MCP health | Menubar status badges + tray icon state + compact title-bar status |
| **Skippable guidance** | Onboarding auto-skips if credentials exist | Wizard pages are gates you can walk through, not walls |
| **Explicit permissions** | Permission modal with typed titles + diff preview | Native permission ask with the same selector explanation pattern |
| **Replay as product** | Replay mode is read-first class | Session browser lets you fork/clone/resume from any historical session visually |

## 3. OpenCode’s XDG-based centralized storage model

The desktop app uses a **centralized, XDG-compliant directory layout** while
preserving Harness’s project-local config override behavior. This mirrors how
OpenCode keeps user state in predictable OS-level home directories rather than
scattering it next to project roots.

### 3.1 Directory contracts

| XDG base | Harness desktop path | Holds |
|----------|----------------------|-------|
| **Config** | `~/.config/harness/` | `harness.jsonc`, `tui.jsonc`, `desktop.jsonc`, agent-profile overrides placed here act as global defaults |
| **Data** | `~/.local/share/harness/` | Centralized session directories (`sessions/`), event logs, exported support bundles, lineage indexes |
| **State** | `~/.local/state/harness/` | First-run flags (`desktop-first-run-shown`), launcher geometry/state, update-prompt dismissals, recent prompts/cycling state |
| **Cache** | `~/.cache/harness/` | Provider catalog snapshots, model metadata cache, transient render caches, downloaded update artifacts |

### 3.2 Project-local override stays valid

Project-local `./harness.jsonc` or `./harness.json` still overrides the global
default. The desktop app shows project-local config in the Config UI as an extra
scope tab (“Project”) and respects it when launching from a folder. Centralized
storage is the **default**, not the only model.

### 3.3 Migration / coexistence

- Existing project-local sessions under `.harness/` or `sessions/` are not moved.
- The session browser can display them as a separate source (“Project-local
  sessions”) by mounting the project root as a library source.
- New sessions created from the desktop app default to the centralized
  `~/.local/share/harness/sessions/` tree unless the user explicitly launched
  from a project folder.
- The first-run wizard asks once whether to import/index existing project-local
  sessions; this choice is reversible from the session-browser settings.

### 3.4 Why this matters for the feel

OpenCode’s desktop app never makes the user wonder where a session lives.
Harness desktop should behave the same: one predictable home for state, one
searchable place for sessions, and clear fall-through rules when a project-local
config or session overrides it. The UI surfaces the effective paths in the
session-browser inspector and the Config UI footer so nothing is hidden.

## 4. First-run wizard

### 3.1 Trigger conditions
The wizard appears only when **all** of these are true:
- No existing `harness.jsonc` / `harness.json` is found.
- No authenticated credential is stored.
- This is the first app launch (a `~/.local/state/harness/desktop-first-run-shown` flag is absent).

Preconfigured users (enterprise rollout, dotfiles sync) see zero wizard steps.

### 3.2 Step architecture
Use a **progressive-disclosure sidebar** on the left, not a forced linear carousel.
Each step is a card; completed steps collapse into a compact log. The user can
jump back at any time.

1. **Welcome / why local-first** — one sentence: “Harness keeps the full session
   log on your machine. Nothing leaves unless you explicitly run a tool that does.”
   Primary action: “Set up provider.”
2. **Provider + auth** — same provider catalog as the TUI; OAuth flows pop a
   narrow native browser sheet. Fallback API-key entry is available but visually
   de-emphasized.
3. **Workspace selection** — default to `$HOME`; offer “Add project folders” with
   a native folder picker. Each folder becomes a quick-open target later.
4. **Agent profile** — default to `build`; explain `plan` in one line; offer
   “Switch anytime with `Tab`.”
5. **First prompt** — a warm-start composer pre-filled with a rotating placeholder
   (`Summarize this codebase`, `Write a test for the current file`, `Explain the
   most recent commit`). Submitting it closes the wizard and opens the live session.

### 3.3 Tone
- **No exclamation points.** Calm, confident, engineer-to-engineer.
- **No feature tours.** The wizard sets up capability, it does not explain the UI.
- **No dark patterns.** Every optional telemetry or update-auto-check is unchecked by default.

## 5. Onboarding flow (post-wizard, ongoing)

Onboarding is not the wizard; onboarding is **the first 30 minutes of use**.

| Moment | Surface | Behavior |
|--------|---------|----------|
| First empty composer | Inline placeholder cycles through real examples | Same as TUI placeholder rotation |
| First file mention (`@`) | Autocomplete panel with frecency; top item is the file nearest to current focus | Reuses TUI `@` logic |
| First tool call | Native permission sheet slides from top, not a modal block | Matches TUI permission modal semantics |
| First error | Error banner + “Show details” + recovery hint; no alert dialog | Mirrors TUI error-details overlay |
| First run end | Toast: “Session saved. Press `Ctrl+Shift+L` to resume.” | Reinforces discoverability |

## 6. System tray / menubar

### 5.1 Tray icon as status beacon
The tray icon has **four visual states**, not just online/offline:

- **Idle** — muted Harness glyph.
- **Active turn running** — subtle pulsing dot (not a bouncing dock icon).
- **Pending permission** — amber warning triangle overlay.
- **Error / retrying** — red dot overlay with a tooltip showing category.

Right-click tray menu:
```
New session             ⌘N / Ctrl+N
Resume last session     ⌘⇧L / Ctrl+Shift+L
—————————————————————
Pending permissions (2) △
Running subagents (1)   ⊙
—————————————————————
Preferences             ⌘, / Ctrl+,
About Harness
Quit                    ⌘Q / Ctrl+Q
```

### 5.2 Menubar (macOS) / hamburger menu (Windows/Linux)
Top-level menus map 1:1 to the command palette categories:

- **Session** — New, Resume, Recent, Fork, Clone, Export, Close Window, Quit
- **Navigate** — Back to composer, Jump to last user message, Jump to first/last message, Toggle sidebar
- **Agent** — Switch agent/model/variant, Open command palette, Toggle Plan/Build mode
- **View** — Toggle timestamps, Toggle thinking blocks, Toggle tool details, Toggle sidebar, Enter/exit replay mode
- **Window** — Native window management + session tabs
- **Help** — Open docs, Keyboard shortcuts, Support bundle, About

Menu item titles must match command-palette titles. A menu action and a palette
action are the same action, surfaced twice.

## 7. Launcher

The launcher is the **desktop-specific entry point** that does not exist in the
TUI. It must not feel like a separate app; it is the same session surface in a
smaller window.

### 6.1 Invocation patterns
- Global hotkey: `Alt+Space` (configurable). Works even when app is closed
  (triggers quick-open window via background agent).
- Dock/taskbar icon click when no window exists: opens the launcher instead of a
  full session window.
- After wizard completion: launcher opens automatically and stays open.

### 6.2 Launcher anatomy
A compact floating window (~640×120 px), position remembered per display:

```
┌────────────────────────────────────────────────────────────┐
│  [Harness glyph]  Ask or run… / ! for shell                │
│  Build   gpt-5.4-mini   ./agent-harness                    │
└────────────────────────────────────────────────────────────┘
```

- Typing starts a new session in a full window.
- `!` prefix enters shell mode (same adaptation as TUI U3: routes through the
  coordinator’s `bash` tool path, asks permission).
- `↑` / `↓` cycles recent prompts from any session.
- `Tab` expands file mentions from cwd.

### 6.3 Launcher persistence
The launcher is the **only** surface that may keep a lightweight background
process alive. Launcher geometry, global-hotkey registration, and dismissed-but-not-submitted drafts persist in `~/.local/state/harness/launcher-state.json`.
If the user types and submits, the launcher opens a full session window and
sends the prompt to the coordinator. If the user dismisses without submitting,
no session is created.

## 8. Config UI

### 7.1 Two-pane model
Left: **scope tabs** — Global (`~/.config/harness/`) / Workspace / Project / Session-local Toggles.
Right: **editor**.

Harness config is still JSONC-first. The Config UI is a **structured editor over
JSONC**, not a hidden-JSON wizard. Every field shows its JSONC path and has a
“Open in editor” button.

### 7.2 Section map
| Section | Live behavior |
|---------|---------------|
| **Provider** | Provider picker, credential status, model catalog with favorites/recents, fallback chain |
| **Agent profiles** | List of `.agent-harness/agents/*.md` files; toggle enablement; open profile in default editor |
| **Permissions** | Durable grants viewer; per-tool allowlist/blocklist; blocked-command list for `bash` |
| **Shortcuts** | Visual keybinding editor; conflicts highlighted; leader-key configurable |
| **Theme** | Theme picker with live preview of the current session transcript |
| **Session defaults** | Default agent, model, variant; auto-export on close; compaction policy |
| **MCP / LSP** | Server registry, discovered tools, health dots, per-server toggle |

### 7.3 Unsafe-action pattern
Changes that invalidate an active session (provider switch mid-turn, permission
revocation) show a **two-stage confirm** with explicit consequences:
> “Switching models will cancel the current streaming turn. Cancel and switch?”

This mirrors the TUI permission modal’s always-stage explanation pattern.

## 9. Session browser

### 8.1 Layout
Three-column browser window:

- **Left (220 px):** Filter + source list. Sources: Centralized (`~/.local/share/harness/sessions/`), Project-local, Trash, Exports, Background children.
- **Center (flex):** Session cards. Each card = title, relative updated time,
  agent/model badge, modified-files count, child-session count, status dot.
- **Right (320 px):** Inspector. Read-only summary from replay-derived data:
  turn count, tool calls, permission grants, exported artifacts, lineage mini-tree.

### 8.2 Power-user gestures
- `Ctrl+F` focuses filter.
- `Ctrl+P` opens an in-browser command palette (`rename`, `fork`, `clone`, `export`, `trash`).
- Drag a session onto the launcher or dock icon to fork from it.
- Pin sessions to a “Pinned” group (same TUI-local state contract as TUI U6).

### 8.3 Visual hierarchy
- Pinned group always first, visually separated by an inset panel background.
- Active/running sessions get a thin left accent line in the theme’s action color.
- Failed sessions get a red left accent + inline error category badge.
- Background children are grouped under their parent, collapsible.

## 10. Update prompts

### 9.1 Philosophy
Harness updates should feel like **transit announcements**, not sales pitches.

### 9.2 Prompt tiers
| Scenario | Surface | Copy shape |
|----------|---------|------------|
| Patch available, no active session | Toast from tray + menubar badge | “Harness 1.2.3 is available. Restart to update.” |
| Patch available, session running | Badge only; no interruptive toast | “Update ready — will apply on next quit.” |
| Security update | Non-blocking banner in every window | “A security update is available. Restart when ready.” |
| Breaking config change | Blocking modal at startup, skippable once | “Your config uses a removed key. See migration notes.” |
| Update downloaded | Menubar item changes to “Restart to Update” | Same |

### 9.3 Changelog presentation
When the user opens the update detail, show a **machine-readable diff summary**:
- New commands added to the palette (with keybindings).
- New config keys.
- Changed default behavior.
- CVEs fixed, if any.

No marketing copy, no GIFs.

## 11. Animation and motion language

- **Fast:** 120 ms transitions for panels, menus, sheets.
- **Purposeful:** Motion explains state change, never decorates.
- **Respectful:** No parallax, no bouncing, no sound effects except for the
  optional “turn complete” subtle chime (off by default).
- **Reduced motion:** A single OS-aware flag disables all animated transitions
  and replaces pulsing tray states with static color changes.

## 12. Accessibility baseline

- Every palette/menu action has an accelerator label.
- Color is never the only signal: pending permissions use a triangle glyph + count.
- All dialogs support `Enter` (primary), `Esc` (cancel/secondary), and `Tab` order.
- Screen-reader labels include the JSONC path for config fields.

## 13. What it does NOT feel like

| Avoid | Because |
|-------|---------|
| A chat app | Harness is a command log, not a conversation |
| A settings labyrinth | Config is JSONC; the UI is an editor over it, not a replacement |
| A notification spammer | Tray pings only for permission/retry/error, not every completed turn |
| A cloud-first product | Share/connect features are explicitly post-V1; no login gate before local use |
| A generic Electron wrapper | Native menu bar, native file pickers, native sheets; the frame should feel platform-native |

## 14. Implementation notes for planners

- **Stack:** Recommend Tauri or a thin native Rust runtime with a web view.
  The TUI logic (`harness-tui`) is deliberately immediate-mode Ratatui and should
  **not** be reused directly; the desktop app should consume `harness-core`
  projections through the same public event-replay contract.
- **Config contracts:** Keep runtime config (`harness.jsonc`) and desktop UI
  config (`desktop.jsonc`) separate, mirroring the TUI `tui.jsonc` split.
  Desktop config lives in `~/.config/harness/desktop.jsonc` by default.
- **Session stores:** Centralized under `~/.local/share/harness/sessions/`. Read
  only through `harness-core` projections; never write session directories from
  the desktop layer. Project-local sessions remain supported as a secondary
  source surfaced in the session browser.
- **Permissions:** Route every approval through the coordinator; the desktop layer
  may render native sheets but the durable event must come from `harness-core`.

## 15. Open questions for the team

1. Should the launcher global hotkey be installable at OS login, or only while
   the app is running?
2. Do we ship a single “unified” window model (tabs) or separate windows per
   session (matches OpenCode desktop behavior more closely)?
3. Should the desktop app bundle its own Rust runtime, or require an existing
   `harness` binary on `PATH`?
4. What is the platform order: macOS first (OpenCode desktop reference), or
   Linux first (dogfooding)?

---

**Next step recommendation:** Validate this definition against the current
roadmap-v1 desktop/mobile item, then convert each surface into a phased set of
implementation cards tied to the backend readiness (coordinator projections,
config contracts, event replay).
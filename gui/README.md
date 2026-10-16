# Harness desktop GUI

A native desktop interface for Harness, written in TypeScript with
[GPUIX](https://github.com/remorses/gpuix) (React rendered by Zed's GPUI). It
covers everything the terminal UI does and adds a project sidebar, review diffs,
a file editor, terminals and per-turn checkpoints. Linux only, like Harness.

## Run it

Requirements: Rust (see `rust-toolchain.toml`), [Bun](https://bun.sh) 1.3 or
newer, and the GPUI build prerequisites (Vulkan driver, Wayland or X11,
fontconfig, xkbcommon).

```bash
cargo build -p harness             # the backend the GUI talks to
gui/native/build.sh                # GPUIX JS packages + native addon (first run takes minutes)
cd gui && bun install
./scripts/run.sh                   # or: bun run dev, for hot reload
```

`harness gui` starts the desktop app found at `$HARNESS_GUI`, or `harness-gui`
next to the `harness` executable. From a checkout:

```bash
HARNESS_GUI=$PWD/gui/scripts/run.sh target/debug/harness gui
```

## How it fits together

```text
gui/src (Bun, React)  ──JSON lines over stdio──▶  harness gui-server (crates/harness)
  app/        screens: sidebar, composer, transcript, right panel, palette, settings
  store/      application state; drafts, layout and preferences persist via the server
  backend/    protocol types, the stdio client and the fixture backend
  model/      Harness events → session view model; tool presentation
  markdown/   GitHub-flavoured markdown on the native rich-text element
  workspace/  files, git, per-turn checkpoints, terminals, OS integration
  ui/ theme/ i18n/  shared controls, tokens, strings
```

- The backend owns everything Harness-specific: configuration, credentials,
  sessions, coordinators (several live at once), events and intents. It reuses
  the terminal UI's runtime wiring (`crates/harness/src/tui/live_runtime.rs`).
  The wire contract is `docs/gui/protocol.md`.
- The GUI process owns generic workspace work and never writes Harness
  journals. Per-turn checkpoints are snapshot commits under
  `refs/harness-gui/<session>/<turn>-{start,end}`; capturing them never touches
  your index, HEAD or branches.
- GUI state lives in `<harness home>/gui/` (`state`, `drafts`, attachment
  blobs), created on the first change, never at startup.

## GPUIX

`gui/native/build.sh` checks out a pinned GPUIX revision and its GPUI fork into
`~/.cache/harness-gui-native` (override with `HARNESS_GUI_NATIVE_SRC`), applies
`native/patches/*.patch` to GPUIX and `native/patches/zed/*.patch` to GPUI, then
builds the JavaScript packages into `vendor/` and the addon into
`native/dist/`. The patches add a `rich-text` element (styled runs and inline
washes), editor syntax runs, gutters, search washes and selection control,
clipboard access, file drag events, window chrome controls, and virtual-list
visibility notifications. To change native code, edit the checkout, export the
diff over the patch file, and rerun the script; `native/smoke/app.tsx`
exercises every native addition.

## Development

```bash
cd gui
bun x tsc --noEmit -p .            # typecheck
bun test                           # unit and behaviour tests
bun run format                     # Prettier
```

- `HARNESS_GUI_FIXTURE=<name>` runs against `test/fixtures/scenarios/<name>.json`
  instead of the backend, for deterministic screens.
- `HARNESS_GUI_MOCK_SCRIPT=<file.json>` runs the real backend with a scripted
  mock provider (text, reasoning, tool calls, delays); see
  `test/fixtures/scripts/interactive.json` and `docs/gui/protocol.md`.
- Always point `HARNESS_HOME` at a temporary directory when experimenting.

### Workspace panels

Review opens on **Uncommitted** changes; the Review button on a transcript turn
opens that turn's checkpoint instead. File headers and the changed-file tree keep
their counts and status badges aligned to the trailing edge. Review scrollbars
appear during scrolling or pointer interaction and fade at rest.

The file editor supports Ctrl+Home/End and Ctrl+Shift+Home/End for document-edge
navigation and selection. Find keeps its case, whole-word and regular-expression
options inside the query field; Enter/Shift+Enter navigate matches. Empty
regular-expression matches are not replacement targets.

Each Terminal tab owns an independent shell. Ctrl+Shift+C copies the selected
terminal cells; Ctrl+Shift+V pastes clipboard text, respecting bracketed paste and
normalizing line endings. Word/line selections can be extended with dragging.
Panel resizing reflows output and resizes the PTY; shell exit preserves output
and marks the status dot red.

`test/fixtures/scripts/panel.json` exercises a local edit, todo updates and a
saved rollout plan through the scripted mock backend, without provider calls.
Use an isolated `HARNESS_HOME`; `test/fixtures/repos/panel.ts` resets only its
dedicated `/tmp/harness-panel-repos` fixture repositories.

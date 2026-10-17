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
  sessions, coordinators (several live at once), events and intents. It shares
  the live session wiring with the terminal UI (`crates/harness/src/live/`).
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
washes), editor syntax runs, gutters, search washes, selection control, ghost
text, opt-in key interception and Linux word selection, clipboard access, file
drag events, window chrome controls and virtual-list visibility notifications;
they also keep virtual-list rows by identity and the editor's desired column.
To change native code, edit the checkout, export the diff over the patch file,
and rerun the script; `native/smoke/app.tsx` exercises every native addition.
`gui/native/build.sh --test` also runs the native crate's tests and the patched
GPUI list regression. Rich-text links use native focus and hit testing; syntax
runs are cached between text/style changes.

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
- `test/fixtures/scripts/panel.json` drives a local edit, todo updates and a
  saved plan through the scripted mock backend; `test/fixtures/repos/panel.ts`
  resets its `/tmp/harness-panel-repos` fixture repositories.

## Using it

- Language: Settings → Appearance → Language (System, English, 简体中文,
  日本語). System follows `LC_ALL`, `LC_MESSAGES` or `LANG`.
- The header's information button opens **Environment and background work**:
  the branch and its line changes, **Commit or push**, **Compare branch** and
  the task's background work. Usage is on Ctrl+U and the composer's meter.
- Review opens on uncommitted changes; a turn's Review button opens that turn's
  checkpoint. Editing an earlier prompt restores the files from its checkpoint.
  Checkpoints skip untracked `.env` files, and a restore first saves the real
  index under `refs/harness-gui/<session>/safety-…-index`, so staged work stays
  recoverable (`git read-tree <ref>`).
- File editor: Ctrl+F find, Ctrl+Alt+F replace (Enter replaces the current
  match, Ctrl+Alt+Enter all), Ctrl+Home/End and their Shift variants.
- Terminal: each tab is its own shell; Ctrl+Shift+C/V copy and paste.
- External links open only `http`, `https` and `mailto` URLs.

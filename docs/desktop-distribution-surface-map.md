# Desktop App Distribution Surface Map

> Target: the `harness` Rust CLI / TUI (event-sourced agent harness).  
> Goal: enumerate every implementation decision, file format, platform API, and user-visible flow that moves the project from `cargo build` to a shipped, installable desktop application.  
> Phase-1 consensus: ship a terminal-first binary that respects XDG-style directories, distributed through an npm wrapper that downloads per-target binaries from GitHub Releases. Tray, launcher, native window frame, in-binary auto-update, and store packages are explicitly post-V1. This document catalogs the full surface for planning, but implementation sequencing is constrained by the Phase-1 scope below.

---

## 0. Phase-1 scope decision

After round-2/3 cross-critique, the team converged on the smallest shippable slice that satisfies the user request (“download via npm, own system folders”).

| Area | Phase 1 (now) | Phase 2+ (later) |
|---|---|---|
| **Application binary** | Single terminal `harness` executable (CLI + Ratatui TUI). | Optional tray, launcher, or native window frame. |
| **Install/distribution** | `npm install -g harness` wrapper that fetches the correct GitHub Release binary. | Homebrew, winget, apt, AppImage, Flatpak, Snap, .app/.dmg/.msi installers. |
| **Auto-update** | Users run `npm update -g harness`; binary self-update is not included. | `harness self-update` backed by a release feed + signatures after we stabilize the CLI contract. |
| **System directories** | Use XDG-style config/data/state/cache dirs; project-local overrides remain valid. | OS keychain credential storage, OS-level log rotation, crash reporter, telemetry opt-in. |
| **First-run UX** | CLI/TUI first-run hints and `harness init`; default config is copied if missing. | Optional GUI onboarding wizard only if we later ship a native window. |
| **PATH/completions** | npm wrapper places binary on PATH; completions command exists but is not auto-installed by Phase 1. | Installers that write shell completions and registry/Launch Services metadata. |

Rationale: a terminal binary requires no new windowing runtime, no background process, and no persistent daemon. The npm wrapper gives the user-requested install experience with minimal distribution plumbing. XDG directories move state out of project roots without changing the existing TUI model.

---

## 1. Build artifacts & packaging

### 1.1 Binary targets
- `crates/harness` produces the primary executable (`harness`).
- Workspace also builds library crates (`harness-core`, `harness-providers`, `harness-tools`, `harness-tui`, `harness-testkit`).
- Distribution focus is the CLI/TUI binary; libraries are internal unless we later ship a public API.

### 1.2 Per-platform package formats

| Concern | macOS | Windows | Linux |
|---|---|---|---|
| **Raw binary** | `harness` (Mach-O universal or arch-specific) | `harness.exe` | `harness` ELF |
| **Archive** | `.tar.gz`, `.zip` | `.zip` | `.tar.gz`, `.tar.xz` |
| **Installer package** | `.dmg` (drag-drop), `.pkg` (system installer) | `.msi` (WiX/MSI), `.exe` (Inno Setup, NSIS), MSIX | `.deb`, `.rpm`, AppImage, Flatpak, Snap |
| **App bundle** | `.app` bundle | — | `.desktop` + icon + metainfo |
| **Store package** | — | MSIX for Windows Store | Flatpak/Snap store |

### 1.3 Cargo tooling options
- `cargo-bundle` — generates `.app`/`.deb`/`.msi` from `Cargo.toml` metadata.
- `cargo-wix` — Windows MSI installer.
- `cargo-deb` / `cargo-rpm` / `cargo-generate-rpm` — Linux package builders.
- `cargo-appimage`, `cargo-flatpak`, `cargo-snap` — sandboxed Linux formats.
- `cargo-build-binutils` / `cargo-zigbuild` — cross-compilation helpers.
- `tauri-bundler` / `create-tauri-app` — if we ever wrap the TUI in a webview shell.

---

## 2. Installers & first-run UX

### 2.1 macOS installers
- **.app bundle**: `Contents/MacOS/harness`, `Contents/Resources/icon.icns`, `Info.plist` with `CFBundleIdentifier`.
- **.dmg**: signed `.dmg`, background image, `/Applications` alias, quarantine handling.
- **.pkg**: post-install scripts to add `/usr/local/bin/harness` symlink, optionally register Login Item.
- **Gatekeeper / Notarization**: Apple notarization, `com.apple.quarantine` xattr, `spctl --assess`, staple.

### 2.2 Windows installers
- **MSI (`cargo-wix`)**: Program Files install, Start Menu shortcut, `Add/Remove Programs` entry, optional environment modification.
- **Inno Setup / NSIS**: custom wizard, PATH update, file associations.
- **MSIX**: containerized install, auto-update via Microsoft Store/own feed, file-system virtualization.
- **Elevation**: per-user vs. per-machine install; UAC prompts.

### 2.3 Linux installers
- **.deb/.rpm**: package dependencies (e.g. `libssl3`, terminal libraries), `/usr/bin/harness` symlink, man page, bash completions.
- **AppImage**: single executable, no install, optional `appimaged` integration.
- **Flatpak**: manifest, runtime/SDK, sandbox portals, `.desktop` export.
- **Snap**: `snapcraft.yaml`, confinement level, interfaces (home, network, etc.).
- **Universal archive**: tar.gz + install script that places binary in `~/.local/bin`.

### 2.4 First-run UX concerns
- Detect first launch vs. upgrade (version marker in data dir).
- Initialize default config if missing (currently requires user to copy `configs/harness.example.jsonc`).
- Prompt for OAuth login (`harness auth login codex`) on first live run.
- Create session/data directories with correct permissions.
- Telemetry/crash-reporting opt-in.
- Welcome/ onboarding TUI overlay or `--first-run` command.
- Migration from older config locations (`$XDG_CONFIG_HOME/harness/config.jsonc` → `harness.jsonc`).

---

## 3. Directory layout & platform directories

### 3.1 Current harness behavior
- Runtime config discovery is XDG-aware (`crates/harness-core/src/config/discovery.rs`):
  - Global: `$XDG_CONFIG_HOME/harness/harness.json{,c}` or `~/.config/harness/harness.json{,c}`.
  - Local: `./harness.json{,c}` or `./.agent-harness/harness.json{,c}`.
  - TUI config: separate `tui.json{,c}` in same locations.
- Session/output directories are likely relative to project or data dir (verify in `crates/harness-core/src/session_paths.rs`).

### 3.2 Standard directories to adopt

| Purpose | macOS | Windows | Linux |
|---|---|---|---|
| **Config** | `~/Library/Application Support/harness` or `~/.config/harness` | `%AppData%\harness` (`FOLDERID_RoamingAppData`) | `$XDG_CONFIG_HOME/harness` |
| **Data** | `~/Library/Application Support/harness` | `%LocalAppData%\harness` | `$XDG_DATA_HOME/harness` |
| **Cache** | `~/Library/Caches/harness` | `%LocalAppData%\harness\cache` | `$XDG_CACHE_HOME/harness` |
| **Logs** | `~/Library/Logs/harness` | `%LocalAppData%\harness\logs` | `$XDG_STATE_HOME/harness/logs` or `$XDG_DATA_HOME/harness/logs` |
| **State** | same as Data | `%LocalAppData%\harness` | `$XDG_STATE_HOME/harness` |

- Use `dirs` or `directories` crate for cross-platform resolution.
- Distinguish **portable mode** (all state next to binary) from installed mode.

### 3.3 Existing files that may need relocation
- `.agent-harness/` runtime prompt assets: currently project-local; in installed app may ship inside Resources / Program Files / `/usr/share/harness`.
- `configs/harness.example.jsonc`: shipped as default template.
- Provider cassettes / mock fixtures: test-only or shipped? Should probably not ship.
- Auth credential storage: currently not in this crate; likely uses OAuth tokens or env vars. If stored locally, use OS keychain (Keychain / DPAPI / Secret Service).

---

## 4. Auto-update

> Phase-1 stance: no in-binary auto-update engine; rely on the npm wrapper (`npm update -g harness`) and GitHub Releases. This avoids self-replacement complexity, unsigned-binary replacement risks, and a background process. The sections below describe the full surface for later phases.

### 4.1 Update strategies
- **Phase 1: package-manager driven** — `npm update -g harness` re-fetches the wrapper; the wrapper downloads the latest matching binary from GitHub Releases.
- **Phase 2+ options:**
  - **In-place binary replacement**: download new binary, swap atomically, restart.
  - **Side-by-side versions**: keep multiple versions, symlink current, rollback support.
  - **Installer-driven**: open downloaded `.dmg`/`.msi`/`.pkg` and let user re-run.
  - **Store/package-manager driven**: Homebrew, winget, apt, Chocolatey, Snap, Flatpak update.

### 4.2 Rust update crates / patterns (Phase 2+)
- `self_update` — download GitHub release assets, verify signatures, replace binary.
- `tauri-updater` / `updater` plugin only if we later add a webview/native GUI shell.
- Sparkle framework (macOS) for `.app` bundles.
- Squirrel.Windows / `NetSparkle` for Windows.
- Linux: rely on package manager; can check version against API.

### 4.3 Update UX (Phase 2+)
- Background check on launch or periodic polling.
- TUI notification: "A new version is available" with release notes.
- Headless flow: `harness self update [--channel stable|nightly]`.
- Signature verification: Ed25519 or minisign signatures checked before replacement.
- Channel support (stable, beta, nightly).
- Delta / differential updates vs. full download.
- Rollback on checksum/signature failure or startup crash.

### 4.4 macOS-specific
- Code-sign / notarize the updater too.
- Handle app bundle replacement vs. single binary.
- Avoid app translocation (`__osx_is_first_launch` / mover).

### 4.5 Windows-specific
- Running EXE cannot overwrite itself; use rename-then-restart, updater helper, or scheduled task.
- SmartScreen warnings if new binary is unsigned.

---

## 5. PATH integration

- Add install directory to system/user `PATH`:
  - macOS/Linux: symlink into `/usr/local/bin` or append `~/.local/bin` to shell rc files.
  - Windows: modify `HKCU\Environment\Path` or use MSI `Environment` table.
- Provide shell completions generation command:
  - `harness completions bash|zsh|fish|powershell|elvish`.
  - Installers can drop generated files into standard completion directories.
- Optional `harness` shell alias or wrapper script.
- Ensure PATH change is picked up without reboot (macOS `path_helper`, Windows broadcast `WM_SETTINGCHANGE`).

---

## 6. Registry (Windows) & Launch Services (macOS)

### 6.1 Windows Registry
- Install location: `HKLM` or `HKCU` `Software\harness`.
- Uninstall entry for Add/Remove Programs (`DisplayName`, `UninstallString`, `Version`).
- `Path` environment modification.
- File associations / ProgIDs if we register `.harness` or `.jsonc` file handling.
- Protocol handler: `harness://` URI scheme registration.
- Auto-start / Run key for background updater or tray agent.

### 6.2 macOS Launch Services
- `Info.plist`: `CFBundleIdentifier`, `CFBundleName`, `LSUIElement` (background), `LSMinimumSystemVersion`.
- URL scheme: `CFBundleURLTypes` for `harness://`.
- File type associations via `CFBundleDocumentTypes`.
- `launchd` plist for background updater or login item.
- `mdls` / `lsregister` to refresh Launch Services database.

### 6.3 Linux desktop integration
- `.desktop` file in `~/.local/share/applications` or `/usr/share/applications`.
- Icon themes / hicolor icon directory.
- `xdg-mime` for URL scheme / file association.
- `xdg-desktop-menu install` for `.desktop` registration.
- systemd user service if background daemon needed.

---

## 7. Code signing, notarization, entitlements

### 7.1 macOS
- Developer ID Application certificate for `.app`/`.dmg`.
- Notarization via `notarytool` (Apple公证).
- Hardened runtime + entitlements:
  - `com.apple.security.cs.allow-jit` (if we ever embed JS/Wasm runtime).
  - `com.apple.security.automation.apple-events` (if scripting other apps).
  - `com.apple.security.network.client` / `server`.
  - Keychain access groups.
- Staple notarization ticket into `.dmg`/`.app`.

### 7.2 Windows
- Authenticode signing with EV or standard code-signing certificate.
- Sign `.exe`, `.msi`, `.dll`.
- SmartScreen reputation and Microsoft Defender SmartScreen.
- Windows Store submission for MSIX.

### 7.3 Linux
- GPG-signed packages (`.asc`, `.deb` Release files, `.rpm` signatures).
- Reproducible builds / SBOM for supply-chain verification.

---

## 8. Logging, crash reporting, telemetry

### 8.1 Logs
- Current crate uses `tracing` + `tracing-appender`.
- For installed app, logs should go to platform log dir (see §3.2).
- Rotation policy: daily / size-limited.
- CLI flag `--log-dir` / `--log-level` for portable/debug use.
- Include binary version, OS version, config digest in log headers.

### 8.2 Crash reporting
- Catch panics with `std::panic::set_hook` and write minidump / stack trace to crash dir.
- Optional upload to Sentry / Crashpad / Breakpad endpoint (opt-in).
- Include event log tail (redacted) in crash report.

### 8.3 Telemetry
- Usage metrics (commands run, TUI sessions, provider used).
- Must be opt-in and configurable in `harness.jsonc`.
- Redact secrets per existing invariants (provider metadata must be redacted).

---

## 9. Security & sandboxing

### 9.1 Sandboxing
- macOS App Sandbox (if App Store target).
- Windows AppContainer via MSIX.
- Linux Flatpak/Snap confinement.
- Note: CLI currently needs broad file-system and network access; sandboxing will require portal/file-picker integration.

### 9.2 Secret storage
- Move OAuth/API tokens from env vars to OS credential store:
  - macOS: Keychain.
  - Windows: Credential Manager / DPAPI.
  - Linux: `secret-service` / `libsecret` / `kwallet`.
- Consider `keyring` crate.

---

## 10. CI/CD release pipeline

### 10.1 Phase-1 pipeline: GitHub Releases + npm wrapper

1. **GitHub Actions matrix** on every version tag:
   - Build `harness` for macOS (x86_64 + aarch64), Windows x86_64, Linux x86_64 (glibc), Linux x86_64 musl (optional).
   - Code-sign macOS and Windows binaries where certificates exist.
   - Attach per-target archives (`harness-{version}-{target}.{tar.gz,zip}`) plus `checksums.txt` and signatures to a GitHub Release.
2. **npm package** (`packages/harness-cli` or separate repo):
   - `package.json` declares `bin.harness` pointing to a JS launcher.
   - `postinstall` downloads the release asset matching the host platform/arch into `~/.cache/harness/dist/{version}/`.
   - Launcher symlinks/copies the binary into `node_modules/.bin/harness` and forwards argv.
   - Supports `--harness-version` override for pinning or testing.
3. **Smoke verification**:
   - `npm pack` + local install test on each target runner.
   - `harness --version`, `harness doctor`, `harness config validate` pass.

### 10.2 Later-phase build matrix
- macOS: x86_64, Apple Silicon (universal binary).
- Windows: x86_64, maybe aarch64.
- Linux: x86_64, aarch64 (musl vs. glibc; compatibility concerns).

### 10.3 Release artifacts (later phases)
- Signed binaries and packages for each target.
- `checksums.txt` / `checksums.txt.sig`.
- SBOM (`cargo-sbom`).
- Release notes from CHANGELOG.

### 10.4 Distribution channels (later phases)
- GitHub Releases (already used by Phase 1).
- Homebrew tap formula.
- `winget` manifest.
- Chocolatey / Scoop packages.
- apt repository / COPR / AUR.
- Snap Store / Flathub.

---

## 11. Uninstall & upgrade hygiene

- MSI / .pkg / package manager handle uninstall automatically.
- Archive install needs an `uninstall` command or script.
- Preserve user config/data on uninstall (offer option).
- Remove shell completion snippets and PATH entries.
- macOS: remove `.harness` quarantine attributes, keychain entries (optional).
- Windows: clean registry, auto-start keys.

---

## 12. Decisions / triage tags

| # | Question | Decision | Phase |
|---|---|---|---|
| 1 | Will `harness` ship as a single terminal binary or a full `.app`/GUI wrapper? | **Terminal binary first.** Native window/tray are explicitly Phase 2+. | 1 |
| 2 | Who owns code-signing certs / Apple Developer account / Windows Authenticode? | **Required before notarized/app-store distribution.** Phase 1 can ship unsigned GitHub Release binaries + npm wrapper; signed binaries are strongly recommended even for Phase 1. | 1 (optional) / 2 |
| 3 | Do we store credentials in OS keychain or keep env-based OAuth? | **Keep env-based OAuth for Phase 1.** OS keychain integration is a later security improvement. | 2 |
| 4 | Which Linux package formats are required for v1? | **None for Phase 1.** npm wrapper covers Linux; apt/rpm/AppImage/Flatpak/Snap are later. | 2+ |
| 5 | Is auto-update required at launch or can it be package-manager-only? | **Package-manager-only for Phase 1** (`npm update -g harness`). In-binary `self_update` is Phase 2. | 1 |
| 6 | Should session/event logs remain project-local or move to a platform data dir? | **Move to XDG data dir by default; keep project-local override.** Covered by Task #1 architecture design. | 1 |
| 7 | Do we need a background updater / tray agent? | **No for Phase 1.** | 2+ |
| 8 | Is telemetry opt-in required from day one? | **No telemetry in Phase 1.** If added later, it must be opt-in and configurable. | 2+ |
| 9 | What is the canonical Phase-1 distribution channel? | **npm wrapper + GitHub Releases.** | 1 |

---

## 13. Files in this repo likely to change

### Phase 1
- `crates/harness-core/src/config/discovery.rs` — integrate `dirs` crate, portable mode, XDG config discovery.
- `crates/harness-core/src/session_paths.rs` — platform data/cache/log paths for sessions, support exports, and event logs.
- `crates/harness/src/cli_config.rs`, `lib.rs` — ensure `--config` and default discovery work under centralized layout; add `harness init` / `harness doctor` checks for XDG paths.
- `.github/workflows/release.yml` — build matrix, artifact signing, GitHub Release creation.
- `packages/harness-cli/` or separate npm wrapper repo — node package, platform downloader, launcher.
- `docs/install.md` or this document — install instructions for `npm install -g harness`.

### Phase 2+
- `crates/harness/Cargo.toml` — add `self_update` or similar behind a feature flag.
- `crates/harness/src/main.rs`, `lib.rs` — `self update`, richer completions install command, `doctor` distribution checks.
- New packaging metadata: `wix/`, `pkg/`, `flatpak/`, `snapcraft.yaml`.
- `scripts/` — additional release build/sign scripts for native installers.

---

*End of surface map.*

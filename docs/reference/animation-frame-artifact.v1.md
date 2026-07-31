# Animation Frame Artifact Schema (v1)

**Status:** Infrastructure foundation for gate `A-ANIMATION`.  
**Does not claim:** full animation parity, pixel identity, or A-ANIMATION pass.

## Purpose

Record **deterministic fixed-tick** terminal-cell frame sequences for functionally
complete animated surfaces (startup idle chrome, streaming wait spinner, cursor
phase when modeled). Captures advance `FakeClock` and AppState animation phase
only — never wall-clock time.

## Evidence root layout

Under a provenance-bearing evidence root (for example
`artifacts/qa-evidence/<YYYYMMDD>-<slug>/`):

```text
animation/
  <surface-id>/
    sequence.frames.json    # AnimationFrameSequence
    plan.json               # optional FixedTickPlan echo
    residual.md             # optional per-surface residual notes
```

Owner unit/integration tests may also write ephemeral sequences under a temp
directory; CI green does not require committing frame JSON.

## Schema: `animation-frame-sequence-v1`

Machine constant: `harness_tui::animation_evidence::ANIMATION_FRAME_SEQUENCE_SCHEMA`.

| Field | Type | Meaning |
|---|---|---|
| `schema_version` | string | Must be `animation-frame-sequence-v1` |
| `surface_id` | string | Stable surface key (e.g. `streaming-wait-spinner`) |
| `width` | u16 | Capture viewport width in cells |
| `height` | u16 | Capture viewport height in cells |
| `tick_ms` | u64 | FakeClock advance applied between consecutive frames |
| `frames` | array | Ordered frames; index `0` is pre-advance state |

### Frame object

| Field | Type | Meaning |
|---|---|---|
| `index` | usize | Zero-based frame index |
| `mono_ms` | u64 | `FakeClock::mono_ms()` at capture |
| `animation_phase` | usize | AppState transcript animation phase |
| `cells` | string | Full terminal cell grid, rows joined by `\n` |

## Capture contract

1. **Deterministic only** — no `Instant::now`, sleeps, or OS time in the capture path.
2. **Double-paint fail-closed** — each frame is rendered twice; differing cell grids abort.
3. **Cross-run equality** — two independent captures with the same plan and fixture must match (`assert_sequences_equal`).
4. **Functionally complete surfaces only** — do not invent animation for incomplete chrome.
5. **Not A-PIXELS** — cell grids only; no PNG/xterm paint comparison in this schema.

## Owner API

| Item | Location |
|---|---|
| Capture / artifact I/O | `crates/harness-tui/src/animation_evidence.rs` |
| Tick advance | `AppState::advance_animation_tick_for_evidence` |
| Phase / active query | `animation_phase_for_evidence`, `has_active_animations_for_evidence` |
| Owner test | `crates/harness-tui/tests/animation_fixed_tick_test.rs` |

## Owner corpus covered (foundation only — still not A-ANIMATION pass)

| Surface id | Owner test | Notes |
|---|---|---|
| `streaming-wait-spinner` | `streaming_wait_spinner_fixed_tick_sequence_is_deterministic` | Braille spinner advances with phase |
| `tool-running-spinner` | `tool_running_spinner_fixed_tick_sequence_is_deterministic` | Running read-tool spinner (distinct surface id) |
| `startup-idle` | `startup_idle_fixed_tick_sequence_is_deterministic` | Stable startup chrome; no phase-driven motion yet |
| `permission-wait` | `permission_wait_fixed_tick_sequence_is_deterministic` | Dock chrome deterministic; spinner frozen while waiting |
| (infra) | `empty_fixed_tick_plan_fails_closed` | Fail-closed empty plan |

## Residual (full A-ANIMATION corpus — not claimed here)

- Startup welcome shimmer / logo motion (if reference has it) — idle cells are stable today
- Cursor blink show/hide sequence vs reference (terminal cursor, not cell-grid phase)
- Tool-running spinner **cadence/parity** vs frozen reference captures (owner determinism only)
- Overlay open/close disclosure frame sequences
- Dedicated permission / question wait **glyph animation** (dock freezes spinner; static chrome only)
- Background task progress choreography
- Reduced-motion / capability fallbacks
- Fixed-tick comparison against frozen reference captures
- Wire into parity manifest owners + evidence root under `artifacts/qa-evidence/.../animation/`
- Timing bounds (`A-TIMING`) and interaction traces (`A-TRACE`)

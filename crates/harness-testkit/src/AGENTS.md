# TESTKIT SOURCE GUIDE

## OVERVIEW

Score 8: a `lib.rs` module boundary, all-Rust source, and measured high symbol/export density justify guidance for this distinct deterministic-test-support domain.

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Script provider/tool boundaries | `fakes.rs` | Seed IDs and capture invocations rather than reaching real services. |
| Create isolated repositories | `workspace.rs` | Use `TestWorkspace` instead of host or user paths. |
| Scan generated evidence | `secret_scanner.rs` | Findings block unsafe persistence. |
| Build simulation summaries | `simulation.rs` | Normalize before fingerprinting or comparison. |
| Index and fingerprint artifacts | `simulation/evidence.rs`, `simulation/fingerprint.rs` | Paths are relative and output is canonical. |
| Validate simulation output | `simulation/validation.rs` | Validate event rows, artifact indexes, and reports together. |

## CONVENTIONS

- Keep this crate runtime-independent; PTY, live-provider, and native-visual workflows belong under `tests/` or binaries.
- Use scripted runners/transports, seeded IDs, and manually advanced clocks for deterministic behavior.
- Normalize JSON before hashing; simulation JSONL starts at sequence 1 and remains contiguous.
- Preserve schema versions, relative artifact paths, provenance, fingerprints, redaction metadata, and invariant outcomes.
- Use `UnwrapOrAbort` only in test-oriented code where aborting is the intended failure mode.

## ANTI-PATTERNS

- Never read credentials from ambient user config or write evidence beneath `$HOME/.config/harness`.
- Do not persist evidence after secret scanning reports authorization, query credentials, keys, tokens, or unsafe symlink components.
- Do not introduce wall-clock sleeps, uncontrolled randomness, process-global environment mutation, or current-directory mutation.
- Do not label offline simulation evidence as live-provider, PTY, shipped-binary, or native-visual coverage.
- Do not let recursive artifact scans escape the dedicated evidence root.

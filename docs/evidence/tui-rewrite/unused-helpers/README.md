# Unused private helper removal

The predecessor is `e421434a`. An explicit library build with `-W dead_code`
reported 69 warnings hidden by the workspace's existing allowance. Source and
caller inspection identified 47 private functions with no callers outside their
own unused chains. Their definitions, ranges, hashes and references were recorded
before removal in `selected.json` and `selected-source.txt`.

The change removes those functions, the unused `StartupCardViewModel`, five
unused constants, stale imports and two empty impl blocks. It removes 555
production lines across 28 files. Tests and stored fields are unchanged. The
complete TUI source tree is now 165,121 lines; the 90,941-line target remains
unmet. Nineteen touched legacy files still exceed 500 lines. `sources.json`
records each file's before/after hashes and line counts.

The removed code includes old session time/date labels, an unused suggestion
predicate, obsolete permission text helpers, unused palette geometry, transcript
accessors, a scrollbar wrapper and a rewind constructor. Their active counterparts
remain. The normal permission display, relative session ages, rewind flow, tool
motion scheduling and public APIs remain in place. `is_suggested` and
`RewindState::new_cancel_offer` used `pub` inside modules that are not publicly
reachable; neither was an external API. The rewind state is stored behind a
crate-visible field.

The matching compiler diagnostic now reports 21 warnings. Test-used helpers,
unused stored fields and other leftovers are deliberately retained for their
own state/rendering replacements. This change does not enable a new lint policy
or claim that all unused code is gone.

## Verification

All 1,635 TUI checks pass, including the independent recorded frame/cursor/intent
reference. All seven gated PTY checks pass. Workspace check, Clippy, formatting
and suite gates pass. No tests were added, altered or removed: code with no
callers has no observable behavior for a new regression test to protect.

The compiler/caller evidence establishes the deletion scope; the existing public
behavior checks protect the working interface. This slice has no new xterm
capture or resource benchmark. It makes no runtime performance claim, and all
previous performance limits and failures remain open. The preceding shortcut
slice's raw performance and 42 paired terminal captures remain available in
[`../key-labels`](../key-labels). They are evidence for that predecessor, not a
fresh measurement of this source.

## Reproduction

From the repository root, reproduce the before/after compiler diagnostic with:

```sh
cargo rustc -p harness-tui --lib -- -W dead_code
```

The crate defines no Cargo features. The diagnostic intentionally checks normal
library code without test-only callers. `selected.json` records full predecessor
identity. Check out that source and run `select.py` from a scratch directory to
rebuild the function-source and reference inventory. `remove.py` validates every
recorded function hash before writing the deletions, then removes the listed
stale declarations/imports. Finish with `cargo fmt --all`.

Copy `checks.py` to scratch and run it from the repository root to repeat TUI,
PTY and quality checks. `checks.json` records the exact commands and environment.
`source.patch.gz` contains the final source diff. `removal.json` verifies that all
47 selected definitions are gone and lists surviving same-name references to
other items, such as the public AppState scroll accessor and theme color fields.
`files.json` hashes every published artifact except itself.

The full state/layout/rendering rewrite, code-reduction target, resource targets,
remaining feature/environment matrix and final independent review are unfinished.

The independent reviewer approved this intermediate deletion commit after
checking all 21 pre-review artifacts, 28 source receipts, frozen snippets, caller
reachability, final diff and verification logs. `review.json` records the
approval; overall rewrite completion and existing acceptance failures remain open.

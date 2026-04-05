---
name: rust-best-practices
description: Repo-bundled Rust guidance fixture for harness skill-loading and live signoff lanes
---

<Purpose>
Provide a small, self-contained Rust guidance skill that the harness can always load from the
repo's project skill root during live verification.
</Purpose>

<Guidance>
- Prefer small, reviewable diffs.
- Keep behavior covered by tests before refactoring.
- Use strong types and explicit error handling over stringly-typed glue.
- Preserve deterministic test behavior and documented harness contracts.
</Guidance>

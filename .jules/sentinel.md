## 2024-05-19 - Internal path traversal bypasses workspace boundary validation
**Learning:** Shell argument validation in `looks_like_shell_path_argument` incorrectly ignored tokens with internal relative path elements (e.g. `foo/../../../etc/passwd`) unless they started with specific path prefixes like `./` or `../`. This allowed relative path traversals to bypass workspace path normalization completely.
**Action:** Always validate arguments containing `/` as potential paths to ensure they don't break out of the workspace, rather than only matching strict prefixes.

## 2024-05-19 - Options and Glob strings also bypass workspace boundary validation
**Learning:** Checking for slashes in arguments is a good start, but shell options (`--git-dir=/etc/passwd`) and paths containing glob characters (`foo/../../../etc/pas*`) also need to be extracted and evaluated. The original code skipped anything starting with `-` or containing glob sequences before looking for paths.
**Action:** Extract the value component of options (e.g. splitting on `=`), and extract the static path prefix before glob sequences, so that the base path is always evaluated against the workspace bounds.

## 2025-06-23 - [Short Flag Embedded Path Boundary Bypass]
**Learning:** Shell path arguments that are embedded directly onto short flags (e.g., `-I/etc/passwd`) without an `=` sign can easily bypass naive prefix-checking boundaries if only `split_once('=')` is used.
**Action:** When validating shell paths, safely parse the payload after a short flag (skipping the `-` and the flag character using `chars().next()` to remain unicode safe) and validate the remaining payload to ensure proper workspace boundary enforcement.

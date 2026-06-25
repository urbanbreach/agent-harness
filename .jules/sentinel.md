## 2024-05-19 - Internal path traversal bypasses workspace boundary validation
**Learning:** Shell argument validation in `looks_like_shell_path_argument` incorrectly ignored tokens with internal relative path elements (e.g. `foo/../../../etc/passwd`) unless they started with specific path prefixes like `./` or `../`. This allowed relative path traversals to bypass workspace path normalization completely.
**Action:** Always validate arguments containing `/` as potential paths to ensure they don't break out of the workspace, rather than only matching strict prefixes.

## 2024-05-19 - Options and Glob strings also bypass workspace boundary validation
**Learning:** Checking for slashes in arguments is a good start, but shell options (`--git-dir=/etc/passwd`) and paths containing glob characters (`foo/../../../etc/pas*`) also need to be extracted and evaluated. The original code skipped anything starting with `-` or containing glob sequences before looking for paths.
**Action:** Extract the value component of options (e.g. splitting on `=`), and extract the static path prefix before glob sequences, so that the base path is always evaluated against the workspace bounds.

## 2025-06-25 - Shell safety bypass for short path options
**Learning:** When parsing paths embedded directly in short shell options (e.g., `-I/etc/passwd`), relying solely on `split_once('=')` leaves them unchecked because there is no equals sign. If the option bypasses validation, it allows operators to escape the workspace boundary via direct path flags.
**Action:** When extracting paths from shell options, explicitly check for short options combined with their arguments (i.e. lengths > 2 starting with `-` but not `--`) and safely advance past the flag using `chars().next()` to extract and validate the actual path payload.

## 2024-05-19 - Internal path traversal bypasses workspace boundary validation
**Learning:** Shell argument validation in `looks_like_shell_path_argument` incorrectly ignored tokens with internal relative path elements (e.g. `foo/../../../etc/passwd`) unless they started with specific path prefixes like `./` or `../`. This allowed relative path traversals to bypass workspace path normalization completely.
**Action:** Always validate arguments containing `/` as potential paths to ensure they don't break out of the workspace, rather than only matching strict prefixes.

## 2024-05-19 - Options and Glob strings also bypass workspace boundary validation
**Learning:** Checking for slashes in arguments is a good start, but shell options (`--git-dir=/etc/passwd`) and paths containing glob characters (`foo/../../../etc/pas*`) also need to be extracted and evaluated. The original code skipped anything starting with `-` or containing glob sequences before looking for paths.
**Action:** Extract the value component of options (e.g. splitting on `=`), and extract the static path prefix before glob sequences, so that the base path is always evaluated against the workspace bounds.
## 2026-06-28 - Short option parsing boundary bypass
**Learning:** Shell tool path extraction relied solely on `split_once('=')` for short options. For flags passed without an equals sign (e.g., `-I/etc/passwd`), this caused the entire argument to be ignored by the path validator, allowing boundaries to be bypassed. Blindly stripping alphanumeric characters also risks mutating legitimate paths.
**Action:** When extracting paths attached directly to short shell options, advance past the dash and the single option character safely (e.g., `chars().next()` twice) and then extract the rest of the string as the path.
